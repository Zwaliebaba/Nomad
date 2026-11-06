//! The network endpoint in-process (N3 plan §7): handshake, commands and their outcomes,
//! limits, the second-session rule, season end, and connect and disconnect in the journal.
//! The server runs on a fake clock; the client is `nomad-link` on a current-thread runtime.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nomad_link::{Connection, LinkError, connect, read_connection_file};
use nomad_protocol::{
    AuthToken, ByeReason, ClientMessage, CommandOutcome, ConnectInfo, PROTOCOL_VERSION,
    RejectReason, ServerMessage,
};
use nomad_server::{
    FakeClock, HUMAN_COMMANDER, Identity, Limits, NetConfig, ServerConfig, ServerHandle,
    parse_universe_id,
};
use nomad_sim::{CommanderInputKind, Tick};
use nomad_types::{Command, HoldingId, HoldingOrder, InvalidReason, SimId};
use tokio::runtime::Runtime;

use crate::common::{
    T0, TIMEOUT, TestDir, config, create_universe, journaled_commander_inputs, snapshot_state,
};

const BUILD: &str = "net-test";

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// Short timeouts and a small burst, so the limits are quick to test.
fn test_limits() -> Limits {
    Limits {
        hello_timeout: Duration::from_millis(500),
        auth_timeout: Duration::from_millis(500),
        commands_per_second: 1,
        command_burst: 3,
        ..Limits::default()
    }
}

fn start_net(dir: &Path, clock: &FakeClock, limits: Limits) -> ServerHandle {
    let net = NetConfig {
        limits,
        ..NetConfig::localhost(0)
    };
    let config = ServerConfig {
        net: Some(net),
        ..config(dir)
    };
    ServerHandle::start(config, Arc::new(clock.clone())).expect("server starts")
}

fn start_live_net(dir: &Path, clock: &FakeClock) -> ServerHandle {
    let server = start_net(dir, clock, test_limits());
    server.wait_until_live(TIMEOUT).expect("live");
    server
}

/// The human's mother station, and a holding of another commander (tick-0 snapshot).
fn holdings(dir: &Path) -> (HoldingId, HoldingId) {
    let state = snapshot_state(dir, 0);
    let mine = state
        .holdings()
        .find(|(_, h)| h.owner == HUMAN_COMMANDER)
        .map(|(id, _)| id)
        .expect("the human has a holding");
    let theirs = state
        .holdings()
        .find(|(_, h)| h.owner != HUMAN_COMMANDER)
        .map(|(id, _)| id)
        .expect("an AI has a holding");
    (mine, theirs)
}

fn launch(holding: HoldingId) -> Command {
    Command::Holding {
        holding,
        order: HoldingOrder::LaunchReady,
    }
}

/// Advances the fake clock a tick at a time until a message arrives.
fn next_message(
    rt: &Runtime,
    server: &ServerHandle,
    clock: &FakeClock,
    connection: &mut Connection,
) -> ServerMessage {
    for _ in 0..200 {
        let got = rt.block_on(async {
            tokio::time::timeout(Duration::from_millis(50), connection.recv()).await
        });
        if let Ok(message) = got {
            return message.expect("a message");
        }
        let next = server.status().tick.next();
        clock.advance(Duration::from_millis(100));
        assert!(server.wait_for_tick(next, TIMEOUT), "tick {next:?}");
    }
    panic!("no message arrived");
}

fn result(message: ServerMessage) -> (u64, CommandOutcome) {
    match message {
        ServerMessage::CommandResult { client_seq, result } => (client_seq, result),
        other => panic!("expected a command result, got {other:?}"),
    }
}

#[test]
fn handshake_welcome_and_ping() {
    let dir = TestDir::new("net-welcome");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 1, &clock);
    let server = start_live_net(&created.dir, &clock);
    let endpoint = server.start_report().endpoint.expect("listening");
    assert!(endpoint.ip().is_loopback(), "{endpoint}");

    // The connection file names the endpoint and the stored identity (D-120, D-151).
    let info = read_connection_file(&created.dir).expect("connection file");
    let identity = Identity::load(&created.dir).unwrap().expect("identity");
    assert_eq!(info.address, endpoint);
    assert_eq!(info.fingerprint, identity.fingerprint());
    assert_eq!(info.token, identity.token());

    let rt = runtime();
    let (mut connection, welcome) = rt.block_on(connect(&info, BUILD)).expect("connects");
    assert_eq!(welcome.commander, HUMAN_COMMANDER);
    assert_eq!(
        welcome.universe,
        parse_universe_id(&created.meta.universe_id).unwrap()
    );
    assert_eq!(
        welcome.clock.season_start_unix_ms(),
        created.meta.season_start_unix_ms
    );
    rt.block_on(async {
        connection
            .send(&ClientMessage::Ping { nonce: 4_242 })
            .await
            .unwrap();
        assert_eq!(
            connection.recv().await.unwrap(),
            ServerMessage::Pong { nonce: 4_242 }
        );
        connection.close().await;
    });
    server.shutdown().unwrap();
    // A clean stop removes the connection file.
    assert!(!created.dir.join(nomad_protocol::CONNECTION_FILE).exists());
}

#[test]
fn a_version_mismatch_and_a_wrong_token_are_refused() {
    let dir = TestDir::new("net-refused");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 2, &clock);
    let server = start_live_net(&created.dir, &clock);
    let info = read_connection_file(&created.dir).unwrap();
    let rt = runtime();
    rt.block_on(async {
        let mut old = Connection::open(&info).await.unwrap();
        old.send(&ClientMessage::Hello {
            protocol: 1,
            build: "n1 client".to_owned(),
        })
        .await
        .unwrap();
        assert_eq!(
            old.recv().await.unwrap(),
            ServerMessage::Bye {
                reason: ByeReason::VersionMismatch {
                    server: PROTOCOL_VERSION
                }
            }
        );

        let wrong = ConnectInfo {
            token: AuthToken::new("0".repeat(64)),
            ..info.clone()
        };
        match connect(&wrong, BUILD).await {
            Err(LinkError::Bye(ByeReason::AuthenticationFailed)) => {}
            other => panic!("expected AuthenticationFailed, got {other:?}"),
        }

        // A wrong fingerprint fails the TLS handshake: the pin holds.
        let impostor = ConnectInfo {
            fingerprint: nomad_protocol::Fingerprint([0; 32]),
            ..info.clone()
        };
        assert!(matches!(
            Connection::open(&impostor).await,
            Err(LinkError::Connect(_))
        ));
    });
    server.shutdown().unwrap();
}

#[test]
fn commands_get_their_outcomes() {
    let dir = TestDir::new("net-commands");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 3, &clock);
    let (mine, theirs) = holdings(&created.dir);
    // The default limits: four quick commands stay within the burst.
    let server = start_net(&created.dir, &clock, Limits::default());
    server.wait_until_live(TIMEOUT).expect("live");
    let info = read_connection_file(&created.dir).unwrap();
    let rt = runtime();
    let (mut connection, _) = rt.block_on(connect(&info, BUILD)).unwrap();
    let sent_at = server.status().tick;

    let send = |connection: &mut Connection, client_seq, command| {
        rt.block_on(connection.send(&ClientMessage::Command {
            client_seq,
            command,
        }))
        .unwrap();
    };
    // Valid: the start kit is docked at the mother station (D-112).
    send(&mut connection, 1, launch(mine));
    let (seq, outcome) = result(next_message(&rt, &server, &clock, &mut connection));
    assert_eq!(seq, 1);
    let CommandOutcome::Applied { tick } = outcome else {
        panic!("not applied: {outcome:?}");
    };
    assert!(tick > sent_at && tick <= server.status().tick, "{tick:?}");

    // Invalid, with a reason the client may learn.
    send(&mut connection, 2, launch(mine));
    assert_eq!(
        result(next_message(&rt, &server, &clock, &mut connection)),
        (
            2,
            CommandOutcome::Rejected {
                reason: RejectReason::Invalid {
                    reason: InvalidReason::NothingToLaunch
                }
            }
        )
    );
    // Someone else's holding and a holding that does not exist read the same (D-123).
    send(&mut connection, 3, launch(theirs));
    send(
        &mut connection,
        4,
        launch(HoldingId(SimId::from_raw(9_999_999))),
    );
    for expected in [3, 4] {
        assert_eq!(
            result(next_message(&rt, &server, &clock, &mut connection)),
            (
                expected,
                CommandOutcome::Rejected {
                    reason: RejectReason::Invalid {
                        reason: InvalidReason::NotFound
                    }
                }
            )
        );
    }
    rt.block_on(connection.close());
    server.shutdown().unwrap();
}

#[test]
fn the_rate_limit_and_protocol_violations_are_enforced() {
    let dir = TestDir::new("net-limits");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 4, &clock);
    let (_, theirs) = holdings(&created.dir);
    let server = start_live_net(&created.dir, &clock);
    let info = read_connection_file(&created.dir).unwrap();
    let rt = runtime();
    rt.block_on(async {
        // Burst of 3 at 1 per second: of 6 commands at once, some are rate limited, at once.
        let (mut connection, _) = connect(&info, BUILD).await.unwrap();
        for client_seq in 1..=6 {
            connection
                .send(&ClientMessage::Command {
                    client_seq,
                    command: launch(theirs),
                })
                .await
                .unwrap();
        }
        let mut limited = 0;
        while let Ok(Ok(message)) =
            tokio::time::timeout(Duration::from_millis(300), connection.recv()).await
        {
            if result(message).1
                == (CommandOutcome::Rejected {
                    reason: RejectReason::RateLimited,
                })
            {
                limited += 1;
            }
        }
        assert!(limited >= 2, "{limited} rate limited");

        // A client_seq that does not increase ends the session.
        connection
            .send(&ClientMessage::Command {
                client_seq: 6,
                command: launch(theirs),
            })
            .await
            .unwrap();
        assert_eq!(
            connection.recv().await.unwrap(),
            ServerMessage::Bye {
                reason: ByeReason::ProtocolViolation
            }
        );

        // So does a malformed frame, and a second Hello.
        for bad in [
            vec![3, 0, 0, 0, 0xFF, 0xFF, 0xFF],
            frame(&ClientMessage::Hello {
                protocol: PROTOCOL_VERSION,
                build: "again".to_owned(),
            }),
        ] {
            let (mut connection, _) = connect(&info, BUILD).await.unwrap();
            connection.send_bytes(&bad).await.unwrap();
            assert_eq!(
                connection.recv().await.unwrap(),
                ServerMessage::Bye {
                    reason: ByeReason::ProtocolViolation
                }
            );
        }
    });
    server.shutdown().unwrap();
}

fn frame(message: &ClientMessage) -> Vec<u8> {
    let mut out = Vec::new();
    nomad_protocol::encode_frame(message, &mut out).expect("the message encodes");
    out
}

#[test]
fn garbage_and_silence_never_hurt_the_server() {
    let dir = TestDir::new("net-fuzz");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 5, &clock);
    let server = start_live_net(&created.dir, &clock);
    let info = read_connection_file(&created.dir).unwrap();
    let rt = runtime();
    rt.block_on(async {
        // Random bytes before and after the handshake (a small LCG, so runs repeat).
        let mut seed: u64 = 0x5EED;
        let mut random = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            u8::try_from(seed >> 56).unwrap_or(0)
        };
        for round in 0..12 {
            let mut connection = Connection::open(&info).await.unwrap();
            if round % 2 == 1 {
                connection.handshake(&info.token, BUILD).await.unwrap();
            }
            let len = 1 + usize::from(random()) % 40;
            let bytes: Vec<u8> = (0..len).map(|_| random()).collect();
            let _ = connection.send_bytes(&bytes).await;
            // Either a Bye, or the server waits for the rest and times out.
            let answer = tokio::time::timeout(Duration::from_secs(3), connection.recv()).await;
            match answer {
                Ok(Ok(ServerMessage::Bye { reason })) => assert!(
                    matches!(
                        reason,
                        ByeReason::ProtocolViolation
                            | ByeReason::HandshakeTimeout
                            | ByeReason::AuthenticationFailed
                            | ByeReason::VersionMismatch { .. }
                    ),
                    "{reason:?}"
                ),
                Ok(Ok(ServerMessage::CommandResult { .. }) | Err(_)) | Err(_) => {}
                Ok(Ok(other)) => panic!("unexpected {other:?}"),
            }
        }
        // Half a frame and then silence: the handshake times out.
        let mut slow = Connection::open(&info).await.unwrap();
        slow.send_bytes(&[200, 0]).await.unwrap();
        assert_eq!(
            slow.recv().await.unwrap(),
            ServerMessage::Bye {
                reason: ByeReason::HandshakeTimeout
            }
        );
        // The server still serves.
        let (mut connection, _) = connect(&info, BUILD).await.unwrap();
        connection
            .send(&ClientMessage::Ping { nonce: 1 })
            .await
            .unwrap();
        assert_eq!(
            connection.recv().await.unwrap(),
            ServerMessage::Pong { nonce: 1 }
        );
    });
    assert_eq!(server.status().phase, nomad_server::Phase::Live);
    server.shutdown().unwrap();
}

#[test]
fn a_new_session_replaces_the_old_one_and_the_journal_shows_one_connection() {
    let dir = TestDir::new("net-replace");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 6, &clock);
    let server = start_live_net(&created.dir, &clock);
    let info = read_connection_file(&created.dir).unwrap();
    let rt = runtime();
    let (mut first, _) = rt.block_on(connect(&info, BUILD)).unwrap();
    let (mut second, _) = rt.block_on(connect(&info, BUILD)).unwrap();
    assert_eq!(
        rt.block_on(first.recv()).unwrap(),
        ServerMessage::Bye {
            reason: ByeReason::Replaced
        }
    );
    // The newer session works.
    rt.block_on(async {
        second
            .send(&ClientMessage::Ping { nonce: 2 })
            .await
            .unwrap();
        assert_eq!(
            second.recv().await.unwrap(),
            ServerMessage::Pong { nonce: 2 }
        );
    });
    // Let the inputs apply, then end the session.
    let tick = server.status().tick;
    crate::common::run_live_until(&server, &clock, tick.get() + 5);
    rt.block_on(second.close());
    let after = server.status().tick;
    // The session's end reaches the hub asynchronously (the QUIC close travels first), so
    // keep ticking until it is journaled, against a real-time deadline.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut disconnected = false;
    let mut step = 0;
    while std::time::Instant::now() < deadline {
        step += 1;
        crate::common::run_live_until(&server, &clock, after.get() + step);
        server.flush().unwrap();
        if journaled_commander_inputs(&created.dir)
            .iter()
            .any(|(_, input)| input.kind == CommanderInputKind::Disconnected)
        {
            disconnected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(disconnected, "the session's end was not journaled");
    let inputs = journaled_commander_inputs(&created.dir);
    let kinds: Vec<&CommanderInputKind> = inputs.iter().map(|(_, input)| &input.kind).collect();
    // One connection for both sessions (D-124), then one disconnection.
    assert_eq!(
        kinds,
        [
            &CommanderInputKind::Connected,
            &CommanderInputKind::Disconnected
        ]
    );
    assert!(inputs.iter().all(|(_, i)| i.commander == HUMAN_COMMANDER));
    server.shutdown().unwrap();
}

#[test]
fn after_the_season_end_commands_are_refused_and_the_session_stays() {
    let dir = TestDir::new("net-season-end");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 7, &clock);
    let (mine, _) = holdings(&created.dir);
    clock.advance(Duration::from_mins(1));
    let net = NetConfig {
        limits: test_limits(),
        ..NetConfig::localhost(0)
    };
    let config = ServerConfig {
        net: Some(net),
        season_end: Some(Tick::new(300)),
        ..config(&created.dir)
    };
    let server = ServerHandle::start(config, Arc::new(clock.clone())).unwrap();
    assert!(
        server
            .wait_for(TIMEOUT, |s| s.phase == nomad_server::Phase::SeasonOver)
            .is_some()
    );
    let info = read_connection_file(&created.dir).unwrap();
    let rt = runtime();
    rt.block_on(async {
        let (mut connection, welcome) = connect(&info, BUILD).await.unwrap();
        assert_eq!(welcome.tick, Tick::new(300));
        connection
            .send(&ClientMessage::Command {
                client_seq: 1,
                command: launch(mine),
            })
            .await
            .unwrap();
        assert_eq!(
            connection.recv().await.unwrap(),
            ServerMessage::CommandResult {
                client_seq: 1,
                result: CommandOutcome::Rejected {
                    reason: RejectReason::SeasonOver
                }
            }
        );
        connection
            .send(&ClientMessage::Ping { nonce: 3 })
            .await
            .unwrap();
        assert_eq!(
            connection.recv().await.unwrap(),
            ServerMessage::Pong { nonce: 3 }
        );
    });
    server.shutdown().unwrap();
}

#[test]
fn a_restart_disconnects_commanders_restored_as_connected() {
    let dir = TestDir::new("net-restart");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 8, &clock);
    let server = start_live_net(&created.dir, &clock);
    let info = read_connection_file(&created.dir).unwrap();
    let rt = runtime();
    let (connection, _) = rt.block_on(connect(&info, BUILD)).unwrap();
    crate::common::run_live_until(&server, &clock, 20);
    // A clean stop with the session open: the final snapshot still has the commander
    // connected.
    let stopped = server.shutdown().unwrap().tick;
    drop(connection);
    assert!(
        snapshot_state(&created.dir, stopped.get())
            .connected()
            .contains(&HUMAN_COMMANDER)
    );

    // Restart at once (no catch-up): the first tick disconnects the restored commander,
    // before any new session's Connected.
    let server = start_live_net(&created.dir, &clock);
    let (mut again, _) = rt
        .block_on(connect(&info_after_restart(&created.dir), BUILD))
        .unwrap();
    crate::common::run_live_until(&server, &clock, stopped.get() + 10);
    server.flush().unwrap();
    let inputs: Vec<_> = journaled_commander_inputs(&created.dir)
        .into_iter()
        .filter(|(tick, _)| *tick > stopped.get())
        .collect();
    let first = inputs.first().expect("inputs after the restart");
    assert_eq!(first.0, stopped.get() + 1);
    assert_eq!(first.1.kind, CommanderInputKind::Disconnected);
    assert_eq!(first.1.client_seq, 0);
    // The new session reconnected, after it.
    assert!(
        inputs
            .iter()
            .any(|(_, i)| i.kind == CommanderInputKind::Connected && i.client_seq >= 1)
    );
    rt.block_on(async {
        again.send(&ClientMessage::Ping { nonce: 9 }).await.unwrap();
        assert_eq!(
            again.recv().await.unwrap(),
            ServerMessage::Pong { nonce: 9 }
        );
    });
    let stopped_again = server.shutdown().unwrap().tick;
    assert!(
        snapshot_state(&created.dir, stopped_again.get())
            .connected()
            .contains(&HUMAN_COMMANDER)
    );
}

/// The connection details after a restart: the port may change, the identity does not
/// (D-151).
fn info_after_restart(dir: &Path) -> ConnectInfo {
    read_connection_file(dir).expect("connection file")
}

#[test]
fn the_connection_limit_refuses_extra_connections() {
    let dir = TestDir::new("net-cap");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 9, &clock);
    let limits = Limits {
        max_connections: 1,
        ..test_limits()
    };
    let server = start_net(&created.dir, &clock, limits);
    server.wait_until_live(TIMEOUT).unwrap();
    let info = read_connection_file(&created.dir).unwrap();
    let rt = runtime();
    rt.block_on(async {
        let (_held, _) = connect(&info, BUILD).await.unwrap();
        assert!(Connection::open(&info).await.is_err());
    });
    server.shutdown().unwrap();
}
