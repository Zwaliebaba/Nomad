//! Snapshot retention and journal compaction over simulated days (N2 plan §7
//! "Retention"; doc 11 §8; the 48 h journal, D-110).
//!
//! In scaled time: an hour lasts [`HOUR`] ticks, 1/300 of a real one, and snapshots and
//! state-hash checkpoints come at the same share of it as in production (5 min and 60 s).
//! Retention then sees the same sequence of snapshots and checkpoints as over three real
//! days, in 1/300 of the ticks (8,647 instead of 2.6 million), so the cost of the game rules
//! hardly matters here. The unit tests in `retention.rs` check the production policy
//! itself, and that scaling it keeps the same snapshots.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nomad_server::{
    CHECKPOINT_INTERVAL, FakeClock, Input, RetentionPolicy, SNAPSHOT_INTERVAL, ServerConfig,
    ServerHandle, status,
};
use nomad_sim::{
    CommanderId, CommanderInput, CommanderInputKind, OperatorInput, TICK_MILLIS, Tick, Ticks,
};

use crate::common::{
    BUILD, T0, TIMEOUT, TestDir, config, create_universe, journal_rows, journaled_commander_inputs,
    journaled_operator_inputs, play,
};

/// How much shorter than real time the test's time is.
const SCALE: u64 = 300;
/// A scaled hour, in ticks.
const HOUR: u64 = Ticks::from_hours(1).get() / SCALE;
/// A scaled day, in ticks.
const DAY: u64 = 24 * HOUR;
/// Ticks between snapshots: 5 scaled minutes.
const SNAPSHOTS: u64 = SNAPSHOT_INTERVAL.get() / SCALE;
/// Ticks between state-hash checkpoints: 60 scaled seconds (D-109).
const CHECKPOINTS: u64 = CHECKPOINT_INTERVAL.get() / SCALE;

// Every production length scales to a whole number of ticks.
const _: () = assert!(
    Ticks::from_hours(1).get().is_multiple_of(SCALE)
        && SNAPSHOT_INTERVAL.get().is_multiple_of(SCALE)
        && CHECKPOINT_INTERVAL.get().is_multiple_of(SCALE)
);

/// Starts the Universe in scaled time and waits until it is live.
fn start_scaled(universe_dir: &Path, clock: &FakeClock) -> ServerHandle {
    let config = ServerConfig {
        snapshot_interval: Ticks::new(SNAPSHOTS),
        checkpoint_interval: Ticks::new(CHECKPOINTS),
        retention: RetentionPolicy::scaled(Ticks::new(HOUR)),
        ..config(universe_dir)
    };
    let server = ServerHandle::start(config, Arc::new(clock.clone())).expect("server starts");
    server.wait_until_live(TIMEOUT).expect("server goes live");
    server
}

#[test]
fn after_three_days_in_scaled_time_only_the_doc_11_snapshots_remain() {
    // The first run: an input applies on tick 6, and the clean stop comes at tick 7, before
    // the first periodic snapshot and between two checkpoints.
    const STOP: u64 = 7;
    let dir = TestDir::new("retention");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 31, &clock);

    let connect = CommanderInput {
        commander: CommanderId(1),
        client_seq: 1,
        kind: CommanderInputKind::Connected,
    };
    let server = start_scaled(&created.dir, &clock);
    play(
        &server,
        &clock,
        &[(5, Input::Commander(connect.clone()))],
        STOP,
    );
    server.shutdown().expect("clean shutdown");
    assert_eq!(journaled_commander_inputs(&created.dir), [(6, connect)]);

    // Three days of downtime: catch-up writes a snapshot every 5 minutes, and retention
    // runs after each one.
    clock.advance(Duration::from_millis(3 * DAY * TICK_MILLIS));
    let server = start_scaled(&created.dir, &clock);
    let end = STOP + 3 * DAY;
    assert_eq!(server.status().tick, Tick::new(end));
    server.shutdown().expect("clean shutdown");

    // Doc 11 §8, worked out by hand for this run. Snapshots exist at 0 (create), 7 (the
    // first stop), every 5 minutes up to 72 h (8,640), and 8,647 (the final stop).
    // - The latest two: 8,647 and 8,640.
    // - Hourly for the 48 h before the latest: 48 h before it is tick 2,887, in the hour
    //   that starts at 2,880 (hour 24), so the first snapshot of hours 24 to 72.
    // - Daily before that: the first snapshot of day 0, tick 0.
    let mut expected: BTreeSet<u64> = (24..=72).map(|h| h * HOUR).collect();
    expected.insert(0);
    expected.insert(end);
    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    let kept: BTreeSet<u64> = report.snapshots.iter().map(|s| s.tick.get()).collect();
    assert_eq!(kept, expected);

    // The journal is kept for 48 h (D-110), from the kept snapshot that opens that window:
    // the hourly snapshot at 2,880, in whose hour the point 48 h before the latest snapshot
    // (2,887) falls. It covers 48 h to 49 h. Everything older is gone: the first run's
    // input of tick 6 and its checkpoints, and the catch-up start of tick 8 with the
    // disconnect of commander 1 that came with it (D-047).
    let floor = 24 * HOUR;
    assert_eq!(report.meta.journal_floor, Tick::new(floor));
    assert!((48 * HOUR..49 * HOUR).contains(&(end - floor)));
    let rows = journal_rows(&created.dir);
    assert!(
        rows.iter()
            .all(|(tick, _, _)| *tick > floor && *tick <= end)
    );
    assert_eq!(
        journaled_operator_inputs(&created.dir),
        [(end, OperatorInput::CatchUpEnd)]
    );
    // What is left besides the end marker: a state-hash checkpoint every minute after the
    // floor (D-109), through 8,646.
    let checkpoints: Vec<u64> = rows
        .iter()
        .filter(|(_, kind, _)| kind == "state_hash")
        .map(|(tick, _, _)| *tick)
        .collect();
    let last_checkpoint = end / CHECKPOINTS * CHECKPOINTS;
    let expected: Vec<u64> = (floor / CHECKPOINTS + 1..=last_checkpoint / CHECKPOINTS)
        .map(|i| i * CHECKPOINTS)
        .collect();
    assert_eq!(checkpoints, expected);
    assert_eq!(rows.len(), checkpoints.len() + 1);
    assert_eq!(report.journal.entries, rows.len() as u64);
    assert_eq!(report.journal.checkpoints, checkpoints.len() as u64);
    assert_eq!(
        report.journal.last_checkpoint,
        Some(Tick::new(last_checkpoint))
    );
}
