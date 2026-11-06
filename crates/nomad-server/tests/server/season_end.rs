//! The season end (D-116): the season's final tick runs normally, then the Universe stops
//! advancing and refuses inputs, also across restarts. A short season stands in for the
//! 8–12 weeks of a real one.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nomad_server::{
    FakeClock, Input, Phase, ServerConfig, ServerEvent, ServerHandle, SubmitError, status,
};
use nomad_sim::{CommanderId, CommanderInput, CommanderInputKind, OperatorInput, Tick};

use crate::common::{
    BUILD, T0, TIMEOUT, TestDir, config, create_universe, journal_rows, journaled_operator_inputs,
    reference_hash, run_live_until,
};

fn connect(seq: u64) -> Input {
    Input::Commander(CommanderInput {
        commander: CommanderId(1),
        client_seq: seq,
        kind: CommanderInputKind::Connected,
    })
}

fn start_short_season(universe_dir: &Path, clock: &FakeClock, end: u64) -> ServerHandle {
    let config = ServerConfig {
        season_end: Some(Tick::new(end)),
        ..config(universe_dir)
    };
    ServerHandle::start(config, Arc::new(clock.clone())).expect("server starts")
}

fn wait_for_season_over(server: &ServerHandle) -> Tick {
    let status = server
        .wait_for(TIMEOUT, |s| s.phase == Phase::SeasonOver)
        .expect("the season ends");
    assert_eq!(status.phase, Phase::SeasonOver);
    status.tick
}

#[test]
fn the_final_tick_runs_normally_and_nothing_follows_it() {
    const SEED: u64 = 52;
    const END: u64 = 600;
    let dir = TestDir::new("season-end-live");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    let server = start_short_season(&created.dir, &clock, END);
    server.wait_until_live(TIMEOUT).expect("live");
    let events = server.subscribe();

    // An input submitted after tick 599 applies on the final tick.
    run_live_until(&server, &clock, END - 1);
    server.submit(connect(1)).expect("accepted before the end");
    run_live_until(&server, &clock, END);
    assert_eq!(wait_for_season_over(&server), Tick::new(END));
    assert!(events.try_iter().any(|e| e
        == ServerEvent::SeasonOver {
            tick: Tick::new(END)
        }));

    // Time passes, but the Universe stays at its final tick, and inputs are refused.
    clock.advance(Duration::from_mins(10));
    assert_eq!(server.submit(connect(2)), Err(SubmitError::SeasonOver));
    let (tick, hash) = server.state_hash().expect("hash");
    assert_eq!(tick, Tick::new(END));
    assert_eq!(server.status().tick, Tick::new(END));
    assert_eq!(
        server.shutdown().expect("clean shutdown").tick,
        Tick::new(END)
    );
    assert_eq!(
        hash,
        reference_hash(SEED, T0, &[(END - 1, connect(1))], END)
    );

    // The input is journaled on the final tick, followed by its checkpoint; nothing later.
    let rows: Vec<(u64, String)> = journal_rows(&created.dir)
        .into_iter()
        .map(|(tick, kind, _)| (tick, kind))
        .collect();
    assert_eq!(
        rows,
        [
            (END, "commander".to_owned()),
            (END, "state_hash".to_owned())
        ]
    );
}

#[test]
fn catch_up_stops_at_the_season_end_and_a_restart_does_not_advance() {
    const SEED: u64 = 51;
    const END: u64 = 1_000;
    let dir = TestDir::new("season-end-catch-up");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, SEED, &clock);
    // The wall clock is an hour on, far past the season end.
    clock.advance(Duration::from_hours(1));

    let server = start_short_season(&created.dir, &clock, END);
    assert_eq!(wait_for_season_over(&server), Tick::new(END));
    assert_eq!(server.submit(connect(1)), Err(SubmitError::SeasonOver));
    clock.advance(Duration::from_hours(1));
    // A round trip through the idle simulation thread: still at the final tick.
    let (tick, hash) = server.state_hash().expect("hash");
    assert_eq!(tick, Tick::new(END));
    assert_eq!(hash, reference_hash(SEED, T0, &[], END));
    server.flush().expect("flush");
    // The final state is saved as a snapshot.
    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(
        report.snapshots.last().map(|s| s.tick),
        Some(Tick::new(END))
    );
    assert_eq!(
        server.shutdown().expect("clean shutdown").tick,
        Tick::new(END)
    );

    // A restart, with the wall clock further on still, loads the final state and stays.
    clock.advance(Duration::from_hours(5));
    let server = start_short_season(&created.dir, &clock, END);
    assert_eq!(server.start_report().tick, Tick::new(END));
    assert_eq!(wait_for_season_over(&server), Tick::new(END));
    let (tick, _) = server.state_hash().expect("hash");
    assert_eq!(tick, Tick::new(END));
    server.shutdown().expect("clean shutdown");

    // Catch-up ran to the final tick and no further; the restart journaled nothing.
    assert_eq!(
        journaled_operator_inputs(&created.dir),
        [
            (1, OperatorInput::CatchUpStart),
            (END, OperatorInput::CatchUpEnd)
        ]
    );
    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(report.journal.last_tick, Some(Tick::new(END)));
}
