//! Crash recovery (N2 plan §7 "Crash recovery"; doc 11 §8; D-048, D-049).

use nomad_server::{FakeClock, Input, status};
use nomad_sim::{CommanderId, CommanderInput, CommanderInputKind, Tick};

use crate::common::{
    BUILD, T0, TestDir, create_universe, journal_rows, play, reference_hash, run_live_until,
    script, start, start_live,
};

#[test]
fn crash_and_restart_end_at_the_state_of_an_uninterrupted_run() {
    const SEED: u64 = 11;
    let dir = TestDir::new("crash");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);

    // Past the periodic snapshot at tick 3,000, with inputs before and after it.
    let server = start_live(&created.dir, &clock);
    play(&server, &clock, &script(), 4_500);
    server.flush().expect("flush");
    drop(server); // A crash: no final snapshot.

    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert!(!report.running);
    assert_eq!(
        report.snapshots.last().map(|s| s.tick),
        Some(Tick::new(3_000))
    );

    // Restart: snapshot 3,000, replay of the journal after it, then 299 ticks of catch-up
    // to the wall clock.
    let server = start(&created.dir, &clock);
    assert_eq!(server.start_report().snapshot_tick, Tick::new(3_000));
    assert!(server.start_report().replayed_entries >= 2);
    let status = server
        .wait_until_live(crate::common::TIMEOUT)
        .expect("live again");
    assert_eq!(status.tick, Tick::new(4_500));
    play(&server, &clock, &script(), 6_000);
    let (tick, recovered_hash) = server.state_hash().expect("hash");
    assert_eq!(tick, Tick::new(6_000));
    server.shutdown().expect("clean shutdown");

    // The same script without a crash, through the server and through nomad-sim alone.
    let reference_dir = TestDir::new("crash-reference");
    let reference_clock = FakeClock::new(T0);
    let reference = create_universe(&reference_dir, SEED, &reference_clock);
    let server = start_live(&reference.dir, &reference_clock);
    play(&server, &reference_clock, &script(), 6_000);
    let (_, uninterrupted_hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");

    assert_eq!(recovered_hash, uninterrupted_hash);
    assert_eq!(
        recovered_hash,
        reference_hash(SEED, T0, &script(), 6_000),
        "the server applies each input on the tick after the one it was submitted in"
    );
}

#[test]
fn a_crash_loses_at_most_the_last_second_of_inputs() {
    let dir = TestDir::new("crash-loss");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 3, &clock);
    let connect = |commander, seq| {
        Input::Commander(CommanderInput {
            commander: CommanderId(commander),
            client_seq: seq,
            kind: CommanderInputKind::Connected,
        })
    };

    let server = start_live(&created.dir, &clock);
    run_live_until(&server, &clock, 100);
    server.submit(connect(1, 1)).expect("accepted"); // applies on tick 101
    run_live_until(&server, &clock, 115); // its batch spans 10 ticks by 110: committed
    server.submit(connect(2, 1)).expect("accepted"); // applies on tick 116
    run_live_until(&server, &clock, 120); // its batch is younger than 1 s: not committed
    drop(server);

    let journal: Vec<u64> = journal_rows(&created.dir)
        .iter()
        .map(|(tick, _, _)| *tick)
        .collect();
    assert_eq!(journal, [101]);
    let server = start_live(&created.dir, &clock);
    assert!(server.wait_for_tick(Tick::new(120), crate::common::TIMEOUT));
    let (_, hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");
    assert_eq!(hash, reference_hash(3, T0, &[(100, connect(1, 1))], 120));
}
