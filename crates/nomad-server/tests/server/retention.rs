//! Snapshot retention and journal compaction over simulated days (N2 plan §7
//! "Retention"; doc 11 §8).

use std::collections::BTreeSet;
use std::time::Duration;

use nomad_server::{FakeClock, Input, status};
use nomad_sim::{CommanderId, CommanderInput, CommanderInputKind, OperatorInput, Tick, Ticks};

use crate::common::{
    BUILD, T0, TestDir, create_universe, journal_rows, journaled_operator_inputs, play, start_live,
};

#[test]
fn after_three_days_only_the_doc_11_snapshots_remain() {
    let dir = TestDir::new("retention");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 31, &clock);

    // An input in the first seconds, then a clean stop at tick 20.
    let connect = Input::Commander(CommanderInput {
        commander: CommanderId(1),
        client_seq: 1,
        kind: CommanderInputKind::Connected,
    });
    let server = start_live(&created.dir, &clock);
    play(&server, &clock, &[(5, connect)], 20);
    server.shutdown().expect("clean shutdown");
    assert_eq!(journal_rows(&created.dir).len(), 1);

    // Three days of downtime: catch-up writes a snapshot every 3,000 ticks, and retention
    // runs after each one.
    clock.advance(Duration::from_hours(72));
    let server = start_live(&created.dir, &clock);
    let end = 20 + Ticks::from_days(3).get();
    assert_eq!(server.status().tick, Tick::new(end));
    server.shutdown().expect("clean shutdown");

    // Doc 11 §8, worked out by hand for this run. Snapshots exist at 0 (create), 20 (the
    // first stop), every multiple of 3,000 up to 2,592,000, and 2,592,020 (the final stop).
    // - The latest two: 2,592,020 and 2,592,000.
    // - Hourly for the 48 h before the latest: 48 h before it is tick 864,020, in the hour
    //   that starts at 864,000 (hour 24), so the first snapshot of hours 24 to 72.
    // - Daily before that: the first snapshot of day 0, tick 0.
    let hour = Ticks::from_hours(1).get();
    let mut expected: BTreeSet<u64> = (24..=72).map(|h| h * hour).collect();
    expected.insert(0);
    expected.insert(end);
    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    let kept: BTreeSet<u64> = report.snapshots.iter().map(|s| s.tick.get()).collect();
    assert_eq!(kept, expected);

    // The journal is compacted to what follows the older of the latest two snapshots: the
    // input of tick 6 and the catch-up start marker are gone, and only the end marker on the
    // final tick is left.
    assert_eq!(report.meta.journal_floor, Tick::new(end - 20));
    assert_eq!(
        journaled_operator_inputs(&created.dir),
        [(end, OperatorInput::CatchUpEnd)]
    );
    assert_eq!(report.journal.entries, 1);
}
