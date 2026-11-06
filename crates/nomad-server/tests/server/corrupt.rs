//! Damaged or foreign input gives a clear error, never a panic (N2 plan §7 "No panics";
//! `CODING_STANDARDS.md` §5; D-048, D-099).

use std::path::Path;
use std::sync::Arc;

use nomad_server::{DATABASE_FILE, FakeClock, ServerConfig, ServerError, ServerHandle, status};
use nomad_sim::{CommanderId, CommanderInput, CommanderInputKind, Tick};

use crate::common::{
    BUILD, T0, TestDir, config, create_universe, execute, run_live_until, start_live,
};

fn start_error(dir: &Path, build: &str) -> ServerError {
    let config = ServerConfig {
        build: build.to_owned(),
        ..config(dir)
    };
    match ServerHandle::start(config, Arc::new(FakeClock::new(T0))) {
        Ok(_) => panic!("the server started"),
        Err(e) => e,
    }
}

fn assert_message(error: &ServerError, words: &[&str]) {
    let message = error.to_string();
    for word in words {
        assert!(message.contains(word), "{word:?} not in: {message}");
    }
}

#[test]
fn a_file_that_is_not_a_database_is_reported_as_corrupt() {
    let dir = TestDir::new("corrupt-garbage");
    let universe = dir.data().universes().join("00000000");
    std::fs::create_dir_all(&universe).unwrap();
    std::fs::write(universe.join(DATABASE_FILE), vec![0x5a; 8_192]).unwrap();

    let error = start_error(&universe, BUILD);
    assert!(
        matches!(error, ServerError::CorruptDatabase { .. }),
        "{error}"
    );
    assert_message(&error, &["corrupt", "universe.db"]);
    let error = status::inspect(&universe, &FakeClock::new(T0), BUILD).unwrap_err();
    assert!(
        matches!(error, ServerError::CorruptDatabase { .. }),
        "{error}"
    );
}

#[test]
fn a_damaged_database_page_is_reported_as_corrupt() {
    let dir = TestDir::new("corrupt-page");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 1, &clock);
    let path = created.dir.join(DATABASE_FILE);
    let mut bytes = std::fs::read(&path).unwrap();
    // Keep the header page, overwrite the rest.
    for byte in &mut bytes[4_096..] {
        *byte = 0xa5;
    }
    std::fs::write(&path, bytes).unwrap();
    let error = start_error(&created.dir, BUILD);
    assert!(
        matches!(error, ServerError::CorruptDatabase { .. }),
        "{error:?}"
    );
}

#[test]
fn another_sqlite_database_is_not_a_universe() {
    let dir = TestDir::new("corrupt-foreign");
    let universe = dir.data().universes().join("00000001");
    std::fs::create_dir_all(&universe).unwrap();
    let conn = rusqlite::Connection::open(universe.join(DATABASE_FILE)).unwrap();
    conn.execute_batch("CREATE TABLE notes (text TEXT);")
        .unwrap();
    drop(conn);
    let error = start_error(&universe, BUILD);
    assert!(matches!(error, ServerError::NotAUniverse { .. }), "{error}");
    assert_message(&error, &["not a Nomad Universe"]);
}

#[test]
fn a_missing_universe_is_reported() {
    let dir = TestDir::new("corrupt-missing");
    let error = start_error(&dir.data().universes().join("zzzzzzzz"), BUILD);
    assert!(
        matches!(error, ServerError::UniverseNotFound { .. }),
        "{error}"
    );
}

#[test]
fn a_missing_snapshot_is_reported() {
    let dir = TestDir::new("corrupt-no-snapshot");
    let created = create_universe(&dir, 1, &FakeClock::new(T0));
    execute(&created.dir, "DELETE FROM snapshots;");
    let error = start_error(&created.dir, BUILD);
    assert!(matches!(error, ServerError::NoSnapshot), "{error}");
    assert_message(&error, &["no snapshot"]);
}

#[test]
fn an_unknown_snapshot_format_is_refused() {
    let dir = TestDir::new("corrupt-format");
    let created = create_universe(&dir, 1, &FakeClock::new(T0));
    execute(&created.dir, "UPDATE snapshots SET format = 999;");
    let error = start_error(&created.dir, BUILD);
    assert!(
        matches!(
            error,
            ServerError::UnsupportedSnapshotFormat { found: 999, .. }
        ),
        "{error}"
    );
    assert_message(&error, &["format 999", "D-099"]);
}

#[test]
fn an_unknown_database_layout_is_refused() {
    let dir = TestDir::new("corrupt-layout");
    let created = create_universe(&dir, 1, &FakeClock::new(T0));
    execute(&created.dir, "UPDATE meta SET format = 99;");
    let error = start_error(&created.dir, BUILD);
    assert!(
        matches!(
            error,
            ServerError::UnsupportedDatabaseFormat { found: 99, .. }
        ),
        "{error}"
    );
}

#[test]
fn damaged_snapshot_bytes_are_refused() {
    let dir = TestDir::new("corrupt-bytes");
    let created = create_universe(&dir, 1, &FakeClock::new(T0));
    execute(&created.dir, "UPDATE snapshots SET bytes = x'00';");
    let error = start_error(&created.dir, BUILD);
    assert!(
        matches!(error, ServerError::NoUsableSnapshot { .. }),
        "{error}"
    );
    assert_message(&error, &["no usable snapshot", "tick 0"]);
}

#[test]
fn a_damaged_latest_snapshot_falls_back_to_the_one_before() {
    let dir = TestDir::new("corrupt-fallback");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 1, &clock);
    let server = start_live(&created.dir, &clock);
    run_live_until(&server, &clock, 3_002);
    server
        .submit(nomad_server::Input::Commander(CommanderInput {
            commander: CommanderId(2),
            client_seq: 1,
            kind: CommanderInputKind::Connected,
        }))
        .unwrap();
    run_live_until(&server, &clock, 3_010);
    let (_, expected) = server.state_hash().unwrap();
    server.shutdown().unwrap();

    // Snapshots 3,000 and 3,010 are the latest two; damage the newer one. Its state
    // follows from the older one plus the journal after it (the input of tick 3,003).
    execute(
        &created.dir,
        "UPDATE snapshots SET bytes = x'4e4f4d4144534e5003000000ff' WHERE tick = 3010;",
    );
    let server = start_live(&created.dir, &clock);
    assert_eq!(server.start_report().snapshot_tick, Tick::new(3_000));
    assert_eq!(server.start_report().replayed_entries, 1);
    // Replay ends at tick 3,003; the 7 ticks to the wall clock run back to back.
    assert!(server.wait_for_tick(Tick::new(3_010), crate::common::TIMEOUT));
    let (tick, hash) = server.state_hash().unwrap();
    server.shutdown().unwrap();
    assert_eq!((tick, hash), (Tick::new(3_010), expected));
}

#[test]
fn a_journal_from_another_build_is_refused_until_a_clean_shutdown() {
    let dir = TestDir::new("corrupt-build");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 1, &clock);
    let server = start_live(&created.dir, &clock);
    run_live_until(&server, &clock, 10);
    server
        .submit(nomad_server::Input::Commander(CommanderInput {
            commander: CommanderId(1),
            client_seq: 1,
            kind: CommanderInputKind::Connected,
        }))
        .unwrap();
    run_live_until(&server, &clock, 20);
    server.flush().unwrap();
    drop(server); // crash: the journal after the tick-0 snapshot must be replayed

    let error = start_error(&created.dir, "another-build");
    assert!(
        matches!(error, ServerError::JournalFromOtherBuild { .. }),
        "{error}"
    );
    assert_message(
        &error,
        &[BUILD, "another-build", "D-048", "stop it cleanly"],
    );

    // The build that wrote the journal recovers and stops cleanly; then the other build
    // can take over from the final snapshot (doc 11 §8: upgrades go through a clean stop).
    start_live(&created.dir, &clock).shutdown().unwrap();
    let config = ServerConfig {
        build: "another-build".to_owned(),
        ..config(&created.dir)
    };
    let server = ServerHandle::start(config, Arc::new(clock.clone())).unwrap();
    assert!(server.wait_until_live(crate::common::TIMEOUT).is_some());
    server.shutdown().unwrap();
}

#[test]
fn a_corrupt_journal_entry_is_refused() {
    let dir = TestDir::new("corrupt-journal");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 1, &clock);
    let server = start_live(&created.dir, &clock);
    server
        .submit(nomad_server::Input::Commander(CommanderInput {
            commander: CommanderId(1),
            client_seq: 1,
            kind: CommanderInputKind::Connected,
        }))
        .unwrap();
    run_live_until(&server, &clock, 5);
    server.flush().unwrap();
    drop(server);
    execute(&created.dir, "UPDATE journal SET payload = x'ffffff';");
    let error = start_error(&created.dir, BUILD);
    assert!(
        matches!(error, ServerError::CorruptJournal { .. }),
        "{error}"
    );
    execute(&created.dir, "UPDATE journal SET kind = 'mystery';");
    let error = start_error(&created.dir, BUILD);
    assert!(
        matches!(error, ServerError::CorruptJournal { .. }),
        "{error}"
    );
}
