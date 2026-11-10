//! AI schedules and the Strategist inside the step (A2 plan §9; D-080, D-157, D-204,
//! D-212 to D-214).

use nomad_types::{BuildItem, CommanderId, CommanderKind, DepositKind, Difficulty, Owner};

use crate::commander::{CommanderSetup, DEFAULT_WINDOW_START, Presence};
use crate::galaxy::generate_stage1_roster;
use crate::input::{OperatorInput, TickInputs};
use crate::rules::Rules;
use crate::snapshot::{decode, encode, state_hash};
use crate::state::{SeasonConfig, UniverseState};
use crate::step::step;
use crate::testkit::idle;
use crate::time::Ticks;

/// 2026-11-09, a Monday, 00:00 UTC.
const MONDAY: i64 = 1_794_182_400_000;

fn scheduled(start_ms: i64) -> UniverseState {
    generate_stage1_roster(
        SeasonConfig::new(5, start_ms),
        [
            CommanderSetup::human(CommanderId(1), DEFAULT_WINDOW_START),
            CommanderSetup::scheduled_ai(CommanderId(2), Difficulty::Normal),
            CommanderSetup::ai(CommanderId(3)),
            CommanderSetup::scheduled_ai(CommanderId(4), Difficulty::Normal),
        ],
        Rules::embedded(),
    )
    .expect("stage 1 generates")
}

fn online(state: &UniverseState, n: u32) -> bool {
    state
        .commander(CommanderId(n))
        .is_some_and(|r| r.presence.is_online())
}

#[test]
fn a_scheduled_ai_goes_online_and_offline_by_its_schedule_and_takes_its_window_from_it() {
    let probe = scheduled(MONDAY);
    let schedule = probe.schedule(CommanderId(2)).expect("scheduled").clone();
    assert!(probe.schedule(CommanderId(3)).is_none());
    assert!(probe.schedule(CommanderId(1)).is_none());
    // The window comes from the schedule (D-157); the unscheduled AI keeps the default.
    let window = probe.exposure_window(CommanderId(2)).unwrap();
    assert_eq!(window.start.index(), schedule.window_start(240));
    assert_eq!(
        probe.exposure_window(CommanderId(3)).unwrap().start,
        DEFAULT_WINDOW_START
    );
    // Start the season 3 s before the first weekday session, which then lasts its length.
    let first = schedule.weekday[0];
    let start = MONDAY + i64::from(first.start_min) * 60_000 - 3_000;
    let mut state = scheduled(start);
    assert_eq!(state.schedule(CommanderId(2)), Some(&schedule));
    idle(&mut state, 20);
    assert!(!online(&state, 2));
    idle(&mut state, 20);
    assert!(online(&state, 2));
    assert!(
        !online(&state, 3),
        "an unscheduled AI waits for SetPresence"
    );
    let left = Ticks::from_minutes(u64::from(first.length_min)).get() - 10;
    idle(&mut state, left - 5);
    assert!(online(&state, 2));
    idle(&mut state, 10);
    assert!(!online(&state, 2));
    assert!(matches!(
        state.commander(CommanderId(2)).unwrap().presence,
        Presence::Offline { .. }
    ));
}

#[test]
fn set_presence_overrides_the_schedule_for_the_rest_of_the_season() {
    let probe = scheduled(MONDAY);
    let first = probe.schedule(CommanderId(4)).unwrap().weekday[0];
    let start = MONDAY + i64::from(first.start_min) * 60_000 - 3_000;
    let mut state = scheduled(start);
    // Pinned offline before the session starts: it stays offline through it.
    let pin = TickInputs {
        operator: vec![OperatorInput::SetPresence {
            commander: CommanderId(4),
            online: false,
        }],
        commanders: vec![],
    };
    let out = step(&mut state, &pin);
    assert_eq!(out.inputs_rejected, 0, "pinning a scheduled AI changes it");
    idle(&mut state, 100);
    assert!(!online(&state, 4));
    assert_eq!(
        state.commander(CommanderId(4)).unwrap().kind,
        CommanderKind::Ai
    );
}

/// Commander 2 pinned online, the others offline.
fn playing(seed: u64) -> UniverseState {
    let mut state = generate_stage1_roster(
        SeasonConfig::new(seed, 0),
        [
            CommanderSetup::human(CommanderId(1), DEFAULT_WINDOW_START),
            CommanderSetup::ai(CommanderId(2)),
            CommanderSetup::ai(CommanderId(3)),
            CommanderSetup::ai(CommanderId(4)),
        ],
        Rules::embedded(),
    )
    .expect("stage 1 generates");
    step(
        &mut state,
        &TickInputs {
            operator: vec![OperatorInput::SetPresence {
                commander: CommanderId(2),
                online: true,
            }],
            commanders: vec![],
        },
    );
    state
}

#[test]
fn an_online_strategist_queues_a_kit_through_the_validator() {
    let mut state = playing(3);
    idle(&mut state, 400);
    let economy = state.economy_of(CommanderId(2));
    let kits = economy
        .holdings
        .iter()
        .any(|h| h.kits > 0 || h.queue.iter().any(|q| q.item == BuildItem::CoreKit));
    assert!(kits, "the first kit is queued in the first minute");
    let counters = state
        .ai_counters()
        .get(&(
            Owner::Commander(CommanderId(2)),
            nomad_ai::Layer::Strategist,
        ))
        .cloned()
        .unwrap_or_default();
    assert!(counters.applied > 0, "{counters:?}");
    // Nobody else's Strategist runs: they are offline.
    assert!(!state.ai_counters().contains_key(&(
        Owner::Commander(CommanderId(3)),
        nomad_ai::Layer::Strategist
    )));
}

#[test]
#[ignore = "long simulation: two hours of a stage-1 Universe; CI runs fast tests only (D-190), run with --ignored"]
fn the_first_claim_is_a_crystal_vein_and_a_snapshot_mid_trip_gives_the_same_decisions() {
    let mut state = playing(3);
    let mut twin = None;
    let mut deployed = None;
    for t in 0..Ticks::from_hours(2).get() {
        if t == Ticks::from_minutes(20).get() {
            twin = Some(decode(&encode(&state).unwrap()).unwrap());
        }
        let out = step(&mut state, &TickInputs::default());
        for event in &out.economy_events {
            if let crate::economy::EconomyEvent::OutpostDeployed { site, holding } = *event
                && state
                    .holding(holding)
                    .is_some_and(|h| h.owner == CommanderId(2))
                && deployed.is_none()
            {
                deployed = Some(site);
            }
        }
    }
    let site = deployed.expect("an outpost within two hours");
    let deposits = &state.galaxy().site(site).unwrap().deposits;
    assert!(
        deposits.iter().any(|d| d.kind == DepositKind::CrystalVein),
        "{deposits:?}"
    );
    // The restored twin decides the same.
    let mut twin = twin.unwrap();
    while twin.tick() < state.tick() {
        step(&mut twin, &TickInputs::default());
    }
    assert_eq!(state_hash(&twin).unwrap(), state_hash(&state).unwrap());
    assert!(state.economy_problems().is_empty() && state.fleet_problems().is_empty());
}

#[test]
fn a_snapshot_during_a_sign_off_gives_the_same_decisions() {
    // Start the season 3 min before commander 2's first weekday session ends: it is online
    // from the start and signs off before the session ends (D-214).
    let probe = scheduled(MONDAY);
    let first = probe.schedule(CommanderId(2)).unwrap().weekday[0];
    let end_min = i64::from(first.start_min) + i64::from(first.length_min);
    let mut state = scheduled(MONDAY + (end_min - 3) * 60_000);
    let signed = |s: &UniverseState| {
        s.strategist_stats(CommanderId(2))
            .map_or(0, |x| x.sign_offs)
    };
    let mut twin = None;
    for _ in 0..Ticks::from_minutes(3).get() {
        step(&mut state, &TickInputs::default());
        if twin.is_none() && signed(&state) > 0 {
            twin = Some(decode(&encode(&state).unwrap()).unwrap());
        }
    }
    let mut twin = twin.expect("a sign-off before the session ends");
    // Past the session end, both run on: the restored twin decides the same.
    idle(&mut state, 600);
    assert!(!online(&state, 2));
    while twin.tick() < state.tick() {
        step(&mut twin, &TickInputs::default());
    }
    assert!(signed(&state) > 0);
    assert_eq!(state_hash(&twin).unwrap(), state_hash(&state).unwrap());
    assert!(state.economy_problems().is_empty() && state.fleet_problems().is_empty());
}
