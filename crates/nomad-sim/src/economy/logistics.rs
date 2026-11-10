//! MULE cargo, convoys and claims (D-054, D-064, D-133 to D-135; S4 plan §4.4, task 7).
//!
//! - **Convoys:** a MULE fleet with a convoy route runs a loop of own holdings ending at a
//!   depot. At a stop that is not a depot it loads the holding's buffer, alloy and crystal in
//!   proportion, up to its free cargo; at a depot it unloads into the pool, where what does
//!   not fit is lost (D-134). It travels with ordinary orders (Launch, Jump, Move), checked
//!   by the single validator (D-077), so it can be raided like any fleet. Any other order to
//!   the fleet ends the route.
//! - **Contested gates** (D-055, D-285; S4 plan §4.4, S6 plan §4.7): armed ships of another
//!   owner within 5 km of a gate, that the convoy's owner detects, make it contested for a
//!   convoy. A convoy whose next leg would depart
//!   from or arrive at a contested gate does not set out: it holds at its last stop, docked,
//!   and reports [`EconomyEvent::ConvoyHeld`] once; it goes on when the gate is clear. Whether
//!   to reroute instead is doctrine, the Deputy's (D-210). *Proposed:* the check runs when the
//!   convoy would launch or jump, on its whole next leg; a convoy already on its way is the
//!   Deputy's evacuation (A1b). A convoy route given to a fleet docked at one of its stops
//!   starts at that stop.
//! - **Cargo** is per MULE, so it follows ships through splits and merges, and is lost with
//!   a destroyed MULE.
//! - **Claims:** a finished core kit waits at its holding; a docked MULE with an empty hold
//!   loads it; deployed on a free site near the fleet, the site becomes an outpost whose core
//!   builds for 2 h and then completes as a holding (D-054, D-135).
//! - **Claim trips continue offline** (D-291): a fleet under direct control that carries a
//!   core kit and whose last order is a move to a free, claimable site (within deploy range
//!   of it) keeps its orders when its commander's grace ends, as doctrine, as convoy routes
//!   do (D-162). On arrival, its orders done and the site in range, the kit is deployed there,
//!   as the commander's `DeployCoreKit` would, through the same validator. Any order to the
//!   fleet ends the trip, and so does a site taken first, a lost kit or an arrival out of
//!   range; the fleet then station-keeps under doctrine. *Proposed:* the site is the free
//!   claimable one nearest the move's end within the deploy range, then by identity; a trip's
//!   fleet back under direct control (its commander ordered it) is no longer on the trip.

use nomad_types::{Command, FleetOrder, Location, Owner, Queueing, ShipClass, SiteId};

use super::rules::MILLI;
use super::{Cargo, EconomyEvent, Stock, queue};
use crate::command::validate_command;
use crate::fleet::{Fleet, FleetEvent, FleetId, Motion, Place, Ship, ShipId, station};
use crate::galaxy::GateId;
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
        // A contested gate on the next leg: hold at the last stop (D-055).
        if matches!(order, FleetOrder::Launch | FleetOrder::Jump { .. }) {
            let held = contested_on_leg(state, id, &fleet, system);
            if let Some(c) = state.economy.convoys.get_mut(&id)
                && c.held != held
            {
                c.held = held;
                if let Some(gate) = held {
                    events.push(EconomyEvent::ConvoyHeld { fleet: id, gate });
                }
            }
            if held.is_some() {
                return true;
            }
        }
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

/// The first gate of the route from where `fleet` is to `system` that is contested for its
/// owner, if any (D-055).
fn contested_on_leg(
    state: &UniverseState,
    id: FleetId,
    fleet: &Fleet,
    system: crate::galaxy::SystemId,
) -> Option<GateId> {
    let (from, start) = match fleet.place {
        Place::Docked(holding) => (
            state
                .holdings
                .get(&holding)
                .and_then(|h| state.galaxy.site(h.site))?
                .system,
            station::launch_point(state, holding)?,
        ),
        Place::InSystem(here) => (here, state.entities.get::<Motion>(id.sim_id())?.position),
        Place::InTransit { .. } | Place::EmergencyJump { .. } => return None,
    };
    let route = crate::fleet::route::plan(
        &state.galaxy,
        from,
        start.into(),
        crate::fleet::route::RouteEnd::System(system),
        crate::command::fleet_cruise_speed(state, fleet),
        crate::time::Ticks::from_secs(state.rules.gates.spool_up_s),
    )?;
    route
        .hops
        .iter()
        .flat_map(|hop| [hop.from_gate, hop.to_gate])
        .find(|&gate| contested(state, gate, fleet.owner))
}

/// Whether `gate` is contested for `owner`'s convoys: an armed ship of another owner that
/// `owner` detects, in space in the gate's system, within the contested radius (D-055,
/// D-285). Only ships in the owner's visibility set count (D-173), so a hidden fleet at a
/// gate can ambush a convoy, and a convoy's hold never reveals it. No alliances in the MVP,
/// so every other owner is hostile (D-040).
pub(crate) fn contested(state: &UniverseState, gate: GateId, owner: Owner) -> bool {
    let Some(g) = state.galaxy.gate(gate) else {
        return false;
    };
    let Some(picture) = crate::sensors::picture_of(state, owner, g.system) else {
        return false;
    };
    let radius = state.rules.gates.contested_radius_m;
    // The picture lists other owners' ships only; some may since have docked or jumped.
    picture.ships.iter().any(|ship| {
        let Some((class, fleet)) = state
            .entities
            .with::<Ship, _>(ship.sim_id(), |s| (s.class, s.fleet))
        else {
            return false;
        };
        state.rules.class(class).weapon_range_m.is_some()
            && state.entities.with::<Fleet, _>(fleet.sim_id(), |f| {
                f.owner != owner && f.place == Place::InSystem(g.system)
            }) == Some(true)
            && state
                .entities
                .get::<Motion>(ship.sim_id())
                .is_some_and(|m| distance_m(m.position, g.position) <= radius)
    })
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

/// The site a claim trip of `fleet` heads for (D-291): the fleet carries a core kit and its
/// last order is a move ending within deploy range of a free, claimable site; the nearest
/// such site, then by identity.
pub(crate) fn claim_trip_site(state: &UniverseState, id: FleetId) -> Option<SiteId> {
    let fleet = state.entities.get::<Fleet>(id.sim_id())?;
    kit_carrier(state, &fleet)?;
    let last = fleet
        .queue
        .back()
        .or_else(|| fleet.active.as_ref().map(|a| &a.order))?;
    let FleetOrder::Move { to } = *last else {
        return None;
    };
    if !state
        .galaxy
        .system(to.system)
        .is_some_and(|sys| crate::galaxy::is_claimable(sys.kind))
    {
        return None;
    }
    let range = state.rules.economy.logistics.deploy_range_m;
    state
        .galaxy
        .sites()
        .filter(|(_, site)| site.system == to.system)
        .filter(|&(site, _)| {
            state.holding_at(site).is_none() && !crate::siege::site_reserved(state, site)
        })
        .map(|(site, s)| (distance_m(s.position, to.position), site))
        .filter(|&(d, _)| d <= range)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, site)| site)
}

/// Runs the claim trips under way (D-291): drops those that ended, and deploys the kit of a
/// fleet that has arrived.
pub(crate) fn run_claims(state: &mut UniverseState, events: &mut Vec<EconomyEvent>) {
    if state.economy.claims.is_empty() {
        return;
    }
    let trips: Vec<(FleetId, SiteId)> = state
        .economy
        .claims
        .iter()
        .map(|(&fleet, &site)| (fleet, site))
        .collect();
    for (id, site) in trips {
        let Some(fleet) = state.entities.get::<Fleet>(id.sim_id()) else {
            state.economy.claims.remove(&id);
            continue;
        };
        let taken = state.holding_at(site).is_some() || crate::siege::site_reserved(state, site);
        if taken || kit_carrier(state, &fleet).is_none() || state.doctrine.direct.contains_key(&id)
        {
            state.economy.claims.remove(&id);
            continue;
        }
        let arrived = fleet.queue.is_empty()
            && fleet
                .active
                .as_ref()
                .is_none_or(|a| a.order == FleetOrder::StationKeep { relative_to: None })
            && matches!(fleet.place, Place::InSystem(_));
        if !arrived {
            continue;
        }
        state.economy.claims.remove(&id);
        let deploy =
            Command::Economy(nomad_types::EconomyCommand::DeployCoreKit { fleet: id, site });
        if validate_command(state, fleet.owner, &deploy).is_ok() {
            deploy_kit(state, id, site, events);
        }
    }
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
