//! Jumps: routing, the 5 s spool-up and its cancellation, lane transit, arrival scatter and
//! the 10 s jump shroud (D-042, D-055; S3 plan §4.4, §7 "Jump tests").

use nomad_types::FleetOrder;

use crate::command::locate;
use crate::fleet::route::{self, RouteEnd};
use crate::fleet::{FleetEvent, FleetId, Motion, Place, ShipClass, TravelStage};
use crate::galaxy::{GateId, SystemId};
use crate::geometry::Vec2;
use crate::state::UniverseState;
use crate::testkit::{at, commander, home, idle, kit_fleet, order, send, spawn_fleet, stage1};
use crate::time::{Tick, Ticks};

/// Runs until `fleet` has an event matching `wanted`, at most `limit` ticks; returns the
/// tick and the event.
fn run_until(
    state: &mut UniverseState,
    limit: u64,
    wanted: impl Fn(&FleetEvent) -> bool,
) -> (Tick, FleetEvent) {
    for _ in 0..limit {
        let out = idle(state, 1).remove(0);
        if let Some(event) = out.fleet_events.iter().find(|e| wanted(e)) {
            return (out.tick, *event);
        }
    }
    panic!("no such event within {limit} ticks");
}

/// The gate of `from` whose lane leads to `to`, and the lane's far gate.
fn gates_between(state: &UniverseState, from: SystemId, to: SystemId) -> (GateId, GateId) {
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

fn gate_position(state: &UniverseState, gate: GateId) -> Vec2 {
    state.galaxy().gate(gate).unwrap().position.into()
}

#[test]
fn a_jump_spools_up_for_5_s_transits_for_the_lane_time_and_arrives_shrouded_within_2_km() {
    let mut state = stage1(3);
    let (_, home_system) = home(&state, 1);
    let neighbour = *state.galaxy().neighbours(home_system).first().unwrap();
    let (departure, arrival) = gates_between(&state, home_system, neighbour);
    let kit = kit_fleet(&state, 1);
    send(&mut state, 1, order(kit, FleetOrder::Launch));
    let out = send(
        &mut state,
        1,
        order(kit, FleetOrder::Jump { system: neighbour }),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));

    let is_kit = |e: &FleetEvent| match *e {
        FleetEvent::SpoolUpStarted { fleet, .. }
        | FleetEvent::Departed { fleet, .. }
        | FleetEvent::Arrived { fleet, .. }
        | FleetEvent::SpoolUpCancelled { fleet } => fleet == kit,
        _ => false,
    };
    let (spooled, event) = run_until(&mut state, 10_000, is_kit);
    let FleetEvent::SpoolUpStarted { gate, until, .. } = event else {
        panic!("{event:?}");
    };
    assert_eq!(gate, departure);
    assert_eq!(until, spooled.after(Ticks::from_secs(5)));
    // Every ship is within 2 km of the gate, and the fleet spools up for exactly 5 s.
    for &ship in &state.fleet(kit).unwrap().ships {
        let p = Vec2::from(state.ship(ship).unwrap().position);
        assert!((p - gate_position(&state, departure)).length() <= 2_000.0);
    }
    let (departed, event) = run_until(&mut state, 100, is_kit);
    let FleetEvent::Departed { lane, arrive, .. } = event else {
        panic!("{event:?}");
    };
    assert_eq!(departed, until);
    let transit = state.galaxy().lane(lane).unwrap().transit;
    assert_eq!(arrive, departed.after(transit));

    // In transit the fleet is a timer: invisible and out of reach (D-042).
    assert!(matches!(
        state.fleet(kit).unwrap().place,
        Place::InTransit { to_gate, .. } if to_gate == arrival
    ));
    assert!(locate(&state, kit.sim_id()).is_none());
    let ships = state.fleet(kit).unwrap().ships;
    assert!(locate(&state, ships[0].sim_id()).is_none());

    let (arrived, event) = run_until(&mut state, transit.get() + 1, is_kit);
    assert_eq!(
        event,
        FleetEvent::Arrived {
            fleet: kit,
            system: neighbour,
            gate: arrival
        }
    );
    assert_eq!(arrived, arrive, "transit time equals the lane's");
    let view = state.fleet(kit).unwrap();
    assert_eq!(view.place, Place::InSystem(neighbour));
    let centre = gate_position(&state, arrival);
    let mut spread = Vec::new();
    for &ship in &ships {
        let ship = state.ship(ship).unwrap();
        let p = Vec2::from(ship.position);
        assert!((p - centre).length() <= 2_000.0, "{p:?}");
        spread.push(p);
        assert_eq!(ship.shroud_until, Some(arrived.after(Ticks::from_secs(10))));
    }
    // Scattered, not stacked.
    assert!((spread[0] - spread[1]).length() > 1.0);
    // The jump was a single hop, so the order is done.
    assert_eq!(view.order, None);

    // The shroud lasts 10 s: still there on the 99th tick after arrival, gone on the 100th.
    idle(&mut state, 98);
    assert_eq!(state.tick(), arrived.after(Ticks::new(98)));
    idle(&mut state, 1);
    assert!(state.ship(ships[0]).unwrap().shroud_until.is_some());
    idle(&mut state, 1);
    assert_eq!(state.tick(), arrived.after(Ticks::from_secs(10)));
    assert!(state.ship(ships[0]).unwrap().shroud_until.is_none());
}

#[test]
fn moving_away_cancels_the_spool_up() {
    let mut state = stage1(3);
    let (_, home_system) = home(&state, 1);
    let neighbour = *state.galaxy().neighbours(home_system).first().unwrap();
    let (departure, _) = gates_between(&state, home_system, neighbour);
    let gate = gate_position(&state, departure);
    let fleet = spawn_fleet(
        &mut state,
        commander(1),
        home_system,
        gate + Vec2::new(500.0, 0.0),
        &[ShipClass::Vx, ShipClass::Vx],
    );
    let spooling = |f: FleetId, state: &UniverseState| {
        matches!(
            state.fleet(f).unwrap().travel,
            Some(TravelStage::SpoolUp { .. })
        )
    };

    // Ordered away during the spool-up: cancelled, and no transit.
    let out = send(
        &mut state,
        1,
        order(fleet, FleetOrder::Jump { system: neighbour }),
    );
    assert!(out.fleet_events.iter().any(|e| matches!(
        e,
        FleetEvent::SpoolUpStarted { fleet: f, .. } if *f == fleet
    )));
    idle(&mut state, 20);
    assert!(spooling(fleet, &state));
    let away = gate + (Vec2::ZERO - gate).normalized().unwrap() * 10_000.0;
    let out = send(
        &mut state,
        1,
        order(
            fleet,
            FleetOrder::Move {
                to: at(home_system, away),
            },
        ),
    );
    assert!(
        out.fleet_events
            .contains(&FleetEvent::SpoolUpCancelled { fleet })
    );
    for out in idle(&mut state, 200) {
        assert!(
            !out.fleet_events
                .iter()
                .any(|e| matches!(e, FleetEvent::Departed { .. }))
        );
    }
    assert_eq!(
        state.fleet(fleet).unwrap().place,
        Place::InSystem(home_system)
    );

    // Pushed out of jump range during the spool-up: cancelled, then restarted once back.
    send(
        &mut state,
        1,
        order(fleet, FleetOrder::Jump { system: neighbour }),
    );
    let (started, _) = run_until(
        &mut state,
        2_000,
        |e| matches!(e, FleetEvent::SpoolUpStarted { fleet: f, .. } if *f == fleet),
    );
    idle(&mut state, 10);
    let ship = state.fleet(fleet).unwrap().ships[0];
    if let Some(m) = state.entities.get_mut::<Motion>(ship.sim_id()) {
        m.position = (gate + Vec2::new(0.0, 3_000.0)).into();
    }
    let (cancelled, _) = run_until(&mut state, 2, |e| {
        *e == FleetEvent::SpoolUpCancelled { fleet }
    });
    assert!(cancelled < started.after(Ticks::from_secs(5)));
    let (restarted, event) = run_until(
        &mut state,
        2_000,
        |e| matches!(e, FleetEvent::SpoolUpStarted { fleet: f, .. } if *f == fleet),
    );
    let FleetEvent::SpoolUpStarted { until, .. } = event else {
        unreachable!()
    };
    assert_eq!(until, restarted.after(Ticks::from_secs(5)));
    let (departed, _) = run_until(
        &mut state,
        100,
        |e| matches!(e, FleetEvent::Departed { fleet: f, .. } if *f == fleet),
    );
    assert_eq!(departed, until);
}

#[test]
fn a_jump_follows_the_shortest_route_by_time_over_several_lanes() {
    let mut state = stage1(4);
    let (_, home_system) = home(&state, 1);
    // The hub is two jumps from every home, by either neighbouring standard system.
    let hub = state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == crate::galaxy::SystemKind::Hub)
        .map(|(id, _)| id)
        .unwrap();
    let start = gate_position(&state, state.galaxy().system(home_system).unwrap().gates[0]);
    let fleet = spawn_fleet(
        &mut state,
        commander(1),
        home_system,
        start * 0.5,
        &[ShipClass::Mule],
    );
    send(
        &mut state,
        1,
        order(fleet, FleetOrder::Jump { system: hub }),
    );
    let planned = route::plan(
        state.galaxy(),
        home_system,
        start * 0.5,
        RouteEnd::System(hub),
        600.0,
        Ticks::from_secs(5),
    )
    .unwrap();
    assert_eq!(planned.hops.len(), 2);
    // Every alternative through the other neighbour takes at least as long.
    for &via in &state.galaxy().neighbours(home_system) {
        let (out_gate, in_gate) = gates_between(&state, home_system, via);
        let (hub_gate, _) = gates_between(&state, via, hub);
        let lane_time = |a: GateId| {
            let g = state.galaxy().gate(a).unwrap();
            g.lanes
                .iter()
                .map(|&l| state.galaxy().lane(l).unwrap())
                .find(|l| {
                    let far = l.other_end(a).unwrap();
                    let s = state.galaxy().gate(far).unwrap().system;
                    s == via || s == hub
                })
                .unwrap()
                .transit
                .get() as f64
                / 10.0
        };
        let legs = ((start * 0.5) - gate_position(&state, out_gate)).length()
            + (gate_position(&state, in_gate) - gate_position(&state, hub_gate)).length();
        let total = legs / 600.0 + lane_time(out_gate) + lane_time(hub_gate) + 10.0;
        assert!(planned.duration_s <= total + 1e-6);
    }
    // The fleet takes the planned route and arrives in the hub.
    let (_, event) = run_until(
        &mut state,
        200_000,
        |e| matches!(e, FleetEvent::Arrived { fleet: f, system, .. } if *f == fleet && *system == hub),
    );
    let FleetEvent::Arrived { gate, .. } = event else {
        unreachable!()
    };
    assert_eq!(gate, planned.hops[1].to_gate);
}
