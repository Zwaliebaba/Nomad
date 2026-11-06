//! `nomad-server`: create, run and inspect a Universe (N2 plan §4.1).
//!
//! ```text
//! nomad-server create [--seed N] [--ai N] [--difficulty easy|normal|hard]
//!                     [--season-weeks 8..12] [--name TEXT] [--backdate-hours N]
//! nomad-server run    [--universe ID]
//! nomad-server status [--universe ID]
//! ```
//!
//! Every command takes `--data-dir PATH`; otherwise `NOMAD_DATA` or the per-user data
//! directory is used. `--universe` may be left out when the data directory holds exactly
//! one Universe. `run` stops cleanly on Ctrl+C (and SIGTERM on Linux). Logs go to stderr;
//! `RUST_LOG` sets the filter.

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use nomad_server::{
    BUILD_ID, CreateParams, DataDir, Difficulty, ServerConfig, ServerError, ServerHandle,
    SystemClock, create, parse_universe_id, short_build, status, utc_text,
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
  status   show a Universe without running it
             --universe ID          the Universe (optional if there is only one)

options for every command:
  --data-dir PATH    data directory (default: NOMAD_DATA, else the per-user data directory)
  --help             this text";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Create(CreateParams),
    Run(Option<UniverseId>),
    Status(Option<UniverseId>),
    Help,
}

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    data_dir: Option<PathBuf>,
    command: Command,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let mut args = args.into_iter();
    let Some(name) = args.next() else {
        return Err("no command given".to_owned());
    };
    let mut data_dir = None;
    let mut universe = None;
    let mut params = CreateParams::default();
    let command = name.as_str();
    if matches!(command, "--help" | "-h" | "help") {
        return Ok(Cli {
            data_dir,
            command: Command::Help,
        });
    }
    if !matches!(command, "create" | "run" | "status") {
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
        match flag.as_str() {
            "--help" | "-h" => {
                return Ok(Cli {
                    data_dir,
                    command: Command::Help,
                });
            }
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            "--universe" if command != "create" => {
                universe = Some(parse_universe_id(&value()?).map_err(|e| e.to_string())?);
            }
            "--seed" => {
                create_only()?;
                params.seed = Some(number(&value()?)?);
            }
            "--ai" => {
                create_only()?;
                params.ai_commanders = number(&value()?)?;
            }
            "--difficulty" => {
                create_only()?;
                params.difficulty = value()?.parse::<Difficulty>().map_err(|e| e.to_string())?;
            }
            "--season-weeks" => {
                create_only()?;
                params.season_weeks = number(&value()?)?;
            }
            "--name" => {
                create_only()?;
                params.name = Some(value()?);
            }
            "--backdate-hours" => {
                create_only()?;
                let hours: u64 = number(&value()?)?;
                params.backdate = Duration::from_secs(hours.saturating_mul(3_600));
            }
            other => return Err(format!("unknown option {other:?} for {command}")),
        }
    }
    let command = match command {
        "create" => Command::Create(params),
        "run" => Command::Run(universe),
        _ => Command::Status(universe),
    };
    Ok(Cli { data_dir, command })
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
        Command::Run(universe) => {
            init_logging("info");
            DataDir::resolve(cli.data_dir).and_then(|data| run_universe(&data, universe))
        }
        Command::Status(universe) => {
            init_logging("warn");
            DataDir::resolve(cli.data_dir).and_then(|data| run_status(&data, universe))
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

fn run_universe(data: &DataDir, universe: Option<UniverseId>) -> Result<(), ServerError> {
    let dir = data.select(universe)?;
    let server = ServerHandle::start(ServerConfig::new(dir), Arc::new(SystemClock::new()))?;
    if let Err(e) = signals::watch(server.shutdown_trigger()) {
        let _ = server.shutdown();
        return Err(ServerError::io("cannot install the Ctrl+C handler", e));
    }
    info!(
        universe = %server.start_report().meta.universe_id,
        build = short_build(BUILD_ID),
        "running; press Ctrl+C to stop"
    );
    let report = server.join()?;
    info!(tick = report.tick.get(), "Universe saved");
    Ok(())
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
            Command::Run(Some(parse_universe_id("0123abcd").unwrap()))
        );
        assert_eq!(parse(&["status"]).unwrap().command, Command::Status(None));
        assert_eq!(parse(&["--help"]).unwrap().command, Command::Help);
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
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
}
