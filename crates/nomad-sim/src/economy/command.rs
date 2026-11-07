//! Validation and execution of economy commands (D-077; S4 plan §4.7).
//!
//! These checks are part of the single validator ([`crate::validate_command`]): the same
//! path serves players, the Deputy and AI commanders.

use std::collections::BTreeSet;

use nomad_types::{BuildItem, EconomyCommand, ModuleKind, Owner, Slot};

#[cfg(test)]
use super::EntryState;
use super::{EconomyEvent, fits, logistics, queue};
use crate::command::CommandRejection;
use crate::fleet::{Fleet, FleetId, Place};
use crate::galaxy::is_claimable;
use crate::holding::HoldingId;
use crate::state::UniverseState;

/// A holding the issuer owns, or why not.
fn own_holding(
    state: &UniverseState,
    issuer: Owner,
    holding: HoldingId,
) -> Result<&crate::holding::Holding, CommandRejection> {
    let h = state
        .holdings
        .get(&holding)
        .ok_or(CommandRejection::UnknownHolding)?;
    if Owner::Commander(h.owner) != issuer {
        return Err(CommandRejection::NotOwner);
    }
    Ok(h)
}

/// A completed holding the issuer owns: outposts cannot build (D-041).
fn own_complete_holding(
    state: &UniverseState,
    issuer: Owner,
    holding: HoldingId,
) -> Result<&crate::holding::Holding, CommandRejection> {
    let h = own_holding(state, issuer, holding)?;
    if !h.core_complete {
        return Err(CommandRejection::Outpost);
    }
    Ok(h)
}

/// A fleet the issuer owns.
fn own_fleet(
    state: &UniverseState,
    issuer: Owner,
    fleet: FleetId,
) -> Result<Fleet, CommandRejection> {
    let f = state
        .entities
        .get::<Fleet>(fleet.sim_id())
        .ok_or(CommandRejection::UnknownFleet)?;
    if f.owner != issuer {
        return Err(CommandRejection::NotOwner);
    }
    Ok(f)
}

/// Checks an economy command (D-077).
pub(crate) fn validate(
    state: &UniverseState,
    issuer: Owner,
    command: &EconomyCommand,
) -> Result<(), CommandRejection> {
    match command {
        EconomyCommand::Build { holding, item } => validate_build(state, issuer, *holding, *item),
        EconomyCommand::Cancel { holding, item } => {
            own_holding(state, issuer, *holding)?;
            let known = state
                .economy
                .holdings
                .get(holding)
                .is_some_and(|e| e.queue.iter().any(|entry| entry.id == *item));
            if known {
                Ok(())
            } else {
                Err(CommandRejection::UnknownItem)
            }
        }
        EconomyCommand::SetPowerProfile { holding, .. }
        | EconomyCommand::SetRepair { holding, .. } => {
            own_complete_holding(state, issuer, *holding).map(drop)
        }
        EconomyCommand::SetRally { holding, fleet } => {
            own_complete_holding(state, issuer, *holding)?;
            if let Some(fleet) = fleet {
                own_fleet(state, issuer, *fleet)?;
            }
            Ok(())
        }
        EconomyCommand::Convoy { fleet, route } => validate_convoy(state, issuer, *fleet, route),
        EconomyCommand::LoadCoreKit { fleet } => {
            let f = own_fleet(state, issuer, *fleet)?;
            let Place::Docked(holding) = f.place else {
                return Err(CommandRejection::NotDocked);
            };
            if logistics::mules(state, &f).is_empty() {
                return Err(CommandRejection::NoMule);
            }
            if logistics::empty_mule(state, &f).is_none() {
                return Err(CommandRejection::CargoFull);
            }
            if state
                .economy
                .holdings
                .get(&holding)
                .is_none_or(|e| e.kits == 0)
            {
                return Err(CommandRejection::NoKit);
            }
            Ok(())
        }
        EconomyCommand::DeployCoreKit { fleet, site } => {
            let f = own_fleet(state, issuer, *fleet)?;
            if logistics::kit_carrier(state, &f).is_none() {
                return Err(CommandRejection::NoKit);
            }
            let s = state
                .galaxy
                .site(*site)
                .ok_or(CommandRejection::UnknownSite)?;
            if !state
                .galaxy
                .system(s.system)
                .is_some_and(|sys| is_claimable(sys.kind))
            {
                return Err(CommandRejection::NotClaimable);
            }
            // A fallen mother station on its way there has it reserved (S6b, D-164).
            if state.holding_at(*site).is_some() || crate::siege::site_reserved(state, *site) {
                return Err(CommandRejection::SiteTaken);
            }
            if !logistics::near_site(state, *fleet, &f, *site) {
                return Err(CommandRejection::TooFar);
            }
            Ok(())
        }
    }
}

fn validate_build(
    state: &UniverseState,
    issuer: Owner,
    holding: HoldingId,
    item: BuildItem,
) -> Result<(), CommandRejection> {
    let h = own_complete_holding(state, issuer, holding)?;
    let economy = state.economy.holdings.get(&holding);
    if economy.map_or(0, |e| e.queue.len()) >= state.rules.economy.queue.max_items {
        return Err(CommandRejection::QueueFull);
    }
    match item {
        BuildItem::Module { kind, slot } => {
            if kind == ModuleKind::Core {
                return Err(CommandRejection::NotBuildable);
            }
            if !state.slot_list(holding).contains(&slot) {
                return Err(CommandRejection::NoSuchSlot);
            }
            let deposit = match slot {
                Slot::Deposit(i) => state
                    .galaxy
                    .site(h.site)
                    .and_then(|s| s.deposits.get(usize::from(i)))
                    .map(|d| d.kind),
                _ => None,
            };
            if !fits(kind, slot, deposit) {
                return Err(CommandRejection::WrongSlot);
            }
            let taken = economy.is_some_and(|e| {
                e.slots.contains_key(&slot)
                    || e.queue.iter().any(|entry| {
                        matches!(entry.item, BuildItem::Module { slot: s, .. } if s == slot)
                    })
            });
            if taken {
                return Err(CommandRejection::SlotTaken);
            }
            Ok(())
        }
        BuildItem::Ship { class } => {
            if state.rules.economy.ship(class).is_none() {
                return Err(CommandRejection::NotBuildable);
            }
            if state.rules.class(class).frigate && state.dry_docks(holding) == 0 {
                return Err(CommandRejection::NoDryDock);
            }
            Ok(())
        }
        // A kit priced above the pool cap is accepted and waits like any unaffordable item:
        // storage modules raise the cap. D-189 has the harness report such kits rather than
        // the validator refuse them.
        BuildItem::CoreKit => Ok(()),
    }
}

fn validate_convoy(
    state: &UniverseState,
    issuer: Owner,
    fleet: FleetId,
    route: &[HoldingId],
) -> Result<(), CommandRejection> {
    let f = own_fleet(state, issuer, fleet)?;
    if logistics::mules(state, &f).is_empty() {
        return Err(CommandRejection::NoMule);
    }
    if route.is_empty() {
        // Ends the route.
        return Ok(());
    }
    if route.len() > state.rules.economy.logistics.max_route_stops {
        return Err(CommandRejection::RouteTooLong);
    }
    let mut seen = BTreeSet::new();
    for &stop in route {
        let h = state
            .holdings
            .get(&stop)
            .ok_or(CommandRejection::UnknownHolding)?;
        if Owner::Commander(h.owner) != issuer {
            return Err(CommandRejection::HoldingNotOwned);
        }
        if !h.core_complete {
            return Err(CommandRejection::Outpost);
        }
        if !seen.insert(stop) {
            return Err(CommandRejection::DuplicateStop);
        }
    }
    if route.last().is_none_or(|&last| !state.is_depot(last)) {
        return Err(CommandRejection::NotDepot);
    }
    if matches!(
        f.place,
        Place::InTransit { .. } | Place::EmergencyJump { .. }
    ) {
        return Err(CommandRejection::InTransit);
    }
    Ok(())
}

/// Carries out a validated economy command.
pub(crate) fn execute(
    state: &mut UniverseState,
    command: &EconomyCommand,
    events: &mut Vec<EconomyEvent>,
) {
    match command {
        EconomyCommand::Build { holding, item } => queue::enqueue(state, *holding, *item, events),
        EconomyCommand::Cancel { holding, item } => queue::cancel(state, *holding, *item, events),
        EconomyCommand::SetPowerProfile { holding, profile } => {
            state.economy.holdings.entry(*holding).or_default().profile = *profile;
        }
        EconomyCommand::SetRally { holding, fleet } => {
            state.economy.holdings.entry(*holding).or_default().rally = *fleet;
        }
        EconomyCommand::SetRepair { holding, on } => {
            state
                .economy
                .holdings
                .entry(*holding)
                .or_default()
                .repairing = *on;
        }
        EconomyCommand::Convoy { fleet, route } => {
            if route.is_empty() {
                if state.economy.convoys.remove(fleet).is_some() {
                    events.push(EconomyEvent::ConvoyEnded { fleet: *fleet });
                }
            } else {
                state.economy.convoys.insert(
                    *fleet,
                    super::Convoy {
                        route: route.clone(),
                        next: 0,
                    },
                );
            }
        }
        EconomyCommand::LoadCoreKit { fleet } => logistics::load_kit(state, *fleet, events),
        EconomyCommand::DeployCoreKit { fleet, site } => {
            logistics::deploy_kit(state, *fleet, *site, events);
        }
    }
}

/// Ends the convoy route of `fleet` because it was given another order.
pub(crate) fn end_convoy(
    state: &mut UniverseState,
    fleet: FleetId,
    events: &mut Vec<EconomyEvent>,
) {
    if state.economy.convoys.remove(&fleet).is_some() {
        events.push(EconomyEvent::ConvoyEnded { fleet });
    }
}

/// Whether a queue item is still waiting (for tests and views).
#[cfg(test)]
pub(crate) fn waiting(state: &UniverseState, holding: HoldingId, item: crate::id::SimId) -> bool {
    state.economy.holdings.get(&holding).is_some_and(|e| {
        e.queue
            .iter()
            .any(|entry| entry.id == item && entry.state == EntryState::Waiting)
    })
}
