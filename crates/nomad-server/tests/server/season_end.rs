//! The season end (D-116, D-192, D-195): the simulation fixes the result at the season's
//! final tick or at an early win, that tick runs normally, then the Universe stops advancing
//! and refuses inputs, also across restarts. A short season stands in for the 8–12 weeks of
//! a real one. The season read model follows the state, also after a crash (D-196).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nomad_server::{
    CreateParams, Created, FakeClock, Input, Phase, ServerConfig, ServerEvent, ServerHandle,
    SubmitError, stage1_roster, status,
};
use nomad_sim::galaxy::{SystemKind, generate_stage1_roster, is_claimable};
use nomad_sim::snapshot::{encode, state_hash};
use nomad_sim::{
    CommanderId, CommanderInput, CommanderInputKind, OperatorInput, Rules, SeasonConfig,
    SeasonEndReason, SeasonResult, SiteId, SystemId, Tick, TickInputs, Ticks, UniverseState,
    scenario, step,
};

use crate::common::{
    BUILD, T0, TIMEOUT, TestDir, config, create_short_season, create_universe, execute,
    journal_rows, journaled_operator_inputs, reference_hash_short, run_live_until,
};

fn connect(seq: u64) -> Input {
    Input::Commander(CommanderInput {
        commander: CommanderId(1),
        client_seq: seq,
        kind: CommanderInputKind::Connected,
    })
}

fn start(universe_dir: &Path, clock: &FakeClock) -> ServerHandle {
    ServerHandle::start(config(universe_dir), Arc::new(clock.clone())).expect("server starts")
}

fn wait_for_season_over(server: &ServerHandle) -> Tick {
    let status = server
        .wait_for(TIMEOUT, |s| s.phase == Phase::SeasonOver)
        .expect("the season ends");
    assert_eq!(status.phase, Phase::SeasonOver);
    status.tick
}

const EVERYONE: [CommanderId; 4] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];

#[test]
fn the_final_tick_runs_normally_and_nothing_follows_it() {
    const SEED: u64 = 52;
    const END: u64 = 600;
    let dir = TestDir::new("season-end-live");
    let clock = FakeClock::new(T0);
    let created = create_short_season(&dir, SEED, &clock, END);
    assert_eq!(created.meta.final_tick, Tick::new(END));
    let server = start(&created.dir, &clock);
    assert_eq!(server.start_report().season_end, Tick::new(END));
    server.wait_until_live(TIMEOUT).expect("live");
    let events = server.subscribe();

    // An input submitted after tick 599 applies on the final tick.
    run_live_until(&server, &clock, END - 1);
    server.submit(connect(1)).expect("accepted before the end");
    run_live_until(&server, &clock, END);
    assert_eq!(wait_for_season_over(&server), Tick::new(END));
    // Nobody claimed anything: all four tie on everything and win jointly (D-194).
    let result = SeasonResult {
        tick: Tick::new(END),
        reason: SeasonEndReason::FinalTick,
        winners: EVERYONE.to_vec(),
    };
    assert!(events.try_iter().any(|e| e
        == ServerEvent::SeasonOver {
            tick: Tick::new(END),
            result: result.clone(),
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
        reference_hash_short(SEED, T0, END, &[(END - 1, connect(1))], END)
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
    // The read model holds the result and the tick-0 standings (D-196).
    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(report.result, Some(result));
    assert_eq!(
        report.standings.map(|(tick, s)| (tick, s.len())),
        Some((Tick::ZERO, 4))
    );
}

#[test]
fn catch_up_stops_at_the_season_end_and_a_restart_does_not_advance() {
    const SEED: u64 = 51;
    const END: u64 = 1_000;
    let dir = TestDir::new("season-end-catch-up");
    let clock = FakeClock::new(T0);
    let created = create_short_season(&dir, SEED, &clock, END);
    // The wall clock is an hour on, far past the season end.
    clock.advance(Duration::from_hours(1));

    let server = start(&created.dir, &clock);
    assert_eq!(wait_for_season_over(&server), Tick::new(END));
    assert_eq!(server.submit(connect(1)), Err(SubmitError::SeasonOver));
    clock.advance(Duration::from_hours(1));
    // A round trip through the idle simulation thread: still at the final tick.
    let (tick, hash) = server.state_hash().expect("hash");
    assert_eq!(tick, Tick::new(END));
    assert_eq!(hash, reference_hash_short(SEED, T0, END, &[], END));
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
    let server = start(&created.dir, &clock);
    assert_eq!(server.start_report().tick, Tick::new(END));
    assert!(server.start_report().result.is_some());
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

// ---------------------------------------------------------------- an early win

/// How long an outpost deploys in the scripted seasons: seconds instead of 2 h, so the
/// tests stay fast (D-190).
const DEPLOY_S: u64 = 10;

/// The free sites of `system`.
fn free_sites(state: &UniverseState, system: SystemId) -> Vec<SiteId> {
    state
        .galaxy()
        .system(system)
        .map(|s| s.sites.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|&site| state.holding_at(site).is_none())
        .collect()
}

/// Claimable systems in which `commander` can take a majority: its own home and every
/// system without another commander's mother station.
fn open_systems(state: &UniverseState, commander: CommanderId) -> Vec<SystemId> {
    state
        .galaxy()
        .systems()
        .filter(|(_, s)| is_claimable(s.kind))
        .filter(|(_, s)| {
            s.kind != SystemKind::Home
                || s.sites.iter().all(|&site| {
                    state
                        .holding_at(site)
                        .is_none_or(|(_, h)| h.owner == commander)
                })
        })
        .map(|(id, _)| id)
        .collect()
}

/// Commander 2 holds majorities in five systems from the start, and one holding short of a
/// majority in a sixth, where its outpost completes after [`DEPLOY_S`]: an early win then
/// (D-192). Without `win`, the sixth system gets only the outpost.
fn script_claims(state: &mut UniverseState, win: bool) {
    let c2 = CommanderId(2);
    let systems = open_systems(state, c2);
    for (index, &system) in systems.iter().take(6).enumerate() {
        let sites = state.galaxy().system(system).map_or(0, |s| s.sites.len());
        let majority = sites / 2 + 1;
        let owned = state
            .holdings()
            .filter(|(_, h)| h.owner == c2)
            .filter(|(_, h)| state.galaxy().site(h.site).map(|s| s.system) == Some(system))
            .count();
        let free = free_sites(state, system);
        if index < 5 {
            for &site in free.iter().take(majority - owned) {
                scenario::place_holding(state, c2, site).expect("a free site");
            }
        } else {
            let completed = if win { majority - owned - 1 } else { 0 };
            for &site in free.iter().take(completed) {
                scenario::place_holding(state, c2, site).expect("a free site");
            }
            scenario::place_outpost(state, c2, free[completed]).expect("a free site");
        }
    }
}

/// The tick and state hash of `state`'s early win in a pure simulation run whose first tick
/// carries `first`, or `None` without one by its scheduled end.
fn early_win(mut state: UniverseState, first: &TickInputs) -> Option<(Tick, u64)> {
    step(&mut state, first);
    while !state.season_over() {
        step(&mut state, &TickInputs::default());
    }
    state
        .season_result()
        .filter(|r| r.reason == SeasonEndReason::EarlyWin)
        .map(|r| (r.tick, state_hash(&state).expect("hash")))
}

/// A short-deployment Universe created like `create`, then given the scripted claims, and
/// written back as its tick-0 snapshot. Returns it with its tick-0 state.
fn create_scripted(
    dir: &TestDir,
    seed: u64,
    clock: &FakeClock,
    end: u64,
    win: bool,
) -> (Created, UniverseState) {
    let created = create_short_season(dir, seed, clock, end);
    let mut rules = Rules::embedded();
    rules.economy.core.deploy_s = DEPLOY_S;
    let mut state = generate_stage1_roster(
        SeasonConfig {
            final_tick: Tick::new(end),
            ..SeasonConfig::new(seed, created.meta.season_start_unix_ms)
        },
        stage1_roster(CreateParams::default().window_start),
        rules,
    )
    .expect("generate");
    script_claims(&mut state, win);
    let bytes = encode(&state).expect("encode");
    let hash = state_hash(&state).expect("hash");
    let conn = rusqlite::Connection::open(created.dir.join(nomad_server::DATABASE_FILE))
        .expect("open database");
    conn.execute(
        "UPDATE snapshots SET bytes = ?1, state_hash = ?2 WHERE tick = 0",
        rusqlite::params![bytes, hash.cast_signed()],
    )
    .expect("replace the tick-0 snapshot");
    (created, state)
}

#[test]
fn an_early_win_stops_the_live_server_on_that_tick() {
    let dir = TestDir::new("season-early-win-live");
    let clock = FakeClock::new(T0);
    let (created, state) = create_scripted(&dir, 61, &clock, 100_000, true);
    let (win, hash) = early_win(state, &TickInputs::default()).expect("the script wins early");
    let server = start(&created.dir, &clock);
    server.wait_until_live(TIMEOUT).expect("live");
    let events = server.subscribe();
    // Run live to a little before the win, then let the clock run well past it.
    run_live_until(&server, &clock, win.get() - 5);
    clock.advance(Duration::from_mins(1));
    assert_eq!(wait_for_season_over(&server), win);
    let result = SeasonResult {
        tick: win,
        reason: SeasonEndReason::EarlyWin,
        winners: vec![CommanderId(2)],
    };
    assert!(events.try_iter().any(|e| e
        == ServerEvent::SeasonOver {
            tick: win,
            result: result.clone(),
        }));
    assert_eq!(server.submit(connect(1)), Err(SubmitError::SeasonOver));
    assert_eq!(server.state_hash().expect("hash"), (win, hash));
    server.flush().expect("flush");
    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(report.snapshots.last().map(|s| s.tick), Some(win));
    assert_eq!(report.season_end(), win);
    assert_eq!(report.result.as_ref(), Some(&result));
    let (changed, standings) = report.standings.clone().expect("standings");
    assert_eq!(changed, win);
    assert_eq!(
        (
            standings[0].commander,
            standings[0].rank,
            standings[0].systems
        ),
        (CommanderId(2), 1, 6)
    );
    // Tick 0, the scripted claims at tick 1, and the win.
    assert_eq!(report.standings_changes, 3);
    let text = report.to_string();
    assert!(text.contains("Result       early win (D-192)"), "{text}");
    assert!(text.contains(": commander 2\n"), "{text}");
    assert_eq!(server.shutdown().expect("clean shutdown").tick, win);
}

#[test]
fn an_early_win_during_catch_up_ends_it_on_that_tick() {
    let dir = TestDir::new("season-early-win-catch-up");
    let clock = FakeClock::new(T0);
    let (created, state) = create_scripted(&dir, 62, &clock, 100_000, true);
    // The reference run is in catch-up mode from its first tick, as the server's is.
    let catch_up = TickInputs {
        operator: vec![OperatorInput::CatchUpStart],
        commanders: vec![],
    };
    let (win, hash) = early_win(state, &catch_up).expect("the script wins early");
    // The server was down while the outpost completed.
    clock.advance(Duration::from_hours(2));
    let server = start(&created.dir, &clock);
    assert_eq!(wait_for_season_over(&server), win);
    assert_eq!(server.state_hash().expect("hash"), (win, hash));
    assert_eq!(server.submit(connect(1)), Err(SubmitError::SeasonOver));
    assert_eq!(server.shutdown().expect("clean shutdown").tick, win);
    // Catch-up started and never ran past the win: no tick remains to end it.
    assert_eq!(
        journaled_operator_inputs(&created.dir),
        [(1, OperatorInput::CatchUpStart)]
    );
    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(report.snapshots.last().map(|s| s.tick), Some(win));
    assert_eq!(
        report.result.map(|r| (r.tick, r.reason)),
        Some((win, SeasonEndReason::EarlyWin))
    );
    // A restart stays at the final state.
    clock.advance(Duration::from_hours(3));
    let server = start(&created.dir, &clock);
    assert!(server.start_report().read_model.unchanged());
    assert_eq!(wait_for_season_over(&server), win);
    server.shutdown().expect("clean shutdown");
}

#[test]
fn the_read_model_follows_the_state_after_a_crash_and_recovery() {
    let dir = TestDir::new("season-read-model");
    let clock = FakeClock::new(T0);
    let (created, _) = create_scripted(&dir, 63, &clock, 100_000, false);
    // A snapshot every 20 s, so a crash at tick 300 recovers at tick 200, after the
    // standings changed.
    let start_often = |clock: &FakeClock| {
        ServerHandle::start(
            ServerConfig {
                snapshot_interval: Ticks::new(200),
                ..config(&created.dir)
            },
            Arc::new(clock.clone()),
        )
        .expect("server starts")
    };
    let server = start_often(&clock);
    server.wait_until_live(TIMEOUT).expect("live");
    // The scripted claims count at tick 1 and the outpost completes after 10 s.
    run_live_until(&server, &clock, 300);
    server.flush().expect("flush");
    let before = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(before.standings_changes, 3, "tick 0, tick 1, the outpost");
    let (changed, standings) = before.standings.clone().expect("standings");
    assert!(changed > Tick::new(1) && changed < Tick::new(200));
    assert_eq!(standings[0].commander, CommanderId(2));
    // A crash: no final snapshot.
    drop(server);

    // The projection is damaged meanwhile: the history lost its newest change and gained a
    // row from a tick the recovered Universe never reached.
    execute(
        &created.dir,
        &format!(
            "DELETE FROM standings_history WHERE tick = {};
             INSERT INTO standings_history VALUES (900000, 3, 1, 9, 9, 9, 900000);
             INSERT INTO season_result VALUES (3, 900000, 'early_win');",
            changed.get()
        ),
    );
    let server = start_often(&clock);
    assert_eq!(server.start_report().tick, Tick::new(200));
    let sync = server.start_report().read_model;
    assert_eq!(sync.dropped_rows, 1);
    assert!(sync.appended && sync.result_written);
    // Straight after recovery the projection matches the recovered state.
    let recovered = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(recovered.standings, before.standings);
    assert_eq!(recovered.result, None);
    server.wait_until_live(TIMEOUT).expect("live");
    run_live_until(&server, &clock, 400);
    server.shutdown().expect("clean shutdown");
    let after = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(after.standings, before.standings);
    assert_eq!(after.standings_changes, 3);
    // A clean restart finds it in sync.
    let server = start_often(&clock);
    assert!(server.start_report().read_model.unchanged());
    server.shutdown().expect("clean shutdown");

    // A crash before anything after tick 0 was durable: the rows the run wrote belong to
    // ticks the recovery undid, so they go, and the next run writes them again.
    let dir = TestDir::new("season-read-model-early-crash");
    let (created, _) = create_scripted(&dir, 63, &clock, 100_000, false);
    let server = start(&created.dir, &clock);
    server.wait_until_live(TIMEOUT).expect("live");
    run_live_until(&server, &clock, 300);
    server.flush().expect("flush");
    drop(server);
    let server = start(&created.dir, &clock);
    assert_eq!(server.start_report().tick, Tick::ZERO);
    let sync = server.start_report().read_model;
    assert_eq!((sync.dropped_rows, sync.appended), (8, false));
    let recovered = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(recovered.standings.map(|(tick, _)| tick), Some(Tick::ZERO));
    server.wait_until_live(TIMEOUT).expect("live");
    server.shutdown().expect("clean shutdown");
    let after = status::inspect(&created.dir, &clock, BUILD).expect("status");
    assert_eq!(after.standings_changes, 3);
}

#[test]
fn a_new_universe_has_its_tick_0_standings_in_the_read_model() {
    let dir = TestDir::new("season-read-model-create");
    let clock = FakeClock::new(T0);
    let created = create_universe(&dir, 64, &clock);
    let report = status::inspect(&created.dir, &clock, BUILD).expect("status");
    let (tick, standings) = report.standings.clone().expect("standings");
    assert_eq!(tick, Tick::ZERO);
    assert_eq!(
        standings
            .iter()
            .map(|s| (s.commander, s.rank))
            .collect::<Vec<_>>(),
        EVERYONE.map(|c| (c, 1))
    );
    assert!(report.result.is_none());
    assert!(
        report
            .to_string()
            .contains("   1. commander 1   0 systems, 1 holding, deposit value 1"),
        "{report}"
    );
    // An 8-week season by default (D-106).
    assert_eq!(report.season_end(), nomad_server::season_end_tick(8));
    let server = ServerHandle::start(
        ServerConfig {
            ..config(&created.dir)
        },
        Arc::new(clock.clone()),
    )
    .expect("server starts");
    assert!(server.start_report().read_model.unchanged());
    server.shutdown().expect("clean shutdown");
}
