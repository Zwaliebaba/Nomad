//! State-hash checkpoints in the journal, checked on replay (N2 plan §7 "Divergence check";
//! doc 11 §5; D-109).

use std::fmt::Write as _;
use std::time::Duration;

use nomad_server::{FakeClock, Input};
use nomad_sim::{CommanderId, CommanderInput, CommanderInputKind, Tick, Ticks};

use crate::common::{
    T0, TIMEOUT, TestDir, create_universe, execute, journal_rows, play, reference_hash,
    run_live_until, start_live, start_without_compaction,
};

fn connect() -> Input {
    Input::Commander(CommanderInput {
        commander: CommanderId(1),
        client_seq: 1,
        kind: CommanderInputKind::Connected,
    })
}

#[test]
fn a_correct_run_replays_every_checkpoint_without_a_mismatch() {
    // An hour of catch-up, then live play with an input. Snapshots are daily, so a crash
    // replays the whole journal from the tick-0 snapshot, and the checkpoints written
    // during catch-up are checked like the live ones.
    const SEED: u64 = 41;
    let hour = Ticks::from_hours(1).get();
    let dir = TestDir::new("checkpoints");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    clock.advance(Duration::from_hours(1));
    let server = start_without_compaction(&created.dir, &clock);
    server.wait_until_live(TIMEOUT).expect("live");
    let script = [(hour + 100, connect())];
    play(&server, &clock, &script, hour + 1_300);
    server.flush().expect("flush");
    drop(server); // A crash: no final snapshot.

    // One checkpoint every 600 ticks through 37,200, each after its tick's inputs.
    let checkpoints: Vec<u64> = journal_rows(&created.dir)
        .iter()
        .filter(|(_, kind, _)| kind == "state_hash")
        .map(|(tick, _, _)| *tick)
        .collect();
    let expected: Vec<u64> = (1..=(hour + 1_300) / 600).map(|i| i * 600).collect();
    assert_eq!(checkpoints, expected);

    let server = start_without_compaction(&created.dir, &clock);
    let report = server.start_report();
    assert_eq!(report.snapshot_tick, Tick::ZERO);
    // The catch-up markers and the input; checkpoints are not inputs.
    assert_eq!(report.replayed_entries, 3);
    assert_eq!(report.replay_check.checked, expected.len());
    assert!(report.replay_check.mismatches.is_empty());
    assert_eq!(report.tick, Tick::new(hour + 1_200));
    let status = server.wait_until_live(TIMEOUT).expect("live again");
    assert_eq!(status.tick, Tick::new(hour + 1_300));
    let (_, hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");
    // Replay ends at the last checkpoint, 37,200; the 100 ticks to the wall clock are a
    // catch-up, which starts by taking commander 1 offline (D-047).
    let offline = Input::Commander(CommanderInput {
        commander: CommanderId(1),
        client_seq: u64::MAX,
        kind: CommanderInputKind::Disconnected,
    });
    let reference = [script[0].clone(), (hour + 1_200, offline)];
    assert_eq!(hash, reference_hash(SEED, T0, &reference, hour + 1_300));
}

#[test]
fn a_wrong_checkpoint_is_reported_and_recovery_completes() {
    const SEED: u64 = 42;
    const WRONG: u64 = 0x0bad_0bad_0bad_0bad;
    let dir = TestDir::new("checkpoint-divergence");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    let server = start_live(&created.dir, &clock);
    run_live_until(&server, &clock, 1_300);
    let (_, expected) = server.state_hash().expect("hash");
    server.flush().expect("flush");
    drop(server); // A crash: replay from the tick-0 snapshot meets checkpoints 600 and 1,200.

    // Plant a wrong hash at the first checkpoint, as a determinism bug would leave it.
    let payload = postcard::to_stdvec(&WRONG).expect("encode");
    let mut hex = String::new();
    for byte in &payload {
        write!(hex, "{byte:02x}").expect("write to a string");
    }
    execute(
        &created.dir,
        &format!("UPDATE journal SET payload = x'{hex}' WHERE kind = 'state_hash' AND tick = 600;"),
    );

    let server = start_live(&created.dir, &clock);
    let check = &server.start_report().replay_check;
    assert_eq!(check.checked, 2);
    assert_eq!(check.mismatches.len(), 1, "{check:?}");
    let mismatch = check.mismatches[0];
    assert_eq!(mismatch.tick, Tick::new(600));
    assert_eq!(mismatch.journaled, WRONG);
    assert_eq!(mismatch.replayed, reference_hash(SEED, T0, &[], 600));

    // Recovery carried on (D-109): the Universe is where it was, in the same state.
    let (tick, hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");
    assert_eq!((tick, hash), (Tick::new(1_300), expected));
}
