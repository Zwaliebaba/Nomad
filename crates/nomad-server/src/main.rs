//! `nomad-server`: create, run and inspect a Universe (N2 plan §4.1; N3 plan §4.9).
//!
//! ```text
//! nomad-server create       [--seed N] [--ai N] [--difficulty easy|normal|hard]
//!                           [--season-weeks 8..12] [--name TEXT] [--backdate-hours N]
//! nomad-server run          [--universe ID] [--bind ADDR] [--port N]
//! nomad-server status       [--universe ID]
//! nomad-server connect-info [--universe ID] [--host ADDR] [--port N]
//! nomad-server rotate-token [--universe ID] [--certificate]
//! ```
//!
//! Every command takes `--data-dir PATH`; otherwise `NOMAD_DATA` or the per-user data
//! directory is used. `--universe` may be left out when the data directory holds exactly
//! one Universe. `run` listens on 127.0.0.1 and UDP port 31790 unless `--bind`/`NOMAD_BIND`
//! or `--port`/`NOMAD_PORT` say otherwise (D-150), and stops cleanly on Ctrl+C (and SIGTERM
//! on Linux). `connect-info` prints what a client needs to connect, as text and as an
//! import string (D-151). Logs go to stderr; `RUST_LOG` sets the filter.

use std::io::{self, IsTerminal, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use nomad_protocol::{CONNECTION_FILE, ConnectInfo, ConnectionFile};
use nomad_server::{
    BIND_ENV, BUILD_ID, CreateParams, DEFAULT_PORT, DataDir, Difficulty, Identity, NetConfig,
    PORT_ENV, ServerConfig, ServerError, ServerHandle, SystemClock, UniverseLock, create,
    local_connect_address, parse_universe_id, short_build, status, utc_text,
};
use nomad_types::UniverseId;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
usage: nomad-server <command> [options]

commands:
  create   generate a new MVP stage-1 Universe
             --seed N               season seed (random if omitted)
             --ai N                 AI commanders (stage 1: 3)
             --difficulty LEVEL     easy, normal (default) or hard
             --season-weeks N       season length, 8 to 12 weeks (default 8)
             --name TEXT            display name
             --backdate-hours N     start the season N hours ago (to test catch-up)
  run      run a Universe until Ctrl+C
             --universe ID          the Universe (optional if there is only one)
             --bind ADDR            address to listen on (default: NOMAD_BIND, else 127.0.0.1)
             --port N               UDP port (default: NOMAD_PORT, else 31790; 0: any free)
  status   show a Universe without running it
             --universe ID          the Universe (optional if there is only one)
  connect-info
           print the address, fingerprint and token a client needs, and an import string
             --universe ID          the Universe (optional if there is only one)
             --host ADDR            address the client should use (default: where it runs)
             --port N               port the client should use (default: where it runs)
  rotate-token
           replace the token, so imported connection details stop working (server stopped)
             --universe ID          the Universe (optional if there is only one)
             --certificate          replace the certificate and its key too

options for every command:
  --data-dir PATH    data directory (default: NOMAD_DATA, else the per-user data directory)
  --help             this text";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Create(CreateParams),
    Run {
        universe: Option<UniverseId>,
        bind: Option<IpAddr>,
        port: Option<u16>,
    },
    Status(Option<UniverseId>),
    ConnectInfo {
        universe: Option<UniverseId>,
        host: Option<IpAddr>,
        port: Option<u16>,
    },
    RotateToken {
        universe: Option<UniverseId>,
        certificate: bool,
    },
    Help,
}

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    data_dir: Option<PathBuf>,
    command: Command,
}

/// The options of every command, as parsed; [`Options::into_command`] keeps those of the
/// chosen one.
#[derive(Default)]
struct Options {
    universe: Option<UniverseId>,
    params: CreateParams,
    bind: Option<IpAddr>,
    host: Option<IpAddr>,
    port: Option<u16>,
    certificate: bool,
}

impl Options {
    fn into_command(self, command: &str) -> Command {
        let Self {
            universe,
            params,
            bind,
            host,
            port,
            certificate,
        } = self;
        match command {
            "create" => Command::Create(params),
            "run" => Command::Run {
                universe,
                bind,
                port,
            },
            "connect-info" => Command::ConnectInfo {
                universe,
                host,
                port,
            },
            "rotate-token" => Command::RotateToken {
                universe,
                certificate,
            },
            _ => Command::Status(universe),
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let mut args = args.into_iter();
    let Some(name) = args.next() else {
        return Err("no command given".to_owned());
    };
    let mut data_dir = None;
    let mut o = Options::default();
    let command = name.as_str();
    if matches!(command, "--help" | "-h" | "help") {
        return Ok(Cli {
            data_dir,
            command: Command::Help,
        });
    }
    if !matches!(
        command,
        "create" | "run" | "status" | "connect-info" | "rotate-token"
    ) {
        return Err(format!("unknown command {command:?}"));
    }
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        let create_only = || {
            if command == "create" {
                Ok(())
            } else {
                Err(format!("{flag} is an option of create, not {command}"))
            }
        };
        let only = |allowed: &[&str]| {
            if allowed.contains(&command) {
                Ok(())
            } else {
                Err(format!("{flag} is not an option of {command}"))
            }
        };
        match flag.as_str() {
            "--help" | "-h" => {
                return Ok(Cli {
                    data_dir,
                    command: Command::Help,
                });
            }
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            "--universe" if command != "create" => {
                o.universe = Some(parse_universe_id(&value()?).map_err(|e| e.to_string())?);
            }
            "--seed" => {
                create_only()?;
                o.params.seed = Some(number(&value()?)?);
            }
            "--ai" => {
                create_only()?;
                o.params.ai_commanders = number(&value()?)?;
            }
            "--difficulty" => {
                create_only()?;
                o.params.difficulty = value()?.parse::<Difficulty>().map_err(|e| e.to_string())?;
            }
            "--season-weeks" => {
                create_only()?;
                o.params.season_weeks = number(&value()?)?;
            }
            "--name" => {
                create_only()?;
                o.params.name = Some(value()?);
            }
            "--backdate-hours" => {
                create_only()?;
                let hours: u64 = number(&value()?)?;
                o.params.backdate = Duration::from_secs(hours.saturating_mul(3_600));
            }
            "--bind" => {
                only(&["run"])?;
                o.bind = Some(address(&value()?)?);
            }
            "--host" => {
                only(&["connect-info"])?;
                o.host = Some(address(&value()?)?);
            }
            "--port" => {
                only(&["run", "connect-info"])?;
                o.port = Some(number(&value()?)?);
            }
            "--certificate" => {
                only(&["rotate-token"])?;
                o.certificate = true;
            }
            other => return Err(format!("unknown option {other:?} for {command}")),
        }
    }
    Ok(Cli {
        data_dir,
        command: o.into_command(command),
    })
}

fn address(text: &str) -> Result<IpAddr, String> {
    text.parse()
        .map_err(|_| format!("not an IP address: {text:?}"))
}

/// `--bind`, else `NOMAD_BIND`, else 127.0.0.1; `--port`, else `NOMAD_PORT`, else 31790
/// (D-150).
fn net_config(bind: Option<IpAddr>, port: Option<u16>) -> Result<NetConfig, ServerError> {
    let from_env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let bind = match bind {
        Some(bind) => bind,
        None => match from_env(BIND_ENV) {
            Some(text) => address(text.trim())
                .map_err(|e| ServerError::InvalidParameter(format!("{BIND_ENV}: {e}")))?,
            None => NetConfig::default().bind.ip(),
        },
    };
    let port = match port {
        Some(port) => port,
        None => match from_env(PORT_ENV) {
            Some(text) => number(text.trim())
                .map_err(|e| ServerError::InvalidParameter(format!("{PORT_ENV}: {e}")))?,
            None => DEFAULT_PORT,
        },
    };
    Ok(NetConfig::new(SocketAddr::new(bind, port)))
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("not a valid number: {text:?}"))
}

fn init_logging(default_filter: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .with_target(false)
        .init();
}

fn main() -> ExitCode {
    let cli = match parse_args(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            let _ = writeln!(io::stderr(), "nomad-server: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match cli.command {
        Command::Help => {
            writeln!(io::stdout(), "{USAGE}").map_err(|e| ServerError::io("stdout", e))
        }
        Command::Create(params) => {
            init_logging("warn");
            DataDir::resolve(cli.data_dir).and_then(|data| run_create(&data, &params))
        }
        Command::Run {
            universe,
            bind,
            port,
        } => {
            init_logging("info");
            net_config(bind, port).and_then(|net| {
                DataDir::resolve(cli.data_dir).and_then(|data| run_universe(&data, universe, net))
            })
        }
        Command::Status(universe) => {
            init_logging("warn");
            DataDir::resolve(cli.data_dir).and_then(|data| run_status(&data, universe))
        }
        Command::ConnectInfo {
            universe,
            host,
            port,
        } => {
            init_logging("warn");
            DataDir::resolve(cli.data_dir)
                .and_then(|data| run_connect_info(&data, universe, host, port))
        }
        Command::RotateToken {
            universe,
            certificate,
        } => {
            init_logging("warn");
            DataDir::resolve(cli.data_dir)
                .and_then(|data| run_rotate_token(&data, universe, certificate))
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(error = %e, "nomad-server failed");
            let _ = writeln!(io::stderr(), "nomad-server: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_create(data: &DataDir, params: &CreateParams) -> Result<(), ServerError> {
    let created = create(data, params, &SystemClock::new(), BUILD_ID)?;
    let meta = &created.meta;
    let mut out = io::stdout().lock();
    let write = |e| ServerError::io("cannot write to stdout", e);
    match &meta.name {
        Some(name) => writeln!(out, "Created Universe {} \"{name}\"", meta.universe_id),
        None => writeln!(out, "Created Universe {}", meta.universe_id),
    }
    .map_err(write)?;
    writeln!(
        out,
        "Directory    {}\nSeed         {}\nSeason       {} weeks from {} (tick 0)\n\
         Commanders   1 human, {} AI ({})\nBuild        {}\nState hash   {:016x}\n\
         Run it with: nomad-server run --universe {}",
        created.dir.display(),
        meta.seed,
        meta.season_weeks,
        utc_text(meta.season_start_unix_ms),
        meta.ai_commanders,
        meta.difficulty,
        short_build(&meta.build),
        created.state_hash,
        meta.universe_id,
    )
    .map_err(write)
}

fn run_status(data: &DataDir, universe: Option<UniverseId>) -> Result<(), ServerError> {
    let dir = data.select(universe)?;
    let report = status::inspect(&dir, &SystemClock::new(), BUILD_ID)?;
    writeln!(io::stdout(), "{report}").map_err(|e| ServerError::io("cannot write to stdout", e))
}

fn run_universe(
    data: &DataDir,
    universe: Option<UniverseId>,
    net: NetConfig,
) -> Result<(), ServerError> {
    let dir = data.select(universe)?;
    let config = ServerConfig {
        net: Some(net),
        ..ServerConfig::new(dir)
    };
    let server = ServerHandle::start(config, Arc::new(SystemClock::new()))?;
    if let Err(e) = signals::watch(server.shutdown_trigger()) {
        let _ = server.shutdown();
        return Err(ServerError::io("cannot install the Ctrl+C handler", e));
    }
    info!(
        universe = %server.start_report().meta.universe_id,
        build = short_build(BUILD_ID),
        endpoint = %server
            .start_report()
            .endpoint
            .map_or_else(|| "none".to_owned(), |a| a.to_string()),
        "running; press Ctrl+C to stop"
    );
    let report = server.join()?;
    info!(tick = report.tick.get(), "Universe saved");
    Ok(())
}

/// The connection file of a running server in `dir`, if there is one: the lock held and
/// `server.json` readable (N3 plan §4.2).
fn running_connection(dir: &Path) -> Option<ConnectionFile> {
    if !UniverseLock::is_held(dir).unwrap_or(false) {
        return None;
    }
    let text = std::fs::read_to_string(dir.join(CONNECTION_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn run_connect_info(
    data: &DataDir,
    universe: Option<UniverseId>,
    host: Option<IpAddr>,
    port: Option<u16>,
) -> Result<(), ServerError> {
    let dir = data.select(universe)?;
    let meta = status::inspect(&dir, &SystemClock::new(), BUILD_ID)?.meta;
    let identity = Identity::load_or_create(&dir)?;
    // Where it runs, else where `run` would listen with this environment (D-150).
    let running = running_connection(&dir);
    let listen = match &running {
        Some(file) => file.address,
        None => local_connect_address(net_config(None, None)?.bind),
    };
    let address = SocketAddr::new(
        host.unwrap_or_else(|| listen.ip()),
        port.unwrap_or_else(|| listen.port()),
    );
    let info = ConnectInfo {
        address,
        fingerprint: identity.fingerprint(),
        token: identity.token(),
        universe: Some(meta.universe_id.clone()),
    };
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Universe     {}\nAddress      {}  ({})\nFingerprint  {}\nToken        {}\nImport       {}",
        meta.universe_id,
        info.address,
        if running.is_some() {
            "running"
        } else {
            "not running"
        },
        info.fingerprint,
        info.token.as_str(),
        info.to_uri(),
    )
    .map_err(|e| ServerError::io("cannot write to stdout", e))?;
    let _ = writeln!(
        io::stderr(),
        "The token lets anyone who has it play this Universe. Keep it private; \
         `nomad-server rotate-token` replaces it."
    );
    Ok(())
}

fn run_rotate_token(
    data: &DataDir,
    universe: Option<UniverseId>,
    certificate: bool,
) -> Result<(), ServerError> {
    let dir = data.select(universe)?;
    // Only while stopped: the running server holds the old identity (D-151).
    let lock = UniverseLock::acquire(&dir).map_err(|e| match e {
        ServerError::Locked { .. } => ServerError::InvalidParameter(
            "the Universe is running; stop it first, then rotate the token".to_owned(),
        ),
        other => other,
    })?;
    let identity = Identity::rotate(&dir, certificate)?;
    drop(lock);
    let what = if certificate {
        "token and certificate"
    } else {
        "token"
    };
    writeln!(
        io::stdout(),
        "Replaced the {what}. Fingerprint {}.\nClients must import the new connection \
         details: nomad-server connect-info",
        identity.fingerprint()
    )
    .map_err(|e| ServerError::io("cannot write to stdout", e))
}

/// Clean shutdown on Ctrl+C and the platform's other stop signals (N2 plan §6 task 7).
mod signals {
    use std::future::poll_fn;
    use std::io;
    use std::task::{Context, Poll};

    use nomad_server::ShutdownTrigger;
    use tracing::info;

    /// Installs the handlers and watches them on a thread of their own. Every signal
    /// requests a clean shutdown. A repeated signal does not cut the shutdown short: tools
    /// such as `timeout` deliver one signal twice, and the final snapshot takes
    /// milliseconds.
    pub fn watch(trigger: ShutdownTrigger) -> io::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()?;
        // Register inside the runtime, before the server is announced, so that an early
        // Ctrl+C is not lost and registration errors are reported.
        let mut listeners = runtime.block_on(async { Listeners::new() })?;
        std::thread::Builder::new()
            .name("nomad-signals".to_owned())
            .spawn(move || {
                runtime.block_on(async move {
                    let mut stopping = false;
                    loop {
                        let signal = poll_fn(|cx| listeners.poll(cx)).await;
                        if stopping {
                            info!(signal, "already stopping");
                        } else {
                            info!(signal, "stopping: final snapshot, then exit");
                            stopping = true;
                        }
                        trigger.shutdown();
                    }
                });
            })?;
        Ok(())
    }

    #[cfg(unix)]
    struct Listeners {
        interrupt: tokio::signal::unix::Signal,
        terminate: tokio::signal::unix::Signal,
        hangup: tokio::signal::unix::Signal,
    }

    #[cfg(unix)]
    impl Listeners {
        fn new() -> io::Result<Self> {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
                hangup: signal(SignalKind::hangup())?,
            })
        }

        fn poll(&mut self, cx: &mut Context<'_>) -> Poll<&'static str> {
            if self.interrupt.poll_recv(cx).is_ready() {
                Poll::Ready("SIGINT")
            } else if self.terminate.poll_recv(cx).is_ready() {
                Poll::Ready("SIGTERM")
            } else if self.hangup.poll_recv(cx).is_ready() {
                Poll::Ready("SIGHUP")
            } else {
                Poll::Pending
            }
        }
    }

    #[cfg(windows)]
    struct Listeners {
        interrupt: tokio::signal::windows::CtrlC,
        break_key: tokio::signal::windows::CtrlBreak,
        close: tokio::signal::windows::CtrlClose,
    }

    #[cfg(windows)]
    impl Listeners {
        fn new() -> io::Result<Self> {
            use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
            Ok(Self {
                interrupt: ctrl_c()?,
                break_key: ctrl_break()?,
                close: ctrl_close()?,
            })
        }

        fn poll(&mut self, cx: &mut Context<'_>) -> Poll<&'static str> {
            if self.interrupt.poll_recv(cx).is_ready() {
                Poll::Ready("Ctrl+C")
            } else if self.break_key.poll_recv(cx).is_ready() {
                Poll::Ready("Ctrl+Break")
            } else if self.close.poll_recv(cx).is_ready() {
                // Windows ends the process a few seconds after a console close; a stage-1
                // final snapshot takes milliseconds.
                Poll::Ready("console close")
            } else {
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, String> {
        parse_args(args.iter().map(|&a| a.to_owned()))
    }

    #[test]
    fn create_options() {
        let cli = parse(&[
            "create",
            "--seed",
            "42",
            "--difficulty",
            "hard",
            "--season-weeks",
            "10",
            "--name",
            "Kestrel",
            "--backdate-hours",
            "6",
            "--data-dir",
            "d",
        ])
        .unwrap();
        assert_eq!(cli.data_dir, Some(PathBuf::from("d")));
        let Command::Create(params) = cli.command else {
            panic!("not create");
        };
        assert_eq!(params.seed, Some(42));
        assert_eq!(params.difficulty, Difficulty::Hard);
        assert_eq!(params.season_weeks, 10);
        assert_eq!(params.name.as_deref(), Some("Kestrel"));
        assert_eq!(params.backdate, Duration::from_hours(6));
    }

    #[test]
    fn run_and_status_take_a_universe() {
        let cli = parse(&["run", "--universe", "0123abcd"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Run {
                universe: Some(parse_universe_id("0123abcd").unwrap()),
                bind: None,
                port: None,
            }
        );
        assert_eq!(parse(&["status"]).unwrap().command, Command::Status(None));
        assert_eq!(parse(&["--help"]).unwrap().command, Command::Help);
    }

    #[test]
    fn network_options() {
        let cli = parse(&["run", "--bind", "0.0.0.0", "--port", "0"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Run {
                universe: None,
                bind: Some("0.0.0.0".parse().unwrap()),
                port: Some(0),
            }
        );
        let cli = parse(&["connect-info", "--host", "::1", "--port", "5"]).unwrap();
        assert_eq!(
            cli.command,
            Command::ConnectInfo {
                universe: None,
                host: Some("::1".parse().unwrap()),
                port: Some(5),
            }
        );
        let cli = parse(&["rotate-token", "--certificate"]).unwrap();
        assert_eq!(
            cli.command,
            Command::RotateToken {
                universe: None,
                certificate: true,
            }
        );
        // Explicit flags win over the environment; without either, the defaults (D-150).
        let net = net_config(Some("::1".parse().unwrap()), Some(7)).unwrap();
        assert_eq!(net.bind, "[::1]:7".parse().unwrap());
    }

    #[test]
    fn mistakes_are_usage_errors() {
        for args in [
            &[][..],
            &["fly"],
            &["run", "--seed", "1"],
            &["create", "--universe", "0123abcd"],
            &["create", "--seed"],
            &["create", "--seed", "x"],
            &["run", "--universe", "bad"],
            &["status", "--bogus"],
            &["run", "--bind", "localhost"],
            &["run", "--port", "70000"],
            &["status", "--port", "1"],
            &["create", "--bind", "0.0.0.0"],
            &["run", "--certificate"],
            &["connect-info", "--bind", "0.0.0.0"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
}
