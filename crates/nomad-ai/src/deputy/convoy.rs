//! Convoys at contested gates (doc 13 §4 item 5; D-016, D-055, D-064, D-210; A1 plan §5.8).
//!
//! The simulation runs convoy routes mechanically (D-134), and a convoy whose next leg
//! passes a contested gate holds at its last stop (D-055, S4 plan §4.4). The Deputy then:
//!
//! - **reroutes** it if a route to its next stop avoids that gate: ordinary fleet orders
//!   along the detour (launch, a jump per system, dock at the stop), which end the convoy
//!   route (S4), so the Deputy keeps the route in memory and gives it again once the fleet
//!   is docked at the stop, where it resumes;
//! - otherwise leaves it **holding** docked at its last stop, and reports it;
//! - reports `ConvoyHeld`, `ConvoyRerouted` and `ConvoyResumed` for the away report.
//!
//! Evacuation when hostiles come near (D-210's first half) needs sensed hostiles and arrives
//! with A1b.
//!
//! *Proposed* readings:
//! - the detour avoids only the gate the convoy holds for: the Deputy learns of a contested
//!   gate when its convoy holds there;
//! - each jump of the detour is checked to take the detour's lane, since a jump order is
//!   routed by the simulation; a detour that would not is no detour;
//! - a held convoy is reconsidered at every evaluation, so a route that opens is taken;
//! - if a detour order is refused or dropped, or the fleet ends its detour anywhere but at
//!   the stop, the route is given again where it is, and the simulation's convoy rules carry
//!   on from there.

use std::collections::BTreeMap;

use nomad_types::{
    Command, EconomyCommand, FleetId, FleetOrder, GateId, HoldingId, Location, Queueing, Tick,
};
use serde::{Deserialize, Serialize};

use super::{DeputyEvent, Stagger};
use crate::inbox::{CommandOutcome, InboxEntry, InboxEvent};
use crate::view::{FleetPlace, OwnConvoy, RouteHop, RouteQuery, RouteSpeed, RouteTo, View};
use crate::{Issue, Layer, Output};

/// The convoys the Deputy is looking after.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ConvoyMemory {
    convoys: BTreeMap<FleetId, Watched>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Watched {
    /// Holding at its last stop for `gate`, with no route around it.
    Held { gate: GateId },
    /// On a detour to `stop`; the route is given again there.
    Detour {
        route: Vec<HoldingId>,
        stop: HoldingId,
        /// The detour's last order, whose answer says whether the detour is under way.
        last: Command,
        /// That order was applied.
        confirmed: bool,
    },
}

/// Reads the answers to the Deputy's detour orders.
pub(super) fn read(memory: &mut ConvoyMemory, fresh: &[&InboxEntry], out: &mut Vec<FleetId>) {
    for entry in fresh {
        let InboxEvent::Outcome {
            layer: Layer::Deputy,
            command,
            outcome,
        } = &entry.event
        else {
            continue;
        };
        for (&fleet, watched) in &mut memory.convoys {
            if let Watched::Detour {
                last, confirmed, ..
            } = watched
                && last == command
            {
                match outcome {
                    CommandOutcome::Applied => *confirmed = true,
                    // The detour failed: give the route again where the fleet is.
                    CommandOutcome::Rejected | CommandOutcome::Dropped(_) => out.push(fleet),
                }
            }
        }
    }
}

/// Looks after the due convoys.
pub(super) fn act(
    view: &impl View,
    memory: &mut ConvoyMemory,
    failed: &[FleetId],
    stagger: Stagger,
    out: &mut Output,
) {
    let now = view.now();
    let convoys: BTreeMap<FleetId, OwnConvoy> = view
        .own_convoys()
        .into_iter()
        .map(|c| (c.fleet, c))
        .collect();
    // Convoys holding at a contested gate, and those that resumed.
    for (&fleet, convoy) in &convoys {
        if !stagger.due(fleet.sim_id().get()) || view.direct_control(fleet).is_some() {
            continue;
        }
        match (convoy.held_at, memory.convoys.get(&fleet)) {
            // Its detour orders are on their way through the outbox.
            (_, Some(Watched::Detour { .. })) => {}
            (Some(gate), _) => held(view, memory, convoy, gate, now, out),
            (None, Some(Watched::Held { .. })) => {
                memory.convoys.remove(&fleet);
                out.events.push(DeputyEvent::ConvoyResumed { fleet });
            }
            (None, _) => {}
        }
    }
    // Detours under way.
    let detours: Vec<FleetId> = memory
        .convoys
        .iter()
        .filter(|(_, w)| matches!(w, Watched::Detour { .. }))
        .map(|(&f, _)| f)
        .collect();
    for fleet in detours {
        let due = stagger.due(fleet.sim_id().get());
        let failed = failed.contains(&fleet);
        if !due && !failed {
            continue;
        }
        let Some(Watched::Detour {
            route,
            stop,
            confirmed,
            ..
        }) = memory.convoys.get(&fleet).cloned()
        else {
            continue;
        };
        // Gone, or the commander took it over (RK-10): not the Deputy's any more.
        let Some(own) = view.own_fleet(fleet) else {
            memory.convoys.remove(&fleet);
            continue;
        };
        if view.direct_control(fleet).is_some() {
            memory.convoys.remove(&fleet);
            continue;
        }
        // Still on its route: the detour orders have not applied yet, or, once they have,
        // the commander gave it a route again.
        if convoys.contains_key(&fleet) {
            if confirmed {
                memory.convoys.remove(&fleet);
            }
            continue;
        }
        let arrived = own.place == FleetPlace::Docked(stop) && !own.busy;
        let ended = confirmed && !own.busy;
        if failed || arrived || ended {
            memory.convoys.remove(&fleet);
            out.commands.push(Issue {
                trigger: now,
                command: Command::Economy(EconomyCommand::Convoy { fleet, route }),
            });
            out.events.push(DeputyEvent::ConvoyResumed { fleet });
        }
    }
}

/// A convoy holds for `gate`: reroute it if a detour exists, else let it hold.
fn held(
    view: &impl View,
    memory: &mut ConvoyMemory,
    convoy: &OwnConvoy,
    gate: GateId,
    now: Tick,
    out: &mut Output,
) {
    let fleet = convoy.fleet;
    if let Some(orders) = detour(view, convoy, gate) {
        let last = orders
            .last()
            .cloned()
            .expect("invariant: a detour ends with its dock order");
        let stop = convoy.route[convoy.next % convoy.route.len().max(1)];
        out.commands.extend(orders.into_iter().map(|command| Issue {
            trigger: now,
            command,
        }));
        memory.convoys.insert(
            fleet,
            Watched::Detour {
                route: convoy.route.clone(),
                stop,
                last,
                confirmed: false,
            },
        );
        out.events.push(DeputyEvent::ConvoyRerouted { fleet, gate });
    } else if memory.convoys.get(&fleet) != Some(&Watched::Held { gate }) {
        memory.convoys.insert(fleet, Watched::Held { gate });
        out.events.push(DeputyEvent::ConvoyHeld { fleet, gate });
    }
}

/// The orders that take `convoy` to its next stop around `gate`, if there is such a route
/// and every jump of it would take the detour's lane.
fn detour(view: &impl View, convoy: &OwnConvoy, gate: GateId) -> Option<Vec<Command>> {
    let fleet = view.own_fleet(convoy.fleet)?;
    let stop = *convoy.route.get(convoy.next)?;
    let (from, docked) = match fleet.place {
        FleetPlace::InSpace(at) => (at, false),
        FleetPlace::Docked(holding) => (view.holding_location(holding)?, true),
        FleetPlace::Travelling => return None,
    };
    let speed = RouteSpeed::Fleet(convoy.fleet);
    let route = view.route(&RouteQuery {
        from,
        to: RouteTo::Holding(stop),
        speed,
        avoid: vec![gate],
    })?;
    if route.hops.is_empty() {
        return None;
    }
    if !jumps_follow(view, from, &route.hops, gate, speed) {
        return None;
    }
    let order = |order, queue| Command::Fleet {
        fleet: convoy.fleet,
        order,
        queue,
        retreat: false,
    };
    let mut orders = Vec::new();
    if docked {
        orders.push(order(FleetOrder::Launch, Queueing::Replace));
    }
    for hop in &route.hops {
        let queue = if orders.is_empty() {
            Queueing::Replace
        } else {
            Queueing::Append
        };
        orders.push(order(
            FleetOrder::Jump {
                system: hop.to_system,
            },
            queue,
        ));
    }
    orders.push(order(FleetOrder::Dock { at: stop }, Queueing::Append));
    Some(orders)
}

/// Whether the simulation's own route of each jump, from where the previous one arrives,
/// avoids `gate` too.
fn jumps_follow(
    view: &impl View,
    start: Location,
    hops: &[RouteHop],
    gate: GateId,
    speed: RouteSpeed,
) -> bool {
    let mut from = Some(start);
    for hop in hops {
        let Some(at) = from else {
            return false;
        };
        let Some(own) = view.route(&RouteQuery {
            from: at,
            to: RouteTo::System(hop.to_system),
            speed,
            avoid: Vec::new(),
        }) else {
            return false;
        };
        if own
            .hops
            .iter()
            .any(|h| h.from_gate == gate || h.to_gate == gate)
        {
            return false;
        }
        from = view.gate_location(hop.to_gate);
    }
    true
}
