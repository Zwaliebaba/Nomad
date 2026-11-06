//! The `nomad-server` binary: `create`, `status`, `run` (N2 plan §4.1, §6 tasks 7–8).

use std::path::Path;
use std::process::{Command, Output};

use nomad_server::{DataDir, UniverseLock};

use crate::common::TestDir;

fn nomad_server(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nomad-server"))
        .args(args)
        .arg("--data-dir")
        .arg(data)
        .env_remove("RUST_LOG")
        .output()
        .expect("nomad-server runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Creates a Universe through the binary and returns its ID.
fn create(data: &Path, extra: &[&str]) -> String {
    let mut args = vec!["create", "--seed", "42"];
    args.extend_from_slice(extra);
    let output = nomad_server(data, &args);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let first = text.lines().next().unwrap_or_default();
    let id = first
        .strip_prefix("Created Universe ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("unexpected output: {text}"));
    id.to_owned()
}

#[test]
fn create_then_status() {
    let dir = TestDir::new("cli-create");
    let id = create(dir.path(), &["--name", "Kestrel", "--season-weeks", "10"]);
    assert_eq!(id.len(), 8);
    assert!(
        dir.path()
            .join("universes")
            .join(&id)
            .join("universe.db")
            .is_file()
    );

    // With one Universe, --universe may be left out.
    let output = nomad_server(dir.path(), &["status"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    for expected in [
        format!("Universe     {id} \"Kestrel\""),
        "Server       not running".to_owned(),
        "Seed         42".to_owned(),
        "Season       10 weeks".to_owned(),
        "Commanders   1 human, 3 AI (normal)".to_owned(),
        "Last tick    0".to_owned(),
        "Snapshots    1".to_owned(),
        "Journal      empty".to_owned(),
    ] {
        assert!(text.contains(&expected), "{expected:?} not in:\n{text}");
    }

    // With two, it must be given.
    let second = create(dir.path(), &[]);
    let output = nomad_server(dir.path(), &["status"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("--universe"),
        "{}",
        stderr(&output)
    );
    let output = nomad_server(dir.path(), &["status", "--universe", &second]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(DataDir::new(dir.path()).list().unwrap().len(), 2);
}

#[test]
fn bad_arguments_are_refused() {
    let dir = TestDir::new("cli-bad");
    let output = nomad_server(dir.path(), &["create", "--season-weeks", "13"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("season length"),
        "{}",
        stderr(&output)
    );
    let output = nomad_server(dir.path(), &["create", "--ai", "5"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("AI commanders"),
        "{}",
        stderr(&output)
    );
    let output = nomad_server(dir.path(), &["fly"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("usage"), "{}", stderr(&output));
    let output = nomad_server(dir.path(), &["run", "--universe", "zzzzzzzz"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("no Universe"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn run_is_refused_while_another_server_holds_the_universe() {
    let dir = TestDir::new("cli-lock");
    let id = create(dir.path(), &[]);
    let universe = dir.path().join("universes").join(&id);
    let lock = UniverseLock::acquire(&universe).expect("lock");
    let output = nomad_server(dir.path(), &["run", "--universe", &id]);
    drop(lock);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("already in use"),
        "{}",
        stderr(&output)
    );
    let output = nomad_server(dir.path(), &["status"]);
    assert!(stdout(&output).contains("not running"));
}

/// Ctrl+C on Linux is SIGINT. Windows delivers Ctrl+C to a console process group, which a
/// test cannot target without unsafe code; the shutdown path after the signal is the same.
#[cfg(unix)]
#[test]
fn run_stops_cleanly_on_sigint() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let dir = TestDir::new("cli-sigint");
    let id = create(dir.path(), &[]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_nomad-server"))
        .args(["run", "--universe", &id, "--data-dir"])
        .arg(dir.path())
        .env("RUST_LOG", "info")
        .stderr(Stdio::piped())
        .spawn()
        .expect("nomad-server runs");
    let mut lines = BufReader::new(child.stderr.take().expect("stderr")).lines();
    // Wait for the server to be up and the handler installed; reading blocks, no sleeping.
    let mut log = String::new();
    for line in lines.by_ref() {
        let line = line.expect("log line");
        log.push_str(&line);
        log.push('\n');
        if line.contains("press Ctrl+C") {
            break;
        }
    }
    let kill = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(kill.success());
    for line in lines {
        log.push_str(&line.expect("log line"));
        log.push('\n');
    }
    let status = child.wait().expect("nomad-server exits");
    assert!(status.success(), "exit {status:?}\n{log}");
    assert!(log.contains("signal=\"SIGINT\""), "{log}");
    assert!(log.contains("Universe saved"), "{log}");

    // The final snapshot holds the tick the server stopped at.
    let saved = log
        .split("Universe saved tick=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("no saved tick in:\n{log}"));
    let output = nomad_server(dir.path(), &["status"]);
    let text = stdout(&output);
    assert!(text.contains("not running"), "{text}");
    assert!(text.contains(&format!("Last tick    {saved} ")), "{text}");
    assert!(text.contains(&format!("tick {saved:>10}")), "{text}");
}
