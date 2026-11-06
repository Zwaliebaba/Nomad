//! Shared test support: temporary data directories, a scripted Universe and a pure
//! `nomad-sim` reference run.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use nomad_server::{
    CreateParams, Created, DataDir, FakeClock, Input, ServerConfig, ServerHandle, create,
};
use nomad_sim::galaxy::generate_stage1;
use nomad_sim::snapshot::state_hash;
use nomad_sim::{
    CommanderId, CommanderInput, CommanderInputKind, OperatorInput, SeasonConfig, Tick, TickInputs,
    Ticks, UniverseState, step,
};

/// 2026-11-06T00:00:00Z.
pub const T0: i64 = 1_793_923_200_000;
/// The build identity tests run as.
pub const BUILD: &str = "test-build";
/// Generous: a safety net against hangs, not a pacing device.
pub const TIMEOUT: Duration = Duration::from_mins(5);
/// Largest clock step while live. Larger steps would make the server catch up instead,
/// because it would be more than a minute behind the wall clock.
const LIVE_STEP_TICKS: u64 = 500;

/// A data directory under the system temporary directory, removed when dropped.
pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "nomad-server-test-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temporary directory");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn data(&self) -> DataDir {
        DataDir::new(&self.path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Creates a stage-1 Universe with `seed` at the clock's time, as [`BUILD`].
pub fn create_universe(dir: &TestDir, seed: u64, clock: &FakeClock) -> Created {
    let params = CreateParams {
        seed: Some(seed),
        ..CreateParams::default()
    };
    create(&dir.data(), &params, clock, BUILD).expect("create")
}

/// The test configuration for `universe_dir`: build [`BUILD`], production snapshot interval.
pub fn config(universe_dir: &Path) -> ServerConfig {
    ServerConfig {
        build: BUILD.to_owned(),
        ..ServerConfig::new(universe_dir)
    }
}

/// Starts the Universe with the fake clock.
pub fn start(universe_dir: &Path, clock: &FakeClock) -> ServerHandle {
    ServerHandle::start(config(universe_dir), Arc::new(clock.clone())).expect("server starts")
}

/// Starts the Universe with snapshots only once a day, so retention compacts nothing
/// within a test of hours and the whole journal stays readable.
pub fn start_without_compaction(universe_dir: &Path, clock: &FakeClock) -> ServerHandle {
    let config = ServerConfig {
        snapshot_interval: Ticks::from_days(1),
        ..config(universe_dir)
    };
    ServerHandle::start(config, Arc::new(clock.clone())).expect("server starts")
}

/// Starts the Universe and waits until it is live.
pub fn start_live(universe_dir: &Path, clock: &FakeClock) -> ServerHandle {
    let server = start(universe_dir, clock);
    server.wait_until_live(TIMEOUT).expect("server goes live");
    server
}

/// Moves the fake clock in steps until the live server has computed `tick`.
pub fn run_live_until(server: &ServerHandle, clock: &FakeClock, tick: u64) {
    loop {
        let now = server.status().tick.get();
        if now >= tick {
            assert_eq!(now, tick, "the server ran past tick {tick}");
            return;
        }
        let step = (tick - now).min(LIVE_STEP_TICKS);
        clock.advance(Duration::from_millis(step * 100));
        assert!(
            server.wait_for_tick(Tick::new(now + step), TIMEOUT),
            "tick {} was not reached",
            now + step
        );
    }
}

fn commander(id: u32, seq: u64, kind: CommanderInputKind) -> Input {
    Input::Commander(CommanderInput {
        commander: CommanderId(id),
        client_seq: seq,
        kind,
    })
}

/// Inputs to submit, each when the server has computed the given tick, so that it applies
/// on the next one. Diagnostics give the timers and random streams work to do.
pub fn script() -> Vec<(u64, Input)> {
    use CommanderInputKind::{Connected, Disconnected};
    vec![
        (
            0,
            Input::Operator(OperatorInput::SpawnDiagnostics {
                count: 40,
                mean_interval: Ticks::from_secs(3),
            }),
        ),
        (0, commander(1, 1, Connected)),
        (1_200, commander(2, 1, Connected)),
        (1_200, commander(1, 2, Disconnected)),
        (
            3_500,
            Input::Operator(OperatorInput::SpawnDiagnostics {
                count: 20,
                mean_interval: Ticks::from_secs(7),
            }),
        ),
        (4_200, commander(2, 2, Disconnected)),
        (4_800, commander(3, 1, Connected)),
        (5_500, commander(1, 3, Connected)),
    ]
}

/// Plays the script entries due before `until` on a live server, then runs to `until`.
pub fn play(server: &ServerHandle, clock: &FakeClock, script: &[(u64, Input)], until: u64) {
    for (tick, input) in script {
        let now = server.status().tick.get();
        if *tick < now || *tick >= until {
            continue;
        }
        run_live_until(server, clock, *tick);
        server.submit(input.clone()).expect("input accepted");
    }
    run_live_until(server, clock, until);
}

/// The state hash after `until` ticks of a pure `nomad-sim` run of the Universe created
/// with `seed` at `start_ms`, applying each script entry on the tick after its own.
pub fn reference_hash(seed: u64, start_ms: i64, script: &[(u64, Input)], until: u64) -> u64 {
    let commanders = [
        CommanderId(1),
        CommanderId(2),
        CommanderId(3),
        CommanderId(4),
    ];
    let mut state = generate_stage1(
        SeasonConfig {
            seed,
            season_start_unix_ms: start_ms,
        },
        commanders,
    )
    .expect("generate");
    for tick in 1..=until {
        let mut inputs = TickInputs::default();
        for (at, input) in script {
            if at + 1 == tick {
                match input {
                    Input::Operator(op) => inputs.operator.push(op.clone()),
                    Input::Commander(c) => inputs.commanders.push(c.clone()),
                }
            }
        }
        step(&mut state, &inputs);
    }
    assert_eq!(state.tick(), Tick::new(until));
    assert!(!state.catching_up());
    state_hash(&state).expect("hash")
}

/// The journal rows on disk as `(tick, kind, payload)`.
pub fn journal_rows(universe_dir: &Path) -> Vec<(u64, String, Vec<u8>)> {
    let conn = rusqlite::Connection::open(universe_dir.join(nomad_server::DATABASE_FILE))
        .expect("open database");
    let mut statement = conn
        .prepare("SELECT tick, kind, payload FROM journal ORDER BY tick, seq")
        .expect("query");
    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?.cast_unsigned(),
                row.get(1)?,
                row.get(2)?,
            ))
        })
        .expect("rows")
        .map(|row| row.expect("row"))
        .collect()
}

/// The operator inputs in the journal, with their ticks.
pub fn journaled_operator_inputs(universe_dir: &Path) -> Vec<(u64, OperatorInput)> {
    journal_rows(universe_dir)
        .into_iter()
        .filter(|(_, kind, _)| kind == "operator")
        .map(|(tick, _, payload)| {
            (
                tick,
                postcard::from_bytes(&payload).expect("operator input decodes"),
            )
        })
        .collect()
}

/// The commander inputs in the journal, with their ticks.
pub fn journaled_commander_inputs(universe_dir: &Path) -> Vec<(u64, CommanderInput)> {
    journal_rows(universe_dir)
        .into_iter()
        .filter(|(_, kind, _)| kind == "commander")
        .map(|(tick, _, payload)| {
            (
                tick,
                postcard::from_bytes(&payload).expect("commander input decodes"),
            )
        })
        .collect()
}

/// The Universe state saved in the snapshot of `tick`.
pub fn snapshot_state(universe_dir: &Path, tick: u64) -> UniverseState {
    let conn = rusqlite::Connection::open(universe_dir.join(nomad_server::DATABASE_FILE))
        .expect("open database");
    let bytes: Vec<u8> = conn
        .query_row(
            "SELECT bytes FROM snapshots WHERE tick = ?1",
            [tick.cast_signed()],
            |row| row.get(0),
        )
        .expect("the snapshot exists");
    nomad_sim::snapshot::decode(&bytes).expect("the snapshot decodes")
}

/// Runs SQL against a Universe's database, to damage it on purpose.
pub fn execute(universe_dir: &Path, sql: &str) {
    let conn = rusqlite::Connection::open(universe_dir.join(nomad_server::DATABASE_FILE))
        .expect("open database");
    conn.execute_batch(sql).expect("sql");
}
