//! MULE cargo, convoys and claims (D-054, D-064, D-133 to D-135; S4 plan §4.4, task 7).
//!
//! - **Convoys:** a MULE fleet with a convoy route runs a loop of own holdings ending at a
//!   depot. At a stop that is not a depot it loads the holding's buffer, alloy and crystal in
//!   proportion, up to its free cargo; at a depot it unloads into the pool, where what does
//!   not fit is lost (D-134). It travels with ordinary orders (Launch, Jump, Move), checked
//!   by the single validator (D-077), so it can be raided like any fleet. Any other order to
//!   the fleet ends the route. Holding at a contested gate (D-055) arrives with the gate
//!   rules (S6).
//! - **Cargo** is per MULE, so it follows ships through splits and merges, and is lost with
//!   a destroyed MULE.
//! - **Claims:** a finished core kit waits at its holding; a docked MULE with an empty hold
//!   loads it; deployed on a free site near the fleet, the site becomes an outpost whose core
//!   builds for 2 h and then completes as a holding (D-054, D-135).

use nomad_types::{Command, FleetOrder, Location, Owner, Queueing, ShipClass, SiteId};

use super::rules::MILLI;
use super::{Cargo, EconomyEvent, Stock, queue};
use crate::command::validate_command;
use crate::fleet::{Fleet, FleetEvent, FleetId, Motion, Place, Ship, ShipId, station};
use crate::geometry::distance_m;
use crate::holding::HoldingId;
use crate::state::UniverseState;
use crate::time::{TICKS_PER_SECOND, Tick};

/// The MULEs of `fleet`, in identity order.
pub(crate) fn mules(state: &UniverseState, fleet: &Fleet) -> Vec<ShipId> {
    fleet
        .members
        .keys()
        .copied()
        .filter(|ship| {
            state
                .entities
                .with::<Ship, _>(ship.sim_id(), |s| s.class == ShipClass::Mule)
                == Some(true)
        })
        .collect()
}

/// Whether `fleet` carries a core kit, and in which MULE.
pub(crate) fn kit_carrier(state: &UniverseState, fleet: &Fleet) -> Option<ShipId> {
    mules(state, fleet)
        .into_iter()
        .find(|ship| state.cargo(*ship).core_kit)
}

/// A MULE of `fleet` with an empty hold.
pub(crate) fn empty_mule(state: &UniverseState, fleet: &Fleet) -> Option<ShipId> {
    mules(state, fleet).into_iter().find(|ship| {
        let cargo = state.cargo(*ship);
        !cargo.core_kit && cargo.stock.is_zero()
    })
}

/// Runs the convoys whose slot is this tick, and drops cargo and routes of ships and fleets
/// that are gone.
pub(crate) fn run(
    state: &mut UniverseState,
    now: Tick,
    slot: u64,
    events: &mut Vec<EconomyEvent>,
    fleet_events: &mut Vec<FleetEvent>,
) {
    if slot == 0 {
        let gone: Vec<ShipId> = state
            .economy
            .cargo
            .keys()
            .copied()
            .filter(|ship| !state.entities.has::<Ship>(ship.sim_id()))
            .collect();
        for ship in gone {
            state.economy.cargo.remove(&ship);
        }
    }
    let due: Vec<FleetId> = state
        .economy
        .convoys
        .keys()
        .copied()
        .filter(|f| f.sim_id().get() % TICKS_PER_SECOND == slot)
        .collect();
    for fleet in due {
        if !convoy_second(state, fleet, now, events, fleet_events) {
            state.economy.convoys.remove(&fleet);
            events.push(EconomyEvent::ConvoyEnded { fleet });
        }
    }
}

/// One second of a convoy. Returns `false` if the route must end.
fn convoy_second(
    state: &mut UniverseState,
    id: FleetId,
    now: Tick,
    events: &mut Vec<EconomyEvent>,
    fleet_events: &mut Vec<FleetEvent>,
) -> bool {
    let _ = now;
    let Some(fleet) = state.entities.get::<Fleet>(id.sim_id()) else {
        return false;
    };
    if mules(state, &fleet).is_empty() {
        return false;
    }
    let Some(convoy) = state.economy.convoys.get(&id).cloned() else {
        return false;
    };
    let Some(&stop) = convoy.route.get(convoy.next) else {
        return false;
    };
    let Some(h) = state.holdings.get(&stop) else {
        return false;
    };
    if fleet.owner != Owner::Commander(h.owner) || !h.core_complete {
        return false;
    }
    let Some(system) = state.galaxy.site(h.site).map(|s| s.system) else {
        return false;
    };
    // Busy: moving, spooling up or in transit.
    if fleet.active.is_some() || !fleet.queue.is_empty() {
        return true;
    }
    let Some(point) = station::launch_point(state, stop) else {
        return false;
    };
    let order = match fleet.place {
        Place::InTransit { .. } | Place::EmergencyJump { .. } => return true,
        Place::Docked(at) if at == stop => None,
        Place::Docked(_) => Some(FleetOrder::Launch),
        Place::InSystem(here) if here != system => Some(FleetOrder::Jump { system }),
        Place::InSystem(_) => {
            let anchor = state
                .entities
                .get::<Motion>(id.sim_id())
                .map(|m| m.position);
            match anchor {
                Some(a) if distance_m(a, point) <= state.rules.economy.logistics.stop_radius_m => {
                    None
                }
                _ => Some(FleetOrder::Move {
                    to: Location {
                        system,
                        position: point,
                    },
                }),
            }
        }
    };
    if let Some(order) = order {
        let command = Command::Fleet {
            fleet: id,
            order: order.clone(),
            queue: Queueing::Replace,
            retreat: false,
        };
        if validate_command(state, fleet.owner, &command).is_err() {
            return false;
        }
        crate::fleet::orders::give(state, id, order, Queueing::Replace, fleet_events);
        return true;
    }
    // At the stop.
    if state.is_depot(stop) {
        unload(state, id, &fleet, stop, events);
    } else {
        load(state, id, &fleet, stop, events);
    }
    if let Some(convoy) = state.economy.convoys.get_mut(&id) {
        convoy.next = (convoy.next + 1) % convoy.route.len().max(1);
    }
    true
}

/// Loads `holding`'s buffer into the fleet's MULEs, alloy and crystal in proportion, up to
/// their free cargo (D-134).
fn load(
    state: &mut UniverseState,
    id: FleetId,
    fleet: &Fleet,
    holding: HoldingId,
    events: &mut Vec<EconomyEvent>,
) {
    let capacity = state
        .rules
        .economy
        .logistics
        .mule_units
        .saturating_mul(MILLI);
    let carriers = mules(state, fleet);
    let free: u64 = carriers
        .iter()
        .map(|ship| {
            let cargo = state.cargo(*ship);
            if cargo.core_kit {
                0
            } else {
                capacity.saturating_sub(cargo.stock.total())
            }
        })
        .sum();
    let Some(buffer) = state.economy.holdings.get(&holding).map(|e| e.buffer) else {
        return;
    };
    let total = buffer.total();
    if total == 0 || free == 0 {
        return;
    }
    let take = total.min(free);
    let alloy = u64::try_from(u128::from(take) * u128::from(buffer.alloy) / u128::from(total))
        .unwrap_or(0)
        .min(buffer.alloy);
    let crystal = (take - alloy).min(buffer.crystal);
    let mut left = Stock { alloy, crystal };
    let taken = left;
    for ship in carriers {
        if left.is_zero() {
            break;
        }
        let cargo = state.economy.cargo.entry(ship).or_default();
        if cargo.core_kit {
            continue;
        }
        let room = capacity.saturating_sub(cargo.stock.total());
        let a = left.alloy.min(room);
        let c = left.crystal.min(room - a);
        cargo.stock = cargo.stock.plus(Stock {
            alloy: a,
            crystal: c,
        });
        left = left.minus(Stock {
            alloy: a,
            crystal: c,
        });
    }
    let loaded = taken.minus(left);
    if let Some(e) = state.economy.holdings.get_mut(&holding) {
        e.buffer = e.buffer.minus(loaded);
    }
    events.push(EconomyEvent::ConvoyLoaded {
        fleet: id,
        holding,
        amount: loaded,
    });
}

/// Unloads the fleet's cargo into its owner's pool at depot `holding` (D-134).
fn unload(
    state: &mut UniverseState,
    id: FleetId,
    fleet: &Fleet,
    holding: HoldingId,
    events: &mut Vec<EconomyEvent>,
) {
    let Some(owner) = state.holdings.get(&holding).map(|h| h.owner) else {
        return;
    };
    let mut amount = Stock::ZERO;
    for ship in mules(state, fleet) {
        if let Some(cargo) = state.economy.cargo.get_mut(&ship)
            && !cargo.core_kit
        {
            amount = amount.plus(cargo.stock);
            cargo.stock = Stock::ZERO;
        }
    }
    state
        .economy
        .cargo
        .retain(|_, c| c.core_kit || !c.stock.is_zero());
    if amount.is_zero() {
        return;
    }
    let kept = queue::credit_pool(state, owner, amount, events);
    events.push(EconomyEvent::ConvoyUnloaded {
        fleet: id,
        holding,
        amount: kept,
        lost: amount.minus(kept),
    });
}

/// Loads a core kit stored at the holding where `fleet` is docked into an empty MULE
/// (validated).
pub(crate) fn load_kit(state: &mut UniverseState, id: FleetId, events: &mut Vec<EconomyEvent>) {
    let Some(fleet) = state.entities.get::<Fleet>(id.sim_id()) else {
        return;
    };
    let Place::Docked(holding) = fleet.place else {
        return;
    };
    let Some(mule) = empty_mule(state, &fleet) else {
        return;
    };
    let Some(e) = state.economy.holdings.get_mut(&holding) else {
        return;
    };
    if e.kits == 0 {
        return;
    }
    e.kits -= 1;
    state.economy.cargo.insert(
        mule,
        Cargo {
            stock: Stock::ZERO,
            core_kit: true,
        },
    );
    events.push(EconomyEvent::KitLoaded { fleet: id, holding });
}

/// Deploys the kit `fleet` carries on `site` (validated): an outpost whose core builds for
/// the deployment time, its hull growing from 10% (D-054, D-224).
pub(crate) fn deploy_kit(
    state: &mut UniverseState,
    id: FleetId,
    site: SiteId,
    events: &mut Vec<EconomyEvent>,
) {
    let Some(fleet) = state.entities.get::<Fleet>(id.sim_id()) else {
        return;
    };
    let Owner::Commander(owner) = fleet.owner else {
        return;
    };
    let Some(carrier) = kit_carrier(state, &fleet) else {
        return;
    };
    let Ok(holding) = state.insert_holding(site, owner, false) else {
        return;
    };
    state.economy.cargo.remove(&carrier);
    super::spawn_outpost_core(state, holding);
    events.push(EconomyEvent::OutpostDeployed { holding, site });
}

/// Whether `fleet` is close enough to `site` to deploy a kit there.
pub(crate) fn near_site(state: &UniverseState, id: FleetId, fleet: &Fleet, site: SiteId) -> bool {
    let Some(s) = state.galaxy.site(site) else {
        return false;
    };
    if fleet.place != Place::InSystem(s.system) {
        return false;
    }
    state.entities.get::<Motion>(id.sim_id()).is_some_and(|m| {
        distance_m(m.position, s.position) <= state.rules.economy.logistics.deploy_range_m
    })
}
