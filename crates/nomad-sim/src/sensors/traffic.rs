//! Lane traffic: observed departures (doc 16 §4; D-074, D-179; S7 plan §4.5).
//!
//! - **A departure is observed** by every other observer whose picture, from the last pass,
//!   holds a ship of the fleet when it enters the lane (D-179). A fleet spools up for 5 s
//!   within 2 km of its gate, so the last pass always falls inside that window.
//! - **The record:** the fleet, its owner, its composition as seen (the detected ships by
//!   class), the origin system, gate and lane, the destination and the arrival tick. One per
//!   fleet and observer: a later departure of the same fleet replaces it.
//! - **Expiry:** when the arrival is observed (the fleet is in the observer's picture at the
//!   destination) or 10 min after the arrival tick, whichever comes first, checked at each
//!   pass.
//! - **Own fleets in transit** are own information ([`super::CommanderView::own_fleets`]),
//!   not records. NPC factions keep lane traffic too (D-181).

use nomad_types::{FleetId, GateId, LaneId, Owner, ShipId, SystemId};
use serde::{Deserialize, Serialize};

use super::memory::{ClassSeen, classes_seen};
use super::{Observer, VisibilityEvent};
use crate::fleet::{Fleet, FleetEvent, Place};
use crate::state::UniverseState;
use crate::time::{Tick, Ticks};

/// An observed departure (D-179).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneTraffic {
    /// The fleet.
    pub fleet: FleetId,
    /// Its owner.
    pub owner: Owner,
    /// Its detected ships by class, in class order.
    pub classes: Vec<ClassSeen>,
    /// The system it left.
    pub from: SystemId,
    /// The gate it left by.
    pub gate: GateId,
    /// The lane it took.
    pub lane: LaneId,
    /// The system it goes to.
    pub to: SystemId,
    /// The tick it entered the lane.
    pub departed: Tick,
    /// The tick it arrives.
    pub arrives: Tick,
}

/// Records the departures among `fleet_events` of tick `now` for every observer that saw them.
pub(crate) fn observe_departures(
    state: &mut UniverseState,
    now: Tick,
    fleet_events: &[FleetEvent],
    events: &mut Vec<VisibilityEvent>,
) {
    for event in fleet_events {
        let FleetEvent::Departed {
            fleet,
            lane,
            arrive,
        } = *event
        else {
            continue;
        };
        let Some((owner, place, members)) = state.entities.with::<Fleet, _>(fleet.sim_id(), |f| {
            (
                f.owner,
                f.place,
                f.members.keys().copied().collect::<Vec<_>>(),
            )
        }) else {
            continue;
        };
        let Place::InTransit { to_gate, .. } = place else {
            continue;
        };
        let Some(gate) = state.galaxy.lane(lane).and_then(|l| l.other_end(to_gate)) else {
            continue;
        };
        let (Some(from), Some(to)) = (
            state.galaxy.gate(gate).map(|g| g.system),
            state.galaxy.gate(to_gate).map(|g| g.system),
        ) else {
            continue;
        };
        let seen: Vec<(Owner, Vec<ShipId>)> = state
            .sensors
            .observers
            .iter()
            .filter(|&(&observer, _)| observer != owner)
            .filter_map(|(&observer, o)| {
                let picture = o.picture.system(from)?;
                let ships: Vec<ShipId> = members
                    .iter()
                    .copied()
                    .filter(|&s| picture.has_ship(s))
                    .collect();
                (!ships.is_empty()).then_some((observer, ships))
            })
            .collect();
        for (observer, ships) in seen {
            let record = LaneTraffic {
                fleet,
                owner,
                classes: classes_seen(state, &ships),
                from,
                gate,
                lane,
                to,
                departed: now,
                arrives: arrive,
            };
            if let Some(o) = state.sensors.observers.get_mut(&observer) {
                o.traffic.insert(fleet, record);
            }
            events.push(VisibilityEvent::DepartureObserved {
                observer,
                fleet,
                to,
                arrives: arrive,
            });
        }
    }
}

/// Drops the records whose arrival `observer` sees at this pass, or whose arrival tick is
/// more than the rules' 10 min ago (D-179).
pub(crate) fn expire(state: &UniverseState, now: Tick, observer: &mut Observer) {
    let after = Ticks::from_secs(state.rules.sensors.lane_traffic_after_arrival_s);
    let Observer {
        picture, traffic, ..
    } = observer;
    traffic.retain(|&fleet, record| {
        let arrived_in_view = picture
            .system(record.to)
            .is_some_and(|p| p.has_fleet(fleet));
        !arrived_in_view && now < record.arrives.after(after)
    });
}
