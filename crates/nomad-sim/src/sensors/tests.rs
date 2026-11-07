//! Tests of sensors, memory and lane traffic against S7 plan §7.

use nomad_types::{
    CommanderId, FactionId, FleetOrder, GateId, Owner, RulesOfEngagement, SystemKind,
};

use super::{SystemPicture, VisibilityEvent};
use crate::fleet::{Fleet, FleetEvent, FleetId, Motion, Place, ShipClass, ShipId};
use crate::galaxy::{STAGE1_COMMANDERS, SystemId, generate_stage1_with_rules};
use crate::geometry::Vec2;
use crate::rules::Rules;
use crate::snapshot::{decode, encode, state_hash};
use crate::state::{SeasonConfig, UniverseState};
use crate::step::TickOutputs;
use crate::testkit::{
    COMMANDERS, at, commander, home, idle, order, send, sense, site_position, spawn_unseen, stage1,
};
use crate::time::{Tick, Ticks};

const VX: ShipClass = ShipClass::Vx;
const LANCER: ShipClass = ShipClass::Lancer;
const WARDEN: ShipClass = ShipClass::Warden;
const RAIDER: ShipClass = ShipClass::RaiderStrikeCraft;

/// A stage-1 Universe and a whole-metre point in commander 1's home system 8 km from its
/// mother station: commanders 2 and 3 have no sensors there but their ships.
fn arena() -> (UniverseState, SystemId, Vec2) {
    let state = stage1(5);
    let (_, system) = home(&state, 1);
    let site = site_position(&state, system, 0);
    let base = Vec2::new(site.x.round() + 8_000.0, site.y.round());
    (state, system, base)
}

/// A fleet that holds fire, so tests of seeing are not tests of fighting.
fn place(
    state: &mut UniverseState,
    owner: Owner,
    system: SystemId,
    centre: Vec2,
    classes: &[ShipClass],
) -> FleetId {
    let fleet = spawn_unseen(state, owner, system, centre, classes);
    if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
        f.rules_of_engagement = RulesOfEngagement::HoldFire;
    }
    fleet
}

fn ships(state: &UniverseState, fleet: FleetId) -> Vec<ShipId> {
    state.fleet(fleet).map(|f| f.ships).unwrap_or_default()
}

/// Puts `fleet` and its ships at rest with its anchor at `centre`, keeping the formation.
fn teleport(state: &mut UniverseState, fleet: FleetId, centre: Vec2) {
    let anchor = state.entities.get::<Motion>(fleet.sim_id()).unwrap();
    let shift = centre - Vec2::from(anchor.position);
    for id in std::iter::once(fleet.sim_id()).chain(ships(state, fleet).iter().map(|s| s.sim_id()))
    {
        let m = state.entities.get_mut::<Motion>(id).unwrap();
        m.position = (Vec2::from(m.position) + shift).into();
        m.velocity = Vec2::ZERO;
    }
}

fn picture(state: &UniverseState, observer: Owner, system: SystemId) -> SystemPicture {
    state
        .sensors
        .observers
        .get(&observer)
        .and_then(|o| o.picture.system(system))
        .cloned()
        .unwrap_or_default()
}

fn events(outputs: &[TickOutputs]) -> Vec<VisibilityEvent> {
    outputs
        .iter()
        .flat_map(|o| o.visibility_events.iter().copied())
        .collect()
}

#[test]
fn detection_follows_range_times_signature_exactly_at_the_boundary() {
    let (mut state, system, base) = arena();
    let eye = place(&mut state, commander(2), system, base, &[VX]);
    // A VX's sensor reaches 15 km at signature 1.0, so 9 km for a Light target (D-072, D-073).
    let target = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(9_000.0, 0.0),
        &[VX],
    );
    sense(&mut state);
    let seen = picture(&state, commander(2), system);
    assert_eq!(seen.ships, ships(&state, target));
    assert_eq!(seen.fleets, vec![target]);
    // Just beyond: not detected. The comparison is on squared whole-metre distances, exact.
    teleport(&mut state, target, base + Vec2::new(9_000.000_001, 0.0));
    sense(&mut state);
    assert!(picture(&state, commander(2), system).ships.is_empty());
    // A Medium target is seen at the full 15 km, a Heavy one at 22.5 km.
    let warden = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(0.0, 15_000.0),
        &[WARDEN],
    );
    let far = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(0.0, -15_000.001),
        &[WARDEN],
    );
    sense(&mut state);
    let seen = picture(&state, commander(2), system);
    assert_eq!(seen.fleets, vec![warden]);
    assert!(!seen.has_fleet(far));
    // Nobody detects their own ships: they are own information.
    assert!(!picture(&state, commander(3), system).has_fleet(warden));
    // Commander 2's VX is seen by commander 3's WARDEN (25 km × 0.6 = 15 km).
    assert!(picture(&state, commander(3), system).has_fleet(eye));
}

#[test]
fn system_control_reveals_the_whole_system_gate_approaches_included() {
    let mut state = stage1(5);
    let (system, sites) = state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == SystemKind::Standard)
        .map(|(id, s)| (id, s.sites.clone()))
        .unwrap();
    // A gate as far as possible from the sites commander 2 will hold.
    let majority = sites.len() / 2 + 1;
    let held: Vec<Vec2> = sites[..majority]
        .iter()
        .map(|&s| state.galaxy().site(s).unwrap().position.into())
        .collect();
    let gate: Vec2 = state
        .galaxy()
        .system(system)
        .unwrap()
        .gates
        .iter()
        .map(|&g| Vec2::from(state.galaxy().gate(g).unwrap().position))
        .max_by(|a, b| {
            let d = |p: &Vec2| {
                held.iter()
                    .map(|h| (*h - *p).length())
                    .fold(f64::MAX, f64::min)
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    for h in &held {
        // No core of commander 2 reaches a Light ship there (40 km × 0.6).
        assert!(
            (*h - gate).length() > 24_000.0,
            "precondition: pick another system"
        );
    }
    let intruder = place(&mut state, commander(3), system, gate, &[LANCER]);
    for &site in &sites[..majority - 1] {
        state.insert_holding(site, CommanderId(2), true).unwrap();
    }
    sense(&mut state);
    assert!(!picture(&state, commander(2), system).has_fleet(intruder));
    state
        .insert_holding(sites[majority - 1], CommanderId(2), true)
        .unwrap();
    assert_eq!(
        state.control_state(system),
        Some(crate::ControlState::Controlled(CommanderId(2)))
    );
    sense(&mut state);
    let seen = picture(&state, commander(2), system);
    assert!(seen.has_fleet(intruder), "control covers the gate approach");
    assert_eq!(seen.ships, ships(&state, intruder));
}

#[test]
fn a_mixed_fleet_is_detected_member_by_member() {
    let (mut state, system, base) = arena();
    place(&mut state, commander(2), system, base, &[VX]);
    // 12 km away: the WARDEN (Medium, 15 km) is detected, the LANCER (Light, 9 km) is not.
    let mixed = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(12_000.0, 0.0),
        &[WARDEN, LANCER],
    );
    sense(&mut state);
    let members = ships(&state, mixed);
    let warden = members[0];
    let seen = picture(&state, commander(2), system);
    assert_eq!(seen.ships, vec![warden]);
    assert_eq!(seen.fleets, vec![mixed]);
    // The view lists the fleet with its detected member only (D-174).
    let view = state.view(commander(2));
    let fleets = view.sensed_fleets_in(system);
    assert_eq!(fleets.len(), 1);
    assert_eq!(
        fleets[0]
            .ships
            .iter()
            .map(|s| (s.id, s.class))
            .collect::<Vec<_>>(),
        vec![(warden, WARDEN)]
    );
    assert!(view.sensed_ship(members[1]).is_none());
    assert_eq!(
        view.sensed_ship(warden).map(|s| s.hull_permille),
        Some(1_000)
    );
}

#[test]
fn a_detected_holding_reveals_its_modules_and_is_remembered_until_its_owner_changes() {
    let (mut state, system, _) = arena();
    let (mother, _) = home(&state, 1);
    let site = site_position(&state, system, 0);
    // A Structure target (1.5) is seen by a VX at 22.5 km.
    let scout = place(
        &mut state,
        commander(2),
        system,
        site + Vec2::new(22_000.0, 0.0),
        &[VX],
    );
    sense(&mut state);
    let view = state.view(commander(2));
    let seen = view
        .sensed_holding(mother)
        .expect("the holding is detected");
    assert_eq!(seen.owner, CommanderId(1));
    assert_eq!(seen.modules.len(), state.modules_at(mother).len());
    assert!(seen.modules.len() >= 10, "the start station's modules");
    assert!(seen.sectors.is_some(), "and its dome");
    assert!(view.last_known_holding(mother).is_none());
    // Out of range: remembered with what was seen (D-177), until it changes hands (D-076).
    teleport(&mut state, scout, site + Vec2::new(23_000.0, 0.0));
    sense(&mut state);
    let view = state.view(commander(2));
    assert!(view.sensed_holding(mother).is_none());
    let remembered = view.last_known_holding(mother).expect("remembered");
    assert_eq!(remembered.holding, seen);
    for _ in 0..10 {
        sense(&mut state);
    }
    assert!(
        state
            .view(commander(2))
            .last_known_holding(mother)
            .is_some()
    );
    state.holdings.get_mut(&mother).unwrap().owner = CommanderId(3);
    sense(&mut state);
    assert!(
        state
            .view(commander(2))
            .last_known_holding(mother)
            .is_none()
    );
}

#[test]
fn docked_ships_fleets_in_transit_and_outposts_sense_nothing() {
    let mut state = stage1(5);
    // Four mother-station cores; the start kits are docked.
    let stats = state.sensors_detection_stats();
    assert_eq!(stats.sensors, 4);
    let (_, system) = home(&state, 1);
    let kit = crate::testkit::kit_fleet(&state, 1);
    if let Some(f) = state.entities.get_mut::<Fleet>(kit.sim_id()) {
        f.rules_of_engagement = RulesOfEngagement::HoldFire;
    }
    send(&mut state, 1, order(kit, FleetOrder::Launch));
    // The kit's 3 VX launch; its MULEs are a docked fleet of their own (D-286).
    assert_eq!(state.sensors_detection_stats().sensors, 4 + 3);
    // An outpost has no core yet (D-176).
    let free = state.galaxy().system(system).unwrap().sites[1];
    let outpost = state.insert_holding(free, CommanderId(1), false).unwrap();
    assert_eq!(state.sensors_detection_stats().sensors, 4 + 3);
    // But it is a target: another observer near it sees it.
    let near: Vec2 = state.galaxy().site(free).unwrap().position.into();
    place(
        &mut state,
        commander(2),
        system,
        near + Vec2::new(5_000.0, 0.0),
        &[VX],
    );
    sense(&mut state);
    assert!(picture(&state, commander(2), system).has_holding(outpost));
    // In lane transit: neither a sensor nor a target (doc 16 §4).
    let neighbour = *state.galaxy().neighbours(system).first().unwrap();
    send(
        &mut state,
        1,
        order(kit, FleetOrder::Jump { system: neighbour }),
    );
    for _ in 0..20_000 {
        if matches!(state.fleet(kit).unwrap().place, Place::InTransit { .. }) {
            break;
        }
        idle(&mut state, 1);
    }
    assert!(matches!(
        state.fleet(kit).unwrap().place,
        Place::InTransit { .. }
    ));
    let stats = state.sensors_detection_stats();
    assert_eq!(stats.sensors, 4 + 1);
    sense(&mut state);
    assert!(!picture(&state, commander(2), system).has_fleet(kit));
}

#[test]
fn passes_run_on_every_fifth_tick_for_every_observer() {
    let (mut state, system, base) = arena();
    place(&mut state, commander(2), system, base, &[VX]);
    let target = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(5_000.0, 0.0),
        &[VX],
    );
    // Placed at tick 0 after its pass: seen from the pass at tick 5 (D-173).
    let outputs = idle(&mut state, 4);
    assert!(events(&outputs).is_empty());
    assert!(!picture(&state, commander(2), system).has_fleet(target));
    let out = idle(&mut state, 1).remove(0);
    assert_eq!(out.tick, Tick::new(5));
    assert_eq!(state.last_sensors_pass(), Tick::new(5));
    assert!(picture(&state, commander(2), system).has_fleet(target));
    // Both see each other on the same pass.
    assert!(
        out.visibility_events
            .contains(&VisibilityEvent::FleetSighted {
                observer: commander(2),
                fleet: target
            })
    );
    assert!(out.visibility_events.iter().any(
        |e| matches!(e, VisibilityEvent::FleetSighted { observer, .. } if *observer == commander(3))
    ));
    idle(&mut state, 9);
    assert_eq!(state.last_sensors_pass(), Tick::new(10));
}

/// What [`leave_view`] did.
struct Left {
    /// The pass that lost the fleet.
    at: Tick,
    /// Commander 3's fleet.
    runner: FleetId,
    /// Its anchor at the end of the tick before.
    last: Motion,
    /// Commander 2's observer.
    eye: FleetId,
}

/// Commander 3's fleet moving away from commander 2's stationary VX until it leaves view.
fn leave_view(state: &mut UniverseState, system: SystemId, base: Vec2) -> Left {
    let eye = place(state, commander(2), system, base, &[VX]);
    let runner = place(
        state,
        commander(3),
        system,
        base + Vec2::new(6_000.0, 0.0),
        &[VX, VX],
    );
    sense(state);
    assert!(picture(state, commander(2), system).has_fleet(runner));
    let out = send(
        state,
        3,
        order(
            runner,
            FleetOrder::Move {
                to: at(system, base + Vec2::new(20_000.0, 0.0)),
            },
        ),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    let mut before = state.entities.get::<Motion>(runner.sim_id()).unwrap();
    for _ in 0..2_000 {
        let out = idle(state, 1).remove(0);
        let lost = VisibilityEvent::FleetLost {
            observer: commander(2),
            fleet: runner,
        };
        if out.visibility_events.contains(&lost) {
            return Left {
                at: out.tick,
                runner,
                last: before,
                eye,
            };
        }
        before = state.entities.get::<Motion>(runner.sim_id()).unwrap();
    }
    panic!("the fleet never left view");
}

#[test]
fn a_fleet_that_leaves_view_leaves_an_entry_with_what_was_last_seen() {
    let (mut state, system, base) = arena();
    let Left {
        at: lost_at,
        runner,
        last,
        ..
    } = leave_view(&mut state, system, base);
    let view = state.view(commander(2));
    assert!(view.sensed_fleet(runner).is_none());
    let entry = view.last_known_fleet(runner).expect("remembered");
    assert_eq!(entry.owner, commander(3));
    assert_eq!(entry.system, system);
    assert_eq!(entry.position, last.position, "where it was last seen");
    assert_eq!(entry.velocity, last.velocity);
    assert_eq!(entry.heading_rad.to_bits(), last.heading_rad.to_bits());
    assert_eq!(entry.seen_at, Tick::new(lost_at.get() - 1));
    assert_eq!(entry.classes.len(), 1);
    assert_eq!((entry.classes[0].class, entry.classes[0].count), (VX, 2));
    assert_eq!(entry.classes[0].hull_permille, 1_000);
    // It was seen at the edge of range, which is still in view: not a ghost yet (D-178).
    idle(&mut state, 50);
    assert!(state.view(commander(2)).last_known_fleet(runner).is_some());
    // The memory survives a snapshot round trip.
    let restored = decode(&encode(&state).unwrap()).unwrap();
    assert_eq!(
        restored.view(commander(2)).last_known_fleet(runner),
        state.view(commander(2)).last_known_fleet(runner)
    );
    // Sensed again, it replaces its entry.
    teleport(&mut state, runner, base + Vec2::new(3_000.0, 0.0));
    sense(&mut state);
    let view = state.view(commander(2));
    assert!(view.sensed_fleet(runner).is_some());
    assert!(view.last_known_fleet(runner).is_none());
}

#[test]
fn a_ghost_is_cleared_when_its_spot_is_back_in_view_without_the_fleet() {
    let (mut state, system, base) = arena();
    let Left { runner, eye, .. } = leave_view(&mut state, system, base);
    let spot: Vec2 = state
        .view(commander(2))
        .last_known_fleet(runner)
        .unwrap()
        .position
        .into();
    // The runner goes far away; the observer turns away, then looks at the spot again.
    teleport(&mut state, runner, base + Vec2::new(60_000.0, 0.0));
    teleport(&mut state, eye, base - Vec2::new(30_000.0, 0.0));
    sense(&mut state);
    assert!(state.view(commander(2)).last_known_fleet(runner).is_some());
    teleport(&mut state, eye, spot + Vec2::new(0.0, 1_000.0));
    sense(&mut state);
    assert!(state.view(commander(2)).last_known_fleet(runner).is_none());
}

#[test]
fn fleet_entries_expire_after_the_memory_time() {
    // Two minutes instead of 24 h, so the test runs in a few thousand ticks.
    let mut rules = Rules::embedded();
    rules.sensors.fleet_memory_s = 120;
    let mut state = generate_stage1_with_rules(SeasonConfig::new(5, 0), COMMANDERS, rules).unwrap();
    let (_, system) = home(&state, 1);
    let site = site_position(&state, system, 0);
    let base = Vec2::new(site.x.round() + 8_000.0, site.y.round());
    let Left { runner, .. } = leave_view(&mut state, system, base);
    let seen = state
        .view(commander(2))
        .last_known_fleet(runner)
        .unwrap()
        .seen_at;
    let expiry = seen.after(Ticks::from_secs(120));
    while state.tick() < expiry {
        idle(&mut state, 1);
        if state.tick() < expiry {
            assert!(state.view(commander(2)).last_known_fleet(runner).is_some());
        }
    }
    // The sweep runs once a minute per commander.
    idle(&mut state, 600);
    assert!(state.view(commander(2)).last_known_fleet(runner).is_none());
}

/// The gate of `from` whose lane leads to `to`.
fn gate_towards(state: &UniverseState, from: SystemId, to: SystemId) -> (GateId, GateId) {
    let galaxy = state.galaxy();
    for &gate in &galaxy.system(from).unwrap().gates {
        for &lane in &galaxy.gate(gate).unwrap().lanes {
            let far = galaxy.lane(lane).unwrap().other_end(gate).unwrap();
            if galaxy.gate(far).unwrap().system == to {
                return (gate, far);
            }
        }
    }
    panic!("no lane between the systems");
}

/// A jump from a standard system to the hub (no holdings, so no cores, in either; doc 10
/// §4.4): commander 2's VX 1 km from the gate; commanders 3 and 4 watch the gate from 4 km;
/// commander 1's VX 13 km in cannot see a Light ship (9 km); commander 3 also watches the far
/// gate.
struct Departure {
    state: UniverseState,
    system: SystemId,
    neighbour: SystemId,
    gate: GateId,
    jumper: FleetId,
}

fn departure() -> Departure {
    let mut state = stage1(5);
    let kind = |state: &UniverseState, s: SystemId| state.galaxy().system(s).unwrap().kind;
    let system = state
        .galaxy()
        .systems()
        .map(|(id, _)| id)
        .find(|&s| kind(&state, s) == SystemKind::Standard)
        .unwrap();
    let neighbour = state
        .galaxy()
        .neighbours(system)
        .into_iter()
        .find(|&s| kind(&state, s) == SystemKind::Hub)
        .unwrap();
    let (gate, far) = gate_towards(&state, system, neighbour);
    let gate_at: Vec2 = state.galaxy().gate(gate).unwrap().position.into();
    let far_at: Vec2 = state.galaxy().gate(far).unwrap().position.into();
    let inward = (Vec2::ZERO - gate_at).normalized().unwrap();
    let far_inward = (Vec2::ZERO - far_at).normalized().unwrap();
    let side = inward.rotated(std::f64::consts::FRAC_PI_2);
    let jumper = place(
        &mut state,
        commander(2),
        system,
        gate_at + inward * 1_000.0,
        &[VX],
    );
    place(
        &mut state,
        commander(3),
        system,
        gate_at + side * 4_000.0,
        &[VX],
    );
    place(
        &mut state,
        commander(4),
        system,
        gate_at - side * 4_000.0,
        &[VX],
    );
    place(
        &mut state,
        commander(1),
        system,
        gate_at + inward * 13_000.0,
        &[VX],
    );
    place(
        &mut state,
        commander(3),
        neighbour,
        far_at + far_inward * 3_000.0,
        &[VX],
    );
    sense(&mut state);
    Departure {
        state,
        system,
        neighbour,
        gate,
        jumper,
    }
}

/// Runs until the jumper departs; checks who saw it and returns the tick and arrival tick.
fn depart(state: &mut UniverseState, jumper: FleetId, neighbour: SystemId) -> (Tick, Tick) {
    let mut departed = None;
    for _ in 0..2_000 {
        let out = idle(state, 1).remove(0);
        if let Some(&FleetEvent::Departed { arrive, .. }) = out
            .fleet_events
            .iter()
            .find(|e| matches!(e, FleetEvent::Departed { fleet, .. } if *fleet == jumper))
        {
            let seen: Vec<Owner> = out
                .visibility_events
                .iter()
                .filter_map(|e| match *e {
                    VisibilityEvent::DepartureObserved {
                        observer,
                        fleet,
                        to,
                        arrives,
                    } => {
                        assert_eq!((fleet, to, arrives), (jumper, neighbour, arrive));
                        Some(observer)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(seen, vec![commander(3), commander(4)]);
            departed = Some((out.tick, arrive));
            break;
        }
    }
    departed.expect("the fleet departed")
}

#[test]
fn a_departure_is_seen_exactly_by_those_who_saw_a_member_and_expires_as_agreed() {
    let Departure {
        mut state,
        system,
        neighbour,
        gate,
        jumper,
    } = departure();
    let out = send(
        &mut state,
        2,
        order(jumper, FleetOrder::Jump { system: neighbour }),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    let (departed, arrive) = depart(&mut state, jumper, neighbour);
    for n in [3, 4] {
        let view = state.view(commander(n));
        let records: Vec<_> = view.lane_traffic().collect();
        assert_eq!(records.len(), 1);
        let r = records[0];
        assert_eq!(
            (r.fleet, r.owner, r.from, r.gate, r.to),
            (jumper, commander(2), system, gate, neighbour)
        );
        assert_eq!((r.departed, r.arrives), (departed, arrive));
        assert_eq!((r.classes[0].class, r.classes[0].count), (VX, 1));
    }
    assert_eq!(state.view(commander(1)).lane_traffic().count(), 0);
    // A departed fleet leaves lane traffic, not a ghost at the gate (Proposed).
    assert!(state.view(commander(3)).last_known_fleet(jumper).is_none());
    // Commander 3 sees it arrive and drops the record at the next pass; commander 4 keeps
    // it until 10 min after the arrival tick (D-179).
    while state.tick() < arrive {
        idle(&mut state, 1);
    }
    idle(&mut state, 5);
    assert_eq!(state.view(commander(3)).lane_traffic().count(), 0);
    let expiry = arrive.after(Ticks::from_minutes(10));
    while state.tick() < expiry {
        assert_eq!(state.view(commander(4)).lane_traffic().count(), 1);
        idle(&mut state, 1);
    }
    idle(&mut state, 5);
    assert_eq!(state.view(commander(4)).lane_traffic().count(), 0);
}

#[test]
fn npc_observers_see_and_keep_lane_traffic_but_no_memory() {
    let (mut state, system, base) = arena();
    let npc = Owner::Npc(FactionId(1));
    place(&mut state, npc, system, base, &[RAIDER]);
    let target = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(5_000.0, 0.0),
        &[VX],
    );
    sense(&mut state);
    assert!(picture(&state, npc, system).has_fleet(target));
    assert!(state.view(npc).sensed_fleet(target).is_some());
    assert!(state.view(npc).economy().is_none());
    teleport(&mut state, target, base + Vec2::new(30_000.0, 0.0));
    sense(&mut state);
    let view = state.view(npc);
    assert!(view.sensed_fleet(target).is_none());
    assert!(view.last_known_fleet(target).is_none(), "D-181");
    assert!(
        state
            .sensors
            .observers
            .get(&npc)
            .is_none_or(|o| o.memory.is_none())
    );
}

#[test]
fn the_view_shows_own_assets_in_full_and_nothing_private_of_others() {
    let (mut state, system, base) = arena();
    let mine = place(&mut state, commander(2), system, base, &[VX]);
    let theirs = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(3_000.0, 0.0),
        &[VX],
    );
    sense(&mut state);
    let view = state.view(commander(2));
    assert_eq!(view.observer(), commander(2));
    assert!(view.own_fleets().iter().any(|f| f.id == mine));
    assert!(view.own_fleet(theirs).is_none());
    assert!(view.own_ship(ships(&state, theirs)[0]).is_none());
    assert!(view.own_cargo(ships(&state, theirs)[0]).is_none());
    assert_eq!(view.own_fleets_in(system).len(), 1);
    // Commander 2's own mother station and economy; not commander 1's.
    let (own_mother, _) = home(&state, 2);
    let (other_mother, _) = home(&state, 1);
    assert!(view.holding_economy(own_mother).is_some());
    assert!(view.defence(own_mother).is_some());
    assert!(view.holding_economy(other_mother).is_none());
    assert!(view.defence(other_mother).is_none());
    assert!(view.siege(own_mother).is_some());
    assert!(view.siege(other_mother).is_none());
    assert!(view.own_station_jumps().is_empty());
    assert_eq!(view.economy().map(|e| e.holdings.len()), Some(1));
    assert_eq!(view.own_holdings().len(), 1);
    // Public: every holding, control, the roster and windows, protections and the season.
    assert_eq!(view.holdings().count(), 4);
    assert_eq!(view.commanders().len(), STAGE1_COMMANDERS);
    assert!(view.exposure_window(CommanderId(3)).is_some());
    assert_eq!(
        view.breachable_from(other_mother),
        state.breachable_from(other_mother)
    );
    assert_eq!(view.standings(), state.standings());
    assert_eq!(view.standings().len(), STAGE1_COMMANDERS);
    assert_eq!(view.final_tick(), state.final_tick());
    assert_eq!(view.early_win_threshold(), state.early_win_threshold());
    assert!(view.near_early_win().is_empty() && view.season_result().is_none());
    // Sensed: the hostile, with no orders or other private details in its type.
    let sensed = view.sensed_fleet(theirs).unwrap();
    assert_eq!(sensed.owner, commander(3));
    assert_eq!(sensed.ships.len(), 1);
    assert!(
        view.sensed_fleet(mine).is_none(),
        "own fleets are not sensed"
    );
    assert!(view.is_sensed(theirs.sim_id()));
    assert_eq!(
        view.sensed_ship_ids_in(system),
        ships(&state, theirs).as_slice()
    );
    assert!(view.sensed_systems().any(|s| s == system));
}

#[test]
fn snapshot_and_restore_between_passes_with_memory_and_traffic_change_nothing() {
    let (mut state, system, base) = arena();
    leave_view(&mut state, system, base);
    idle(&mut state, 2);
    assert!(!super::is_pass_tick(state.tick()), "between two passes");
    let mut restored = decode(&encode(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
    for _ in 0..200 {
        let a = crate::step(&mut state, &crate::TickInputs::default());
        let b = crate::step(&mut restored, &crate::TickInputs::default());
        assert_eq!(a.visibility_events, b.visibility_events);
    }
    assert_eq!(state_hash(&state).unwrap(), state_hash(&restored).unwrap());
}

#[test]
fn the_range_rule_rejects_a_weapon_beyond_its_own_sensor() {
    let mut rules = Rules::embedded();
    // A VX's cannon reaches 1.5 km; a 2.5 km sensor sees a Light target at 1.5 km: not below.
    rules.sensors.ranges_m.insert("VX".to_owned(), 2_500);
    assert!(matches!(
        rules.check(),
        Err(crate::RulesError::Invalid { field, .. }) if field.contains("Vx")
    ));
    let mut rules = Rules::embedded();
    rules.sensors.holding_core_range_m = 4_000;
    assert!(matches!(
        rules.check(),
        Err(crate::RulesError::Invalid { field, .. }) if field == "sensors.holding_core_range_m"
    ));
    let mut rules = Rules::embedded();
    rules.ship_classes.get_mut(&LANCER).unwrap().sensor = "RADAR".to_owned();
    assert!(rules.check().is_err());
    let mut rules = Rules::embedded();
    rules
        .sensors
        .signature_permille
        .remove(&crate::rules::combat::SizeClass::Heavy);
    assert!(rules.check().is_err());
    // The embedded values (D-072, D-073).
    let rules = Rules::embedded();
    assert_eq!(rules.sensors.holding_core_range_m, 40_000);
    assert_eq!(
        rules.sensors.range_m(&rules.class(WARDEN).sensor),
        Some(25_000)
    );
    assert_eq!(
        rules
            .sensors
            .range_m(&rules.class(ShipClass::HeavyCruiser).sensor),
        Some(30_000)
    );
    assert_eq!(rules.sensors.smallest_signature(), 600);
    assert_eq!(rules.sensors.largest_signature(), 1_500);
}
