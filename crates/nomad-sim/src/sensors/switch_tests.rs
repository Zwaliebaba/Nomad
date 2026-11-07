//! Tests of the switch from "everything is visible" to detection (S7 plan §4.6, §7
//! "Switch-over tests"): auto-targeting and lock, the lost Attack target (D-180), orders that
//! follow only what is seen, and fleets in an emergency jump (S6b). The validator's
//! `NotVisible` is in `command::tests`, the engagement trigger in `combat::tests`.

use nomad_types::{FleetOrder, Owner, RulesOfEngagement};

use crate::combat::{WeaponSlot, Weapons};
use crate::fleet::{ActiveOrder, Fleet, FleetEvent, FleetId, Motion, ShipClass, Task};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::testkit::{
    commander, home, idle, order, send, sense, site_position, spawn_unseen, stage1,
};

const VX: ShipClass = ShipClass::Vx;

/// Commander 1's home system, 8 km from its mother station, where commanders 2 and 3 have
/// only their ships' sensors.
fn arena() -> (UniverseState, SystemId, Vec2) {
    let state = stage1(5);
    let (_, system) = home(&state, 1);
    let base = site_position(&state, system, 0) + Vec2::new(8_000.0, 0.0);
    (state, system, base)
}

fn place(
    state: &mut UniverseState,
    owner: Owner,
    system: SystemId,
    centre: Vec2,
    roe: RulesOfEngagement,
) -> FleetId {
    let fleet = spawn_unseen(state, owner, system, centre, &[VX]);
    if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
        f.rules_of_engagement = roe;
    }
    fleet
}

fn first_ship(state: &UniverseState, fleet: FleetId) -> crate::fleet::ShipId {
    state.fleet(fleet).unwrap().ships[0]
}

fn slot(state: &UniverseState, fleet: FleetId) -> WeaponSlot {
    state
        .entities
        .get::<Weapons>(first_ship(state, fleet).sim_id())
        .map(|w| w.slots[0])
        .unwrap_or_default()
}

fn teleport(state: &mut UniverseState, fleet: FleetId, centre: Vec2) {
    let ships = state.fleet(fleet).unwrap().ships;
    for id in std::iter::once(fleet.sim_id()).chain(ships.iter().map(|s| s.sim_id())) {
        let m = state.entities.get_mut::<Motion>(id).unwrap();
        m.position = centre.into();
        m.velocity = Vec2::ZERO;
    }
}

fn task(state: &UniverseState, fleet: FleetId) -> Option<Task> {
    state
        .entities
        .get::<Fleet>(fleet.sim_id())
        .and_then(|f| f.active)
        .map(|ActiveOrder { task, .. }| task)
}

fn fleet_events(outputs: &[TickOutputs]) -> Vec<FleetEvent> {
    outputs
        .iter()
        .flat_map(|o| o.fleet_events.iter().copied())
        .collect()
}

#[test]
fn weapons_choose_and_lock_only_what_their_commander_detects() {
    let (mut state, system, base) = arena();
    let shooter = place(
        &mut state,
        commander(2),
        system,
        base,
        RulesOfEngagement::WeaponsFree,
    );
    // In weapon range (1.5 km), but no pass has seen it yet.
    let target = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(1_000.0, 0.0),
        RulesOfEngagement::HoldFire,
    );
    for _ in 0..4 {
        idle(&mut state, 1);
        assert_eq!(slot(&state, shooter).target, None, "{:?}", state.tick());
    }
    // The pass at tick 5 holds it, and lock starts to build (doc 15 §4).
    idle(&mut state, 1);
    assert_eq!(
        slot(&state, shooter).ship_target(),
        Some(first_ship(&state, target))
    );
    assert_eq!(slot(&state, shooter).lock_ms, 100);
    idle(&mut state, 3);
    assert_eq!(slot(&state, shooter).lock_ms, 400);
    // A target out of the picture is dropped and its lock lost, however close it is. (The
    // fixture empties the shooter's picture before tick 9, which is not a pass tick; no MVP
    // weapon can reach beyond its own sensor.)
    if let Some(o) = state.sensors.observers.get_mut(&commander(2)) {
        o.picture.systems.clear();
    }
    idle(&mut state, 1);
    assert_eq!(slot(&state, shooter).target, None);
    assert_eq!(slot(&state, shooter).lock_ms, 0);
}

/// Commander 2's VX attacking commander 3's VX 5 km away, both seen.
fn attack(state: &mut UniverseState, system: SystemId, base: Vec2) -> (FleetId, FleetId) {
    let attacker = place(
        state,
        commander(2),
        system,
        base,
        RulesOfEngagement::WeaponsFree,
    );
    let target = place(
        state,
        commander(3),
        system,
        base + Vec2::new(5_000.0, 0.0),
        RulesOfEngagement::HoldFire,
    );
    sense(state);
    let out = send(
        state,
        2,
        order(
            attacker,
            FleetOrder::Attack {
                target: target.sim_id(),
            },
        ),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    idle(state, 10);
    assert!(matches!(
        task(state, attacker),
        Some(Task::Attack { seen: Some(_), .. })
    ));
    (attacker, target)
}

/// Moves `target` out of view to `far` (a fixture) and runs until `attacker` has lost it:
/// returns where it was last seen. Until the next pass the target is still in the picture,
/// so the attacker sees it at `far` (S7 plan §4.3).
fn lose(state: &mut UniverseState, attacker: FleetId, target: FleetId, far: Vec2) -> Vec2 {
    teleport(state, target, far);
    for _ in 0..10 {
        idle(state, 1);
        if let Some(Task::Pursue { to, .. }) = task(state, attacker) {
            return to.into();
        }
    }
    panic!("the target was not lost: {:?}", task(state, attacker));
}

#[test]
fn a_lost_attack_target_is_followed_to_where_it_was_last_seen() {
    let (mut state, system, base) = arena();
    let (attacker, target) = attack(&mut state, system, base);
    let Some(Task::Attack {
        seen: Some(seen), ..
    }) = task(&state, attacker)
    else {
        panic!("attacking");
    };
    assert_eq!(
        Vec2::from(seen),
        Vec2::from(state.fleet(target).unwrap().position)
    );
    // It vanishes from view: the attacker goes where it was last seen, holds there and
    // reports TargetLost (D-180). Where it really went since is not followed.
    let far = base + Vec2::new(40_000.0, 0.0);
    let seen = lose(&mut state, attacker, target, far);
    assert_eq!(seen, far);
    teleport(&mut state, target, far + Vec2::new(0.0, 30_000.0));
    let outputs = idle(&mut state, 3_000);
    let events = fleet_events(&outputs);
    assert!(events.contains(&FleetEvent::TargetLost { fleet: attacker }));
    assert!(!events.contains(&FleetEvent::OrderDropped { fleet: attacker }));
    assert!(!events.contains(&FleetEvent::OrderCompleted { fleet: attacker }));
    let view = state.fleet(attacker).unwrap();
    assert_eq!(view.order, None, "the order is done");
    assert!((Vec2::from(view.position) - seen).length() < 1.0);
    idle(&mut state, 100);
    assert!((Vec2::from(state.fleet(attacker).unwrap().position) - seen).length() < 1.0);
}

#[test]
fn a_lost_attack_target_seen_again_on_the_way_is_attacked_again() {
    let (mut state, system, base) = arena();
    let (attacker, target) = attack(&mut state, system, base);
    lose(
        &mut state,
        attacker,
        target,
        base + Vec2::new(40_000.0, 0.0),
    );
    // Back in view, 3 km from the attacker, before it gets there.
    let near = Vec2::from(state.fleet(attacker).unwrap().position) + Vec2::new(0.0, 3_000.0);
    teleport(&mut state, target, near);
    let outputs = idle(&mut state, 20);
    assert!(matches!(
        task(&state, attacker),
        Some(Task::Attack { seen: Some(_), .. })
    ));
    let events = fleet_events(&outputs);
    assert!(!events.contains(&FleetEvent::TargetLost { fleet: attacker }));
}

#[test]
fn guard_on_a_foreign_fleet_is_dropped_when_it_goes_out_of_view() {
    let (mut state, system, base) = arena();
    let guard = place(
        &mut state,
        commander(2),
        system,
        base,
        RulesOfEngagement::HoldFire,
    );
    let ward = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(2_000.0, 0.0),
        RulesOfEngagement::HoldFire,
    );
    sense(&mut state);
    let out = send(
        &mut state,
        2,
        order(
            guard,
            FleetOrder::Guard {
                object: ward.sim_id(),
            },
        ),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    teleport(&mut state, ward, base + Vec2::new(40_000.0, 0.0));
    let outputs = idle(&mut state, 6);
    assert!(fleet_events(&outputs).contains(&FleetEvent::OrderDropped { fleet: guard }));
    assert_eq!(task(&state, guard), None);
}

#[test]
fn a_fleet_in_an_emergency_jump_is_neither_seen_nor_a_target_and_senses_nothing() {
    let (mut state, system, base) = arena();
    let shooter = place(
        &mut state,
        commander(2),
        system,
        base,
        RulesOfEngagement::WeaponsFree,
    );
    let jumper = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(1_000.0, 0.0),
        RulesOfEngagement::HoldFire,
    );
    sense(&mut state);
    idle(&mut state, 5);
    assert_eq!(
        slot(&state, shooter).ship_target(),
        Some(first_ship(&state, jumper))
    );
    let sensors = state.sensors_detection_stats().sensors;
    // It enters an emergency jump (S6b, D-166; a fixture): invisible and untargetable like
    // lane transit, and no sensor.
    let site = state.galaxy().system(system).unwrap().sites[0];
    if let Some(f) = state.entities.get_mut::<Fleet>(jumper.sim_id()) {
        f.place = crate::fleet::Place::EmergencyJump {
            site,
            arrive: crate::time::Tick::new(100_000),
        };
    }
    assert_eq!(state.sensors_detection_stats().sensors, sensors - 1);
    idle(&mut state, 5);
    let seen = |state: &UniverseState, observer, fleet: FleetId| {
        state.view(observer).sensed_fleet(fleet).is_some()
    };
    assert!(!seen(&state, commander(2), jumper));
    assert!(!seen(&state, commander(3), shooter));
    assert!(state.view(commander(2)).last_known_fleet(jumper).is_none());
    assert_eq!(slot(&state, shooter).target, None);
    let out = send(
        &mut state,
        2,
        order(
            shooter,
            FleetOrder::Attack {
                target: jumper.sim_id(),
            },
        ),
    );
    assert!(out.commands[0].outcome.is_err());
}
