//! Catch-up after downtime (N2 plan §4.5, §7 "Catch-up"; doc 11 §7; D-047).

use std::time::Duration;

use nomad_server::{FakeClock, Phase, ServerEvent};
use nomad_sim::{OperatorInput, Tick, Ticks};

use crate::common::{
    T0, TIMEOUT, TestDir, create_universe, journaled_operator_inputs, play, reference_hash,
    run_live_until, script, start, start_live, start_without_compaction,
};

const SIX_HOURS: u64 = Ticks::from_hours(6).get();

#[test]
fn a_season_that_started_six_hours_ago_catches_up_exactly() {
    const SEED: u64 = 21;
    let dir = TestDir::new("catch-up-start");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    clock.advance(Duration::from_hours(6));

    let server = start(&created.dir, &clock);
    let events = server.subscribe();
    let status = server.wait_until_live(TIMEOUT).expect("live");
    assert_eq!(status.tick, Tick::new(SIX_HOURS));
    let progress = status.catch_up.expect("catch-up progress");
    assert_eq!(progress.from, Tick::ZERO);
    assert_eq!(progress.target, Tick::new(SIX_HOURS));
    assert_eq!(progress.remaining(), Ticks::new(0));
    let (tick, hash) = server.state_hash().expect("hash");
    assert_eq!(tick, Tick::new(SIX_HOURS));
    assert_eq!(hash, reference_hash(SEED, T0, &[], SIX_HOURS));

    // Subscribers saw progress, then live play.
    let seen: Vec<ServerEvent> = events.try_iter().collect();
    assert!(seen.iter().any(|e| matches!(e, ServerEvent::CatchUp(_))));
    assert!(seen.iter().any(|e| *e
        == ServerEvent::Live {
            tick: Tick::new(SIX_HOURS)
        }));
    // Live from here: the next tick comes 100 ms later.
    run_live_until(&server, &clock, SIX_HOURS + 1);
    server.shutdown().expect("clean shutdown");

    // The final snapshot is one tick after the snapshot of the target tick, so retention
    // compacted the whole journal, markers included (`catch_up_is_marked_in_the_journal`
    // reads them).
    assert!(journaled_operator_inputs(&created.dir).is_empty());
}

#[test]
fn catch_up_is_marked_in_the_journal() {
    let dir = TestDir::new("catch-up-markers");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 26, &clock);
    clock.advance(Duration::from_hours(6));
    let server = start_without_compaction(&created.dir, &clock);
    server.wait_until_live(TIMEOUT).expect("live");
    server.shutdown().expect("clean shutdown");
    // The first catch-up tick starts the mode; the target tick ends it and runs live.
    assert_eq!(
        journaled_operator_inputs(&created.dir),
        [
            (1, OperatorInput::CatchUpStart),
            (SIX_HOURS, OperatorInput::CatchUpEnd)
        ]
    );
}

#[test]
fn a_universe_stopped_six_hours_ago_catches_up_exactly() {
    const SEED: u64 = 22;
    let dir = TestDir::new("catch-up-stopped");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);

    // Ten minutes of live play with inputs, then a clean stop.
    let server = start_live(&created.dir, &clock);
    play(&server, &clock, &script(), 6_000);
    server.shutdown().expect("clean shutdown");

    clock.advance(Duration::from_hours(6));
    let server = start_live(&created.dir, &clock);
    let target = 6_000 + SIX_HOURS;
    assert_eq!(server.status().tick, Tick::new(target));
    let (_, hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");
    assert_eq!(hash, reference_hash(SEED, T0, &script(), target));
}

#[test]
fn an_interrupted_catch_up_resumes_and_ends_at_the_same_state() {
    const SEED: u64 = 23;
    let dir = TestDir::new("catch-up-interrupted");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    clock.advance(Duration::from_hours(6));

    // Shut down at once: catch-up stops at its next inbox check, almost certainly long
    // before the target, and the final snapshot keeps the mode.
    let server = start_without_compaction(&created.dir, &clock);
    let stopped_at = server.shutdown().expect("clean shutdown").tick.get();
    let interrupted = stopped_at < SIX_HOURS;
    if interrupted {
        assert_eq!(
            journaled_operator_inputs(&created.dir),
            [(1, OperatorInput::CatchUpStart)]
        );
    }

    let server = start_without_compaction(&created.dir, &clock);
    let status = server.wait_until_live(TIMEOUT).expect("live");
    assert_eq!(status.tick, Tick::new(SIX_HOURS));
    let (_, hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");
    assert_eq!(hash, reference_hash(SEED, T0, &[], SIX_HOURS));

    // The resumed catch-up did not start the mode again; it only ended it. (Retention kept
    // the snapshot of the first stop and the journal after it.)
    if interrupted {
        assert_eq!(
            journaled_operator_inputs(&created.dir),
            [(SIX_HOURS, OperatorInput::CatchUpEnd)],
            "first run stopped at tick {stopped_at}"
        );
    }
}

#[test]
fn a_wall_clock_jump_while_live_is_caught_up() {
    // A computer that slept for an hour: the wall clock moved, the monotonic clock did
    // not (doc 11 §2: Universe time is UTC).
    const SEED: u64 = 24;
    let dir = TestDir::new("catch-up-jump");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    let server = start_without_compaction(&created.dir, &clock);
    server.wait_until_live(TIMEOUT).expect("live");
    run_live_until(&server, &clock, 100);

    clock.jump_utc(3_600_000);
    clock.advance(Duration::from_secs(1));
    let target = 100 + 10 + Ticks::from_hours(1).get();
    let status = server
        .wait_for(TIMEOUT, |s| {
            s.phase == Phase::Live && s.tick >= Tick::new(target)
        })
        .expect("caught up");
    assert_eq!(status.tick, Tick::new(target));
    let (_, hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");
    assert_eq!(hash, reference_hash(SEED, T0, &[], target));
    let markers: Vec<u64> = journaled_operator_inputs(&created.dir)
        .into_iter()
        .map(|(tick, _)| tick)
        .collect();
    assert_eq!(markers, [110, target]);
}

#[test]
fn a_wall_clock_set_back_does_not_stop_the_universe() {
    let dir = TestDir::new("catch-up-back");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 25, &clock);
    let server = start_live(&created.dir, &clock);
    run_live_until(&server, &clock, 100);
    clock.jump_utc(-3_600_000);
    // Pacing follows the monotonic clock, so live play goes on at 10 Hz.
    run_live_until(&server, &clock, 150);
    server.shutdown().expect("clean shutdown");

    // A restart while the wall clock is still behind the Universe does not wait for it.
    let server = start_live(&created.dir, &clock);
    run_live_until(&server, &clock, 160);
    server.shutdown().expect("clean shutdown");
    assert!(journaled_operator_inputs(&created.dir).is_empty());
}
