//! The client–server integration test (Design doc 20 §11, D-089; N3 plan §7): the real
//! `nomad-server` binary on localhost, a headless client through `nomad-link`, a command
//! and its acknowledgement, then a crash, a restart with catch-up, and a reconnect with the
//! same identity (D-151). Runs in CI on Linux and Windows.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nomad_link::{Connection, LinkError, connect, read_connection_file};
use nomad_protocol::{ClientMessage, CommandOutcome, ConnectInfo, ServerMessage};
use nomad_server::HUMAN_COMMANDER;
use nomad_types::{Command as GameCommand, HoldingOrder};
use tokio::runtime::Runtime;

use crate::cli::create;
use crate::common::{TestDir, snapshot_state};

const BUILD: &str = "integration-test";
/// Generous: Debug builds on loaded CI runners.
const WAIT: Duration = Duration::from_mins(1);

/// A running `nomad-server run`, with its log collected on a thread.
struct Running {
    child: Child,
    log: Arc<Mutex<String>>,
}

impl Running {
    /// Starts `run` on an OS-chosen port and waits until it says it is running.
    fn start(data: &Path, id: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_nomad-server"))
            .args(["run", "--universe", id, "--port", "0", "--data-dir"])
            .arg(data)
            .env("RUST_LOG", "info")
            .stderr(Stdio::piped())
            .spawn()
            .expect("nomad-server runs");
        let stderr = child.stderr.take().expect("stderr");
        let log = Arc::new(Mutex::new(String::new()));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let collected = Arc::clone(&log);
        thread::spawn(move || {
            let mut ready = Some(ready_tx);
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if line.contains("press Ctrl+C")
                    && let Some(ready) = ready.take()
                {
                    let _ = ready.send(());
                }
                let mut log = collected.lock().expect("log lock");
                log.push_str(&line);
                log.push('\n');
            }
        });
        let running = Self { child, log };
        assert!(
            ready_rx.recv_timeout(WAIT).is_ok(),
            "the server did not start:\n{}",
            running.log()
        );
        running
    }

    fn log(&self) -> String {
        self.log.lock().expect("log lock").clone()
    }

    /// Stops it like a crash or a power cut.
    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// Connects, retrying while the server starts up.
fn connect_with_retry(rt: &Runtime, info: &ConnectInfo) -> Connection {
    let deadline = Instant::now() + WAIT;
    loop {
        match rt.block_on(connect(info, BUILD)) {
            Ok((connection, welcome)) => {
                assert_eq!(welcome.commander, HUMAN_COMMANDER);
                return connection;
            }
            Err(e) if Instant::now() < deadline => {
                let _: LinkError = e;
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => panic!("cannot connect: {e}"),
        }
    }
}

/// The next message that is not catch-up progress.
fn next_reply(rt: &Runtime, connection: &mut Connection) -> ServerMessage {
    rt.block_on(async {
        loop {
            let message = tokio::time::timeout(WAIT, connection.recv())
                .await
                .expect("a reply in time")
                .expect("a reply");
            if !matches!(message, ServerMessage::CatchUp { .. }) {
                return message;
            }
        }
    })
}

fn universe_dir(data: &Path, id: &str) -> PathBuf {
    data.join("universes").join(id)
}

#[test]
#[ignore = "network: opens sockets on localhost; CI runs fast tests only (D-190), run with --ignored"]
fn a_client_plays_survives_a_crash_and_reconnects() {
    let dir = TestDir::new("client-server");
    let id = create(dir.path(), &[]);
    let universe = universe_dir(dir.path(), &id);
    let mine = snapshot_state(&universe, 0)
        .holdings()
        .find(|(_, h)| h.owner == HUMAN_COMMANDER)
        .map(|(id, _)| id)
        .expect("the human's mother station");
    let rt = runtime();

    // First run: connect, ping, and launch the start kit.
    let server = Running::start(dir.path(), &id);
    let info = read_connection_file(&universe).expect("server.json");
    assert!(info.address.ip().is_loopback(), "{}", info.address);
    let mut connection = connect_with_retry(&rt, &info);
    rt.block_on(connection.send(&ClientMessage::Ping { nonce: 77 }))
        .expect("ping");
    assert_eq!(
        next_reply(&rt, &mut connection),
        ServerMessage::Pong { nonce: 77 }
    );
    rt.block_on(connection.send(&ClientMessage::Command {
        client_seq: 1,
        command: GameCommand::Holding {
            holding: mine,
            order: HoldingOrder::LaunchReady,
        },
    }))
    .expect("command");
    let ServerMessage::CommandResult { client_seq, result } = next_reply(&rt, &mut connection)
    else {
        panic!("expected a command result\n{}", server.log());
    };
    assert_eq!(client_seq, 1);
    let CommandOutcome::Applied { tick: applied } = result else {
        panic!("not applied: {result:?}\n{}", server.log());
    };
    // Wait until the command's tick is surely journaled (commits at least every second,
    // D-048), then crash.
    thread::sleep(Duration::from_millis(1_500));
    server.kill();
    drop(connection);

    // Second run: recovery and catch-up, then the same identity on a new port.
    let server = Running::start(dir.path(), &id);
    let again = read_connection_file(&universe).expect("server.json after the restart");
    assert_eq!(
        again.fingerprint, info.fingerprint,
        "the identity is kept (D-151)"
    );
    assert_eq!(again.token, info.token);
    let mut connection = connect_with_retry(&rt, &again);
    rt.block_on(connection.send(&ClientMessage::Ping { nonce: 78 }))
        .expect("ping");
    assert_eq!(
        next_reply(&rt, &mut connection),
        ServerMessage::Pong { nonce: 78 }
    );
    // The launch survived the crash: launching again finds nothing docked.
    rt.block_on(connection.send(&ClientMessage::Command {
        client_seq: 1,
        command: GameCommand::Holding {
            holding: mine,
            order: HoldingOrder::LaunchReady,
        },
    }))
    .expect("command");
    let reply = next_reply(&rt, &mut connection);
    let log = server.log();
    assert!(
        matches!(
            reply,
            ServerMessage::CommandResult {
                client_seq: 1,
                result: CommandOutcome::Rejected { .. }
            }
        ),
        "the launch at tick {applied:?} was lost: {reply:?}\n{log}"
    );
    assert!(log.contains("listening (QUIC)"), "{log}");
    rt.block_on(connection.close());
    server.kill();
}
