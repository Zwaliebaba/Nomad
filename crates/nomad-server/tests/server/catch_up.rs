//! Catch-up after downtime (N2 plan §4.5, §7 "Catch-up"; doc 11 §7; D-047).

use std::time::Duration;

use nomad_server::{FakeClock, Input, Phase, ServerEvent};
use nomad_sim::{CommanderId, CommanderInput, CommanderInputKind, OperatorInput, Tick, Ticks};

use crate::common::{
    T0, TIMEOUT, TestDir, create_universe, journaled_commander_inputs, journaled_operator_inputs,
    play, reference_hash, run_live_until, script, snapshot_state, start, start_live,
    start_without_compaction,
};

const SIX_HOURS: u64 = Ticks::from_hours(6).get();
const HOUR: u64 = Ticks::from_hours(1).get();

/// Commander 1's client connects.
fn connect() -> CommanderInput {
    CommanderInput {
        commander: CommanderId(1),
        client_seq: 1,
        kind: CommanderInputKind::Connected,
    }
}

/// The `Disconnected` input the server journals for commander 1 when catch-up starts.
fn server_disconnect() -> CommanderInput {
    CommanderInput {
        commander: CommanderId(1),
        client_seq: u64::MAX,
        kind: CommanderInputKind::Disconnected,
    }
}

/// Every commander is offline during catch-up (D-047): in a snapshot taken mid-catch-up,
/// the mode is on and nobody is connected.
fn assert_offline_mid_catch_up(universe_dir: &std::path::Path, tick: u64) {
    let state = snapshot_state(universe_dir, tick);
    assert!(state.catching_up(), "tick {tick} is not mid-catch-up");
    assert!(
        state.connected().is_empty(),
        "connected at tick {tick}: {:?}",
        state.connected()
    );
}

#[test]
fn a_commander_connected_when_the_computer_slept_is_offline_during_catch_up() {
    const SEED: u64 = 27;
    let dir = TestDir::new("catch-up-sleep-offline");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    let server = start_live(&created.dir, &clock);
    play(&server, &clock, &[(50, Input::Commander(connect()))], 100);

    // The computer slept for an hour with commander 1 connected (D-117).
    clock.jump_utc(3_600_000);
    clock.advance(Duration::from_secs(1));
    let target = 100 + 10 + HOUR;
    let status = server
        .wait_for(TIMEOUT, |s| {
            s.phase == Phase::Live && s.tick >= Tick::new(target)
        })
        .expect("caught up");
    assert_eq!(status.tick, Tick::new(target));
    let (_, hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");

    // Catch-up started on tick 110, and commander 1 went offline on that same tick.
    assert_eq!(
        journaled_operator_inputs(&created.dir),
        [
            (110, OperatorInput::CatchUpStart),
            (target, OperatorInput::CatchUpEnd)
        ]
    );
    assert_eq!(
        journaled_commander_inputs(&created.dir),
        [(51, connect()), (110, server_disconnect())]
    );
    // Still offline at the last snapshot before catch-up ended.
    assert_offline_mid_catch_up(&created.dir, HOUR);
    assert_eq!(
        hash,
        reference_hash(
            SEED,
            T0,
            &[
                (50, Input::Commander(connect())),
                (109, Input::Commander(server_disconnect()))
            ],
            target
        )
    );
}

#[test]
fn a_commander_connected_at_a_crash_is_offline_during_the_catch_up_after_it() {
    const SEED: u64 = 28;
    let dir = TestDir::new("catch-up-crash-offline");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    let server = start_live(&created.dir, &clock);
    play(&server, &clock, &[(50, Input::Commander(connect()))], 100);
    server.flush().expect("flush");
    drop(server); // A crash with commander 1 connected.

    // An hour later: replay ends at tick 51, the last journaled tick, where commander 1 is
    // connected; catch-up then starts on tick 52.
    clock.advance(Duration::from_hours(1));
    let server = start_live(&created.dir, &clock);
    assert_eq!(server.start_report().tick, Tick::new(51));
    let target = 100 + HOUR;
    assert_eq!(server.status().tick, Tick::new(target));
    let (_, hash) = server.state_hash().expect("hash");
    server.shutdown().expect("clean shutdown");

    assert_eq!(
        journaled_operator_inputs(&created.dir),
        [
            (52, OperatorInput::CatchUpStart),
            (target, OperatorInput::CatchUpEnd)
        ]
    );
    assert_eq!(
        journaled_commander_inputs(&created.dir),
        [(51, connect()), (52, server_disconnect())]
    );
    assert_offline_mid_catch_up(&created.dir, HOUR);
    assert_eq!(
        hash,
        reference_hash(
            SEED,
            T0,
            &[
                (50, Input::Commander(connect())),
                (51, Input::Commander(server_disconnect()))
            ],
            target
        )
    );
}

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

    // The journal is kept for 48 h (D-110), so both markers are still there.
    assert_eq!(
        journaled_operator_inputs(&created.dir),
        [
            (1, OperatorInput::CatchUpStart),
            (SIX_HOURS, OperatorInput::CatchUpEnd)
        ]
    );
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

    // The script leaves commanders 1 and 3 connected at the stop. Every commander is offline
    // during catch-up (D-047), so the first catch-up tick disconnects both.
    let disconnect = |id| CommanderInput {
        commander: CommanderId(id),
        client_seq: u64::MAX,
        kind: CommanderInputKind::Disconnected,
    };
    let at_start: Vec<CommanderInput> = journaled_commander_inputs(&created.dir)
        .into_iter()
        .filter(|(tick, _)| *tick == 6_001)
        .map(|(_, input)| input)
        .collect();
    assert_eq!(at_start, [disconnect(1), disconnect(3)]);
    let mut reference = script();
    reference.push((6_000, Input::Commander(disconnect(1))));
    reference.push((6_000, Input::Commander(disconnect(3))));
    assert_eq!(hash, reference_hash(SEED, T0, &reference, target));
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

    // The resumed catch-up did not start the mode again; it only ended it. (The journal is
    // kept for 48 h, D-110, so the first run's start marker is still there.)
    if interrupted {
        assert_eq!(
            journaled_operator_inputs(&created.dir),
            [
                (1, OperatorInput::CatchUpStart),
                (SIX_HOURS, OperatorInput::CatchUpEnd)
            ],
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
