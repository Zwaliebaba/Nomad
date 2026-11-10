//! The raid launch rule (A3 plan §9 "Rule tests").

use nomad_types::{FactionId, Owner, RulesOfEngagement};

use super::*;
use crate::fleet::ShipClass;
use crate::galaxy::SystemKind;
use crate::geometry::{Vec2, distance_m};
use crate::input::{OperatorInput, TickInputs};
use crate::state::UniverseState;
use crate::step::step;
use crate::testkit::{COMMANDERS, home, spawn_unseen, stage1};

const RAIDERS: FactionId = FactionId(1);
const HOUR: u64 = 36_000;

fn faction(state: &UniverseState) -> RaidFactionView {
    state
        .raid_factions()
        .into_iter()
        .next()
        .expect("stage 1 has a raider faction")
}

/// Fires the faction's timer directly at its deadline, as phase 2 would.
fn fire(state: &mut UniverseState) -> (Tick, Vec<RaidEvent>) {
    let at = faction(state).next_launch;
    // The queued timer would fire it too; take it out, as phase 2 does.
    while state.timers.pop_due(at).is_some() {}
    let mut events = Vec::new();
    launch(state, at, at, RAIDERS, &mut events);
    (at, events)
}

fn launched(events: &[RaidEvent]) -> Option<RaidEvent> {
    events
        .iter()
        .copied()
        .find(|e| matches!(e, RaidEvent::Launched { .. }))
}

fn hub(state: &UniverseState) -> SystemId {
    state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == SystemKind::Hub)
        .map(|(id, _)| id)
        .expect("stage 1 has a hub")
}

#[test]
fn stage_one_has_one_faction_whose_first_launch_is_two_to_four_hours_in() {
    for seed in [1, 7, 99] {
        let state = stage1(seed);
        let factions = state.raid_factions();
        assert_eq!(factions.len(), 1);
        let f = &factions[0];
        assert_eq!(f.faction, RAIDERS);
        assert_eq!(
            state.galaxy().system(f.haven).map(|s| s.kind),
            Some(SystemKind::RaiderHaven)
        );
        assert!(f.groups.is_empty());
        assert!(
            (2 * HOUR..=4 * HOUR).contains(&f.next_launch.get()),
            "seed {seed}: first launch at {}",
            f.next_launch.get()
        );
        assert!(
            state.timers.pending().any(|(t, e)| t == f.next_launch
                && *e == ScheduledEvent::RaidLaunch { faction: RAIDERS })
        );
    }
}

#[test]
fn launch_intervals_stay_within_two_to_four_hours() {
    let mut state = stage1(3);
    let mut previous = Tick::ZERO;
    let (mut shortest, mut longest) = (u64::MAX, 0);
    for _ in 0..200 {
        let (at, _) = fire(&mut state);
        let gap = at.get() - previous.get();
        assert!((2 * HOUR..=4 * HOUR).contains(&gap), "gap {gap}");
        shortest = shortest.min(gap);
        longest = longest.max(gap);
        previous = at;
        assert_eq!(state.timers.len(), 1, "one launch timer");
    }
    // The draw spreads over the whole range.
    assert!(shortest < 2 * HOUR + HOUR / 4 && longest > 4 * HOUR - HOUR / 4);
}

#[test]
fn no_commander_is_targeted_twice_within_six_hours() {
    let mut state = stage1(5);
    let mut last: BTreeMap<CommanderId, Tick> = BTreeMap::new();
    let mut launches = 0;
    for _ in 0..120 {
        let (at, events) = fire(&mut state);
        let Some(RaidEvent::Launched { target, .. }) = launched(&events) else {
            continue;
        };
        launches += 1;
        if let Some(before) = last.insert(target, at) {
            assert!(at.get() - before.get() >= 6 * HOUR, "{target:?} again");
        }
        assert_eq!(state.last_raided(target), Some(at));
    }
    // Four commanders and a launch every 2–4 h: most firings find someone eligible.
    assert!(launches > 60, "{launches} launches");
}

#[test]
fn a_firing_during_catch_up_launches_nothing_and_draws_the_next_time_as_usual() {
    let mut live = stage1(11);
    let mut catching_up = live.clone();
    catching_up.catching_up = true;
    let (_, live_events) = fire(&mut live);
    let (_, events) = fire(&mut catching_up);
    assert_eq!(events, vec![RaidEvent::Suppressed { faction: RAIDERS }]);
    assert!(faction(&catching_up).groups.is_empty());
    assert!(launched(&live_events).is_some());
    // The next launch is the same either way: it is drawn first (D-047: not deferred).
    assert_eq!(
        faction(&catching_up).next_launch,
        faction(&live).next_launch
    );
}

#[test]
fn the_timer_fires_in_the_step_and_catch_up_suppresses_it() {
    let mut state = stage1(2);
    let at = faction(&state).next_launch;
    let mut suppressed = state.clone();
    state.tick = Tick::new(at.get() - 1);
    let out = step(&mut state, &TickInputs::default());
    assert!(matches!(
        launched(&out.raid_events),
        Some(RaidEvent::Launched {
            faction: RAIDERS,
            ..
        })
    ));
    assert_eq!(faction(&state).groups.len(), 1);

    suppressed.tick = Tick::new(at.get() - 1);
    let out = step(
        &mut suppressed,
        &TickInputs {
            operator: vec![OperatorInput::CatchUpStart],
            commanders: vec![],
        },
    );
    assert_eq!(
        out.raid_events,
        vec![RaidEvent::Suppressed { faction: RAIDERS }]
    );
    assert!(suppressed.fleet_ids().len() < state.fleet_ids().len());
}

#[test]
fn the_budget_buys_strike_craft_and_one_heavy_cruiser_with_a_floor_of_four() {
    let rules = Rules::embedded();
    // D-313's floor of 4 strike craft.
    assert_eq!(composition(&rules, 0), (0, 4));
    assert_eq!(composition(&rules, 3_300), (0, 4));
    assert_eq!(composition(&rules, 6_399), (0, 4));
    assert_eq!(composition(&rules, 8_000), (0, 5));
    // An HSTL needs 17,000 plus two strike craft, and the floor still holds beside it.
    assert_eq!(composition(&rules, 20_199), (0, 12));
    assert_eq!(composition(&rules, 20_200), (1, 4));
    assert_eq!(composition(&rules, 50_000), (1, 20));
    // D-220's values: VX 1,200, LANCER 2,300, WARDEN 5,800, MULE 1,500.
    let value = |c| class_value(&rules, c);
    assert_eq!(value(ShipClass::Vx), 1_200);
    assert_eq!(value(ShipClass::Lancer), 2_300);
    assert_eq!(value(ShipClass::Warden), 5_800);
    assert_eq!(value(ShipClass::Mule), 1_500);
    assert_eq!(value(ShipClass::RaiderStrikeCraft), 1_600);
    assert_eq!(value(ShipClass::HeavyCruiser), 17_000);
}

#[test]
fn strength_is_thirty_to_fifty_percent_of_the_fleet_value_in_the_system() {
    let mut shares = Vec::new();
    for seed in 0..40 {
        let mut state = stage1(seed);
        let (_, events) = fire(&mut state);
        let Some(RaidEvent::Launched {
            faction: RAIDERS,
            fleet,
            target,
            system,
            fleet_value: worth,
            share_permille,
            budget,
            strike_craft,
            heavy_cruisers,
            value,
        }) = launched(&events)
        else {
            panic!("seed {seed}: a fresh Universe always has a target");
        };
        assert!((300..=500).contains(&share_permille));
        assert_eq!(worth, fleet_value(&state, target, system));
        assert_eq!(budget, worth * u64::from(share_permille) / 1_000);
        assert_eq!(
            composition(&state.rules, budget),
            (heavy_cruisers, strike_craft)
        );
        // The only fleet at the start is the kit at home: 3 VX and 2 MULE, 6,600; elsewhere
        // nothing, which the floor still raids (A3 plan §4.4 example; D-313's floor of 4).
        let (_, home_system) = home(&state, target.0);
        let expected = if system == home_system { 6_600 } else { 0 };
        assert_eq!(worth, expected);
        assert_eq!((heavy_cruisers, strike_craft), (0, 4));
        assert_eq!(value, 6_400);
        let ships = state.fleet(fleet).expect("the group exists").ships;
        assert_eq!(ships.len(), 4);
        shares.push(share_permille);
    }
    assert!(shares.iter().any(|&s| s < 350) && shares.iter().any(|&s| s > 450));
}

#[test]
fn a_bigger_fleet_in_the_system_draws_a_bigger_raid() {
    let mut state = stage1(4);
    // Every commander keeps eight WARDENs at home: 46,400 more, so 53,000 in all.
    for n in 1..=4 {
        let (_, system) = home(&state, n);
        spawn_unseen(
            &mut state,
            Owner::Commander(CommanderId(n)),
            system,
            Vec2::new(30_000.0, 30_000.0),
            &[ShipClass::Warden; 8],
        );
    }
    let (_, events) = fire(&mut state);
    let Some(RaidEvent::Launched {
        fleet_value,
        heavy_cruisers,
        strike_craft,
        ..
    }) = launched(&events)
    else {
        panic!("a launch");
    };
    assert_eq!(fleet_value, 6_600 + 8 * 5_800);
    // 30–50% of 53,000 is 15,900–26,500: an HSTL from 20,200, strike craft otherwise.
    let budget_ok = (4..=16).contains(&strike_craft);
    assert!(budget_ok && heavy_cruisers <= 1, "{events:?}");
}

#[test]
fn target_weights_use_public_data_only() {
    let state = stage1(9);
    let haven = faction(&state).haven;
    let now = faction(&state).next_launch;
    let mut hidden = state.clone();
    // Hidden state: fleets nobody has seen, the start kits moved out of their docks.
    for n in 1..=4 {
        let (_, system) = home(&hidden, n);
        spawn_unseen(
            &mut hidden,
            Owner::Commander(CommanderId(n)),
            system,
            Vec2::new(20_000.0, -20_000.0),
            &[ShipClass::Lancer; 6],
        );
    }
    assert_eq!(
        target_weights(&state, haven, now),
        target_weights(&hidden, haven, now)
    );
    let (mut a, mut b) = (state, hidden);
    let target = |events: &[RaidEvent]| match launched(events) {
        Some(RaidEvent::Launched { target, system, .. }) => (target, system),
        _ => panic!("a launch"),
    };
    let (_, ea) = fire(&mut a);
    let (_, eb) = fire(&mut b);
    // The same target, a different strength.
    assert_eq!(target(&ea), target(&eb));
    assert_ne!(ea, eb);
}

#[test]
fn target_weights_follow_assets_and_distance() {
    let mut state = stage1(6);
    let haven = faction(&state).haven;
    let now = Tick::ZERO;
    let weights = target_weights(&state, haven, now);
    // At the start, every commander has its mother station at home, 3 jumps out (doc 10
    // §4.4): (site value + 1) ÷ 4.
    assert_eq!(weights.len(), COMMANDERS.len());
    for ((commander, system), weight) in &weights {
        let (id, home_system) = home(&state, commander.0);
        assert_eq!(*system, home_system);
        let site = state.holding(id).expect("mother station").site;
        let value = state.galaxy().site_value(site).expect("site").milli();
        assert_eq!(*weight, (value + 1_000) / 4);
    }
    // An outpost in the hub, 1 jump out: (site value + 2) ÷ 2.
    let hub = hub(&state);
    let site = state.galaxy().system(hub).expect("hub").sites[0];
    state
        .insert_holding(site, CommanderId(2), false)
        .expect("free site");
    let weights = target_weights(&state, haven, now);
    let value = state.galaxy().site_value(site).expect("site").milli();
    let jumps = 1;
    let weight = (value + 2_000) / (1 + jumps);
    assert!(weights.contains(&((CommanderId(2), hub), weight)));
    // A commander raided in the last 6 h is no candidate.
    state.raids.last_targeted.insert(CommanderId(2), Tick::ZERO);
    let weights = target_weights(&state, haven, Tick::new(6 * HOUR - 1));
    assert!(weights.iter().all(|((c, _), _)| *c != CommanderId(2)));
    let weights = target_weights(&state, haven, Tick::new(6 * HOUR));
    assert!(weights.iter().any(|((c, _), _)| *c == CommanderId(2)));
}

#[test]
fn a_group_spawns_at_the_haven_gate_and_is_removed_when_it_returns() {
    let mut state = stage1(8);
    let (at, events) = fire(&mut state);
    let Some(RaidEvent::Launched { fleet, .. }) = launched(&events) else {
        panic!("a launch");
    };
    let f = faction(&state);
    let gate = state.galaxy().system(f.haven).expect("haven").gates[0];
    let gate_at = state.galaxy().gate(gate).expect("gate").position;
    let view = state.fleet(fleet).expect("the group");
    assert_eq!(view.owner, Owner::Npc(RAIDERS));
    assert_eq!(view.place, Place::InSystem(f.haven));
    assert_eq!(view.rules_of_engagement, RulesOfEngagement::WeaponsFree);
    assert!(view.order.is_none());
    for &ship in &view.ships {
        let s = state.ship(ship).expect("ship");
        assert!(distance_m(s.position, gate_at) <= 2_000.0);
        assert_eq!(s.hull, s.max_hull);
        assert_eq!(s.shroud_until, None);
    }
    assert!(state.fleet_problems().is_empty());
    assert_eq!(
        state.raid_group(fleet).map(|(id, g)| (id, g.launched)),
        Some((RAIDERS, at))
    );

    // Back from a raid: it arrives at the haven's gate on the next tick.
    let lane = state.galaxy().gate(gate).expect("gate").lanes[0];
    state.tick = at;
    let arrive = at.next();
    state.transits.schedule(&mut state.ids, arrive, fleet);
    state
        .entities
        .get_mut::<Fleet>(fleet.sim_id())
        .expect("fleet")
        .place = Place::InTransit {
        lane,
        to_gate: gate,
        arrive,
    };
    let out = step(&mut state, &TickInputs::default());
    assert_eq!(
        out.raid_events,
        vec![RaidEvent::Returned {
            faction: RAIDERS,
            fleet,
            ships: 4
        }]
    );
    assert!(state.fleet(fleet).is_none());
    assert!(view.ships.iter().all(|&s| state.ship(s).is_none()));
    assert!(faction(&state).groups.is_empty());
    assert!(state.fleet_problems().is_empty());
}

#[test]
fn a_destroyed_group_ends_its_record() {
    let mut state = stage1(10);
    let (_, events) = fire(&mut state);
    let Some(RaidEvent::Launched { fleet, .. }) = launched(&events) else {
        panic!("a launch");
    };
    for ship in state.fleet(fleet).expect("the group").ships {
        state.entities.despawn(ship.sim_id());
    }
    state.entities.despawn(fleet.sim_id());
    let mut events = Vec::new();
    prune(&mut state, &mut events);
    assert_eq!(
        events,
        vec![RaidEvent::Destroyed {
            faction: RAIDERS,
            fleet
        }]
    );
    assert!(faction(&state).groups.is_empty());
}

#[test]
fn a_raid_on_the_hub_is_sized_by_the_ships_there() {
    let mut state = stage1(12);
    let hub = hub(&state);
    // Only commander 3 is eligible, with an outpost and 2 VX in the hub.
    for c in [1, 2, 4] {
        state.raids.last_targeted.insert(CommanderId(c), Tick::ZERO);
    }
    let site = state.galaxy().system(hub).expect("hub").sites[0];
    state
        .insert_holding(site, CommanderId(3), false)
        .expect("free site");
    spawn_unseen(
        &mut state,
        Owner::Commander(CommanderId(3)),
        hub,
        Vec2::new(0.0, 25_000.0),
        &[ShipClass::Vx; 2],
    );
    let at = Tick::new(HOUR);
    let mut weights = target_weights(&state, faction(&state).haven, at);
    let mut events = Vec::new();
    launch(&mut state, at, at, RAIDERS, &mut events);
    weights.retain(|((c, _), _)| *c == CommanderId(3));
    let draws: Vec<_> = launched(&events).into_iter().collect();
    // The hub outpost weighs (value + 2) ÷ 2 against the home's (value + 1) ÷ 4.
    assert_eq!(weights.len(), 2);
    let Some(RaidEvent::Launched {
        system,
        fleet_value,
        ..
    }) = draws.first().copied()
    else {
        panic!("a launch");
    };
    assert_eq!(
        fleet_value,
        if system == hub { 2_400 } else { 6_600 },
        "{system:?}"
    );
}
