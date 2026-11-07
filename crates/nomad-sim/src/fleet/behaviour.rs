//! Behaviour of ships and fleets: the start kit, fleet management, movement, formation,
//! cruise mode, obstacles, separation and in-system orders (S3 plan §7 "Done when").

use nomad_types::{Command, FleetOrder, HoldingOrder};

use crate::fleet::station::Obstacle;
use crate::fleet::{FleetEvent, FleetId, MotionMode, Place, ShipClass, formation_offsets};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::state::UniverseState;
use crate::testkit::{
    at, commander, home, idle, kit_fleet, order, send, ship_positions, site_position, spawn_fleet,
    stage1, then,
};

const VX: ShipClass = ShipClass::Vx;
const MULE: ShipClass = ShipClass::Mule;
const WARDEN: ShipClass = ShipClass::Warden;

/// Every ship of every fleet in space, checked against every footprint of its system.
fn assert_outside_footprints(state: &UniverseState) {
    for ship in state.ship_ids() {
        let view = state.ship(ship).unwrap();
        let Place::InSystem(system) = view.place else {
            continue;
        };
        let p = Vec2::from(view.position);
        for Obstacle { centre, radius_m } in state.obstacles(system) {
            assert!(
                (p - centre).length() >= radius_m - 1e-6,
                "{ship:?} is {:.1} m inside a footprint",
                radius_m - (p - centre).length()
            );
        }
    }
}

/// A point 3 km east of site 1 of commander 1's home system: inside an ops area, away from
/// the mother station (site 0).
fn quiet_spot(state: &UniverseState) -> (SystemId, Vec2) {
    let (_, system) = home(state, 1);
    (
        system,
        site_position(state, system, 1) + Vec2::new(3_000.0, 0.0),
    )
}

/// A point at the system edge, far from every site: outside every ops area.
fn open_space(state: &UniverseState, system: SystemId) -> Vec2 {
    let gate = state.galaxy().system(system).unwrap().gates[0];
    Vec2::from(state.galaxy().gate(gate).unwrap().position)
}

#[test]
fn every_mother_station_has_its_start_kit_docked() {
    let state = stage1(3);
    for n in 1..=4 {
        let (holding, _) = home(&state, n);
        let fleet = state.fleet(kit_fleet(&state, n)).unwrap();
        assert_eq!(fleet.place, Place::Docked(holding));
        let classes: Vec<ShipClass> = fleet
            .ships
            .iter()
            .map(|&s| state.ship(s).unwrap().class)
            .collect();
        assert_eq!(classes, [VX, VX, VX, MULE, MULE]);
        // D-102: mother-station core 24, plus 6 for the start kit's dry dock.
        assert_eq!(state.docking_capacity(holding), 30);
        assert_eq!(state.docked_ships(holding), 5);
    }
    assert_eq!(state.fleet_ids().len(), 4);
    assert_eq!(state.ship_ids().len(), 20);
}

#[test]
fn launch_ready_puts_docked_fleets_at_the_launch_point_in_formation() {
    let mut state = stage1(3);
    let (holding, system) = home(&state, 1);
    let fleet = kit_fleet(&state, 1);
    let out = send(
        &mut state,
        1,
        Command::Holding {
            holding,
            order: HoldingOrder::LaunchReady,
        },
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    assert!(
        out.fleet_events
            .contains(&FleetEvent::Launched { fleet, holding })
    );
    let view = state.fleet(fleet).unwrap();
    assert_eq!(view.place, Place::InSystem(system));
    let launch = Vec2::from(state.launch_point(holding).unwrap());
    assert!((Vec2::from(view.position) - launch).length() < 1e-6);
    assert_eq!(state.docked_ships(holding), 0);
    assert_outside_footprints(&state);
    // Nothing left to launch.
    let out = send(
        &mut state,
        1,
        Command::Holding {
            holding,
            order: HoldingOrder::LaunchReady,
        },
    );
    assert!(out.commands[0].outcome.is_err());
}

#[test]
fn split_and_merge_keep_every_ship_in_exactly_one_fleet() {
    let mut state = stage1(3);
    let kit = kit_fleet(&state, 1);
    let ships = state.fleet(kit).unwrap().ships;
    let out = send(
        &mut state,
        1,
        Command::CreateFleet {
            ships: ships[3..].to_vec(),
        },
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    let mules = *state
        .fleet_ids()
        .iter()
        .max()
        .expect("the new fleet has the newest identity");
    assert_eq!(state.fleet(mules).unwrap().ships, ships[3..].to_vec());
    assert_eq!(state.fleet(kit).unwrap().ships, ships[..3].to_vec());
    for &ship in &ships[3..] {
        assert_eq!(state.ship(ship).unwrap().fleet, mules);
    }
    // Both are docked at the mother station still.
    let (holding, _) = home(&state, 1);
    assert_eq!(state.fleet(mules).unwrap().place, Place::Docked(holding));

    let out = send(
        &mut state,
        1,
        Command::MergeFleets {
            fleet: mules,
            into: kit,
        },
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    assert!(state.fleet(mules).is_none());
    assert_eq!(state.fleet(kit).unwrap().ships, ships);

    // Taking every ship out of a fleet dissolves it.
    send(
        &mut state,
        1,
        Command::CreateFleet {
            ships: ships.clone(),
        },
    );
    assert!(state.fleet(kit).is_none());
    assert_eq!(state.fleet_ids().len(), 4);
}

/// Runs `fleet` to `target` and checks the Done-when movement criteria on the way: no ship
/// overshoots its final slot along the leg by more than 1% of the leg, and the formation
/// holds once it has settled.
fn run_leg(state: &mut UniverseState, fleet: FleetId, system: SystemId, target: Vec2) -> u64 {
    let start = Vec2::from(state.fleet(fleet).unwrap().position);
    let leg = target - start;
    let direction = leg.normalized().unwrap();
    let out = send(
        state,
        1,
        order(
            fleet,
            FleetOrder::Move {
                to: at(system, target),
            },
        ),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    let ships = state.fleet(fleet).unwrap().ships;
    let offsets = formation_offsets(ships.len(), 150.0);
    let final_slots: Vec<Vec2> = offsets
        .iter()
        .map(|o| target + o.rotated(direction.angle()))
        .collect();
    let mut worst_overshoot: f64 = 0.0;
    let mut ticks = 1;
    loop {
        let out = idle(state, 1).remove(0);
        ticks += 1;
        let anchor = Vec2::from(state.fleet(fleet).unwrap().position);
        worst_overshoot = worst_overshoot.max((anchor - target).dot(direction));
        for (p, slot) in ship_positions(state, fleet).iter().zip(&final_slots) {
            worst_overshoot = worst_overshoot.max((*p - *slot).dot(direction));
        }
        assert_outside_footprints(state);
        if out
            .fleet_events
            .contains(&FleetEvent::OrderCompleted { fleet })
        {
            break;
        }
        assert!(ticks < 100_000, "the fleet never arrived");
    }
    assert!(
        worst_overshoot <= 0.01 * leg.length(),
        "overshoot {worst_overshoot:.2} m on a {:.0} m leg",
        leg.length()
    );
    // Settle, then the ships hold their slots: the formation's spacing.
    idle(state, 300);
    for (p, slot) in ship_positions(state, fleet).iter().zip(&final_slots) {
        assert!(
            (*p - *slot).length() < 1.0,
            "{p:?} is off its slot {slot:?}"
        );
    }
    ticks
}

#[test]
fn ships_reach_their_target_without_overshoot_and_keep_formation() {
    let mut state = stage1(3);
    let (system, start) = quiet_spot(&state);
    let fleet = spawn_fleet(
        &mut state,
        commander(1),
        system,
        start,
        &[VX, VX, WARDEN, MULE, MULE],
    );
    // Tactical legs inside an ops area: east, then north (the formation turns), then back.
    run_leg(&mut state, fleet, system, start + Vec2::new(5_000.0, 0.0));
    run_leg(
        &mut state,
        fleet,
        system,
        start + Vec2::new(5_000.0, 4_000.0),
    );
    let ticks = run_leg(&mut state, fleet, system, start);
    // 6.4 km at the WARDEN's 110 m/s, plus acceleration and braking at 15 m/s².
    assert!(ticks > 580 && ticks < 900, "{ticks} ticks");
    // Spacing between neighbours in a row is exactly the formation spacing.
    let p = ship_positions(&state, fleet);
    assert!(((p[0] - p[1]).length() - 150.0).abs() < 1.0);
    assert!(((p[3] - p[4]).length() - 150.0).abs() < 1.0);
}

#[test]
fn a_fleet_cruises_at_its_slowest_members_cruise_speed_outside_ops_areas() {
    for (classes, cruise) in [(&[VX, VX][..], 900.0), (&[VX, VX, MULE][..], 600.0)] {
        let mut state = stage1(3);
        let (_, system) = home(&state, 1);
        let start = open_space(&state, system);
        let fleet = spawn_fleet(&mut state, commander(1), system, start, classes);
        // Along the system edge, 60 km from the gate: far from every ops area.
        let tangent = Vec2::new(-start.y, start.x).normalized().unwrap();
        let target = start + tangent * 60_000.0;
        send(
            &mut state,
            1,
            order(
                fleet,
                FleetOrder::Move {
                    to: at(system, target),
                },
            ),
        );
        let mut fastest: f64 = 0.0;
        for tick in 0..1_500 {
            idle(&mut state, 1);
            let view = state.fleet(fleet).unwrap();
            assert_eq!(view.mode, MotionMode::Cruise);
            fastest = fastest.max(view.velocity.length());
            if tick == 600 {
                // Mid-leg, in steady cruise, every ship moves at the fleet's speed.
                for &ship in &view.ships {
                    let speed = state.ship(ship).unwrap().velocity.length();
                    assert!((speed - cruise).abs() < 1.0, "{speed}");
                }
            }
        }
        assert!(
            (fastest - cruise).abs() < 1e-6,
            "{fastest} instead of {cruise}"
        );
        let end = Vec2::from(state.fleet(fleet).unwrap().position);
        assert!((end - target).length() < 0.5);
    }
}

#[test]
fn cruise_mode_switches_off_inside_ops_areas_and_when_engaged() {
    let mut state = stage1(3);
    let (_, system) = home(&state, 1);
    let start = open_space(&state, system);
    let sites: Vec<Vec2> = (0..7).map(|i| site_position(&state, system, i)).collect();
    let nearest = sites
        .iter()
        .copied()
        .min_by(|a, b| (*a - start).length().total_cmp(&(*b - start).length()))
        .unwrap();
    let fleet = spawn_fleet(&mut state, commander(1), system, start, &[VX, VX]);
    assert_eq!(state.fleet(fleet).unwrap().mode, MotionMode::Cruise);
    // The "taking fire" and "hostile detected" hook (S5, S7).
    state.set_fleet_engaged(fleet, true);
    assert_eq!(state.fleet(fleet).unwrap().mode, MotionMode::Tactical);
    state.set_fleet_engaged(fleet, false);

    let target = nearest + (start - nearest).normalized().unwrap() * 2_000.0;
    send(
        &mut state,
        1,
        order(
            fleet,
            FleetOrder::Move {
                to: at(system, target),
            },
        ),
    );
    let (mut cruised, mut entered) = (false, false);
    for _ in 0..3_000 {
        idle(&mut state, 1);
        let view = state.fleet(fleet).unwrap();
        let here = Vec2::from(view.position);
        let inside = sites.iter().any(|s| (here - *s).length() < 20_000.0);
        let expected = if inside {
            MotionMode::Tactical
        } else {
            MotionMode::Cruise
        };
        assert_eq!(view.mode, expected);
        cruised |= !inside && view.velocity.length() > 280.0;
        entered |= inside;
    }
    assert!(cruised && entered);
    assert!((Vec2::from(state.fleet(fleet).unwrap().position) - target).length() < 0.5);
}

#[test]
fn no_ship_enters_an_obstacle_footprint() {
    let mut state = stage1(3);
    let (holding, system) = home(&state, 1);
    let kit = kit_fleet(&state, 1);
    send(&mut state, 1, order(kit, FleetOrder::Launch));
    let core = Vec2::from(
        state
            .galaxy()
            .site(state.holding(holding).unwrap().site)
            .unwrap()
            .position,
    );
    let launch = Vec2::from(state.launch_point(holding).unwrap());
    // Straight across the station to the far side, and back round it.
    let across = core + (core - launch) * 1.5;
    let side = core + (launch - core).rotated(std::f64::consts::FRAC_PI_2) * 1.5;
    let out = send(
        &mut state,
        1,
        order(
            kit,
            FleetOrder::Patrol {
                route: vec![at(system, across), at(system, side), at(system, launch)],
            },
        ),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    // A second fleet flies through the station from the east.
    let east = spawn_fleet(
        &mut state,
        commander(1),
        system,
        core + Vec2::new(4_000.0, 300.0),
        &[WARDEN, VX, VX],
    );
    send(
        &mut state,
        1,
        order(
            east,
            FleetOrder::Move {
                to: at(system, core - Vec2::new(4_000.0, 0.0)),
            },
        ),
    );
    let obstacles = state.obstacles(system);
    assert_eq!(obstacles.len(), 6);
    let mut closest: f64 = f64::INFINITY;
    for _ in 0..1_500 {
        idle(&mut state, 1);
        assert_outside_footprints(&state);
        for p in ship_positions(&state, east) {
            closest = closest.min((p - core).length());
        }
    }
    // The east fleet went round the core spindle, not far around, and got through.
    assert!(closest < 2_000.0, "{closest}");
    for p in ship_positions(&state, east) {
        assert!(
            (p - (core - Vec2::new(4_000.0, 0.0))).length() < 400.0,
            "{p:?}"
        );
    }
}

#[test]
fn fleets_sent_to_one_point_keep_a_soft_separation() {
    let mut state = stage1(3);
    let (system, start) = quiet_spot(&state);
    let a = spawn_fleet(&mut state, commander(1), system, start, &[VX, VX, VX]);
    let b = spawn_fleet(
        &mut state,
        commander(1),
        system,
        start + Vec2::new(0.0, 3_000.0),
        &[VX, VX, VX],
    );
    let target = start + Vec2::new(2_000.0, 1_500.0);
    for fleet in [a, b] {
        send(
            &mut state,
            1,
            order(
                fleet,
                FleetOrder::Move {
                    to: at(system, target),
                },
            ),
        );
    }
    idle(&mut state, 600);
    let ships: Vec<Vec2> = [a, b]
        .iter()
        .flat_map(|&f| ship_positions(&state, f))
        .collect();
    for (i, p) in ships.iter().enumerate() {
        for q in &ships[i + 1..] {
            // Two thirds of the 120 m separation for ships that share a slot.
            assert!((*p - *q).length() > 75.0, "{:.1} m", (*p - *q).length());
        }
    }
}

/// Sets `fleets` to hold fire. The movement tests (S3) use it where a target must survive to
/// be followed; firing is tested in `combat::tests` (S5a).
fn hold_fire(state: &mut UniverseState, fleets: &[FleetId]) {
    for fleet in fleets {
        if let Some(f) = state
            .entities
            .get_mut::<crate::fleet::Fleet>(fleet.sim_id())
        {
            f.rules_of_engagement = nomad_types::RulesOfEngagement::HoldFire;
        }
    }
}

#[test]
fn patrol_guard_station_keep_and_attack() {
    let mut state = stage1(3);
    let (system, start) = quiet_spot(&state);
    // Patrol two points: the fleet goes back and forth.
    let patrol = spawn_fleet(&mut state, commander(1), system, start, &[VX]);
    let (p1, p2) = (start + Vec2::new(2_000.0, 0.0), start);
    send(
        &mut state,
        1,
        order(
            patrol,
            FleetOrder::Patrol {
                route: vec![at(system, p1), at(system, p2)],
            },
        ),
    );
    // A hostile fleet that moves away, guarded, kept station on and attacked.
    let hostile = spawn_fleet(
        &mut state,
        commander(2),
        system,
        start + Vec2::new(0.0, 8_000.0),
        &[MULE],
    );
    let guard = spawn_fleet(&mut state, commander(1), system, start, &[VX, VX]);
    let keeper = spawn_fleet(
        &mut state,
        commander(1),
        system,
        start + Vec2::new(0.0, 7_000.0),
        &[VX],
    );
    let attacker = spawn_fleet(
        &mut state,
        commander(1),
        system,
        start + Vec2::new(-3_000.0, 0.0),
        &[VX, WARDEN],
    );
    hold_fire(&mut state, &[patrol, guard, keeper, attacker]);
    for (fleet, o) in [
        (
            guard,
            FleetOrder::Guard {
                object: hostile.sim_id(),
            },
        ),
        (
            keeper,
            FleetOrder::StationKeep {
                relative_to: Some(hostile.sim_id()),
            },
        ),
        (
            attacker,
            FleetOrder::Attack {
                target: hostile.sim_id(),
            },
        ),
    ] {
        let out = send(&mut state, 1, order(fleet, o));
        assert_eq!(out.commands[0].outcome, Ok(()));
    }
    let away = start + Vec2::new(6_000.0, 12_000.0);
    send(
        &mut state,
        2,
        order(
            hostile,
            FleetOrder::Move {
                to: at(system, away),
            },
        ),
    );

    let mut visits = Vec::new();
    for _ in 0..2_400 {
        idle(&mut state, 1);
        let here = Vec2::from(state.fleet(patrol).unwrap().position);
        for (i, p) in [p1, p2].iter().enumerate() {
            if (here - *p).length() < 1.0 && visits.last() != Some(&i) {
                visits.push(i);
            }
        }
    }
    assert_eq!(&visits[..4], &[0, 1, 0, 1], "{visits:?}");
    let target = Vec2::from(state.fleet(hostile).unwrap().position);
    assert!((target - away).length() < 1.0);
    let distance = |f| (Vec2::from(state.fleet(f).unwrap().position) - target).length();
    assert!(distance(guard) <= 3_000.0, "guard {}", distance(guard));
    // Station-keeping kept the starting offset (1 km south of the hostile).
    let keeper_at = Vec2::from(state.fleet(keeper).unwrap().position);
    assert!((keeper_at - (target + Vec2::new(0.0, -1_000.0))).length() < 1.0);
    // The attacker holds within its shortest weapon range, the VX's 1.5 km.
    assert!(
        distance(attacker) <= 1_500.0,
        "attacker {}",
        distance(attacker)
    );
    assert!(distance(attacker) >= 1_000.0);
}

#[test]
fn dock_and_retreat_go_to_an_owned_holding_with_room() {
    let mut state = stage1(3);
    let (holding, system) = home(&state, 1);
    let kit = kit_fleet(&state, 1);
    send(&mut state, 1, order(kit, FleetOrder::Launch));
    let outside = site_position(&state, system, 0) + Vec2::new(0.0, 6_000.0);
    let out = send(
        &mut state,
        1,
        then(
            kit,
            FleetOrder::Move {
                to: at(system, outside),
            },
        ),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    send(&mut state, 1, then(kit, FleetOrder::Dock { at: holding }));
    let mut docked = false;
    for _ in 0..3_000 {
        let out = idle(&mut state, 1).remove(0);
        docked |= out.fleet_events.contains(&FleetEvent::Docked {
            fleet: kit,
            holding,
        });
        assert_outside_footprints(&state);
    }
    assert!(docked);
    assert_eq!(state.fleet(kit).unwrap().place, Place::Docked(holding));

    // A fleet in space retreats to the nearest owned holding and docks.
    let (_, start) = quiet_spot(&state);
    let lost = spawn_fleet(&mut state, commander(1), system, start, &[VX]);
    let out = send(&mut state, 1, order(lost, FleetOrder::Retreat { to: None }));
    assert_eq!(out.commands[0].outcome, Ok(()));
    idle(&mut state, 4_000);
    assert_eq!(state.fleet(lost).unwrap().place, Place::Docked(holding));
    assert_eq!(state.docked_ships(holding), 6);
}
