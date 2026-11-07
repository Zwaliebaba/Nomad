//! Doctrine and direct control (Design doc 12 §5.2, §5.3; RK-10; D-016, D-082, D-162;
//! A1 plan §5.1, §5.2).
//!
//! **Doctrine** is player input, stored per commander and private (D-075): a preset (D-082,
//! Balanced by default) plus whole overrides per holding and per fleet, and the replacement
//! list (D-208). The preset values come from the rules file (D-211). Only the commander's
//! own client or Strategist may change it: the validator refuses doctrine commands from the
//! Deputy (PR-3, doc 12 §5.1).
//!
//! **Direct control** (RK-10, *Derived* in A1 plan §5.2): a fleet is under it from the moment
//! its commander's client or Strategist gives it any fleet order, until a `Release`, the
//! lapse after the disconnect grace (D-162) or its destruction. While under it the Deputy's
//! commands for the fleet are dropped when they would apply, except the retreat the direct
//! order opted into ([`blocks_deputy`]). The fleet view marks it for the HUD.
//!
//! *Proposed* readings:
//! - the retreat opt-in is the last direct order's flag, whether it replaced or joined the
//!   queue;
//! - a fleet formed with `CreateFleet` takes the direct control of the fleet its first listed
//!   ship came from (A1 plan §5.2: "splits inherit direct control"); a merge keeps the
//!   receiving fleet's;
//! - giving a fleet a convoy route releases it, since convoy routes are doctrine (D-162);
//! - a Deputy command "for" a fleet is a fleet order to it, a fleet-management command that
//!   takes its ships, or a convoy or core-kit command for it; holding and rally commands are
//!   not;
//! - overrides of holdings the commander no longer owns, and of fleets that are gone, are
//!   dropped at the end of each tick; replacement entries stay, since a lost fleet is rebuilt
//!   as a new one (A1 plan §5.7).

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{
    Command, DirectControl, DoctrineCommand, DoctrinePreset, EconomyCommand, FleetDoctrine,
    FleetOrder, FleetRole, HoldingDoctrine, Owner, ReplacementEntry, ShipId,
};
use serde::{Deserialize, Serialize};

use crate::command::{CommandRejection, CommandSource};
use crate::fleet::{Fleet, FleetId, Ship};
use crate::holding::HoldingId;
use crate::id::CommanderId;
use crate::state::UniverseState;

/// Doctrine and direct control, part of the state (A1 plan §5.1, §5.2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Doctrines {
    pub(crate) commanders: BTreeMap<CommanderId, Doctrine>,
    pub(crate) direct: BTreeMap<FleetId, DirectControl>,
}

/// A commander's doctrine as they set it (doc 12 §5.2). Private to the commander (D-075).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Doctrine {
    /// The preset.
    pub preset: DoctrinePreset,
    /// Holding overrides.
    pub holdings: BTreeMap<HoldingId, HoldingDoctrine>,
    /// Fleet overrides.
    pub fleets: BTreeMap<FleetId, FleetDoctrine>,
    /// The replacement list, in order (D-208).
    pub replacement: Vec<ReplacementEntry>,
}

impl UniverseState {
    /// A commander's doctrine; the default (Balanced, no overrides) if they never set one.
    /// Private to the commander (D-075).
    pub fn doctrine(&self, commander: CommanderId) -> Doctrine {
        self.doctrine
            .commanders
            .get(&commander)
            .cloned()
            .unwrap_or_default()
    }

    /// A commander's preset (D-082).
    pub fn doctrine_preset(&self, commander: CommanderId) -> DoctrinePreset {
        self.doctrine
            .commanders
            .get(&commander)
            .map_or_else(DoctrinePreset::default, |d| d.preset)
    }

    /// The doctrine that applies to `holding` for its owner: the override, or the preset's.
    pub fn holding_doctrine(&self, holding: HoldingId) -> Option<HoldingDoctrine> {
        let owner = self.holdings.get(&holding)?.owner;
        let set = self.doctrine.commanders.get(&owner);
        Some(
            set.and_then(|d| d.holdings.get(&holding).copied())
                .unwrap_or_else(|| self.rules.ai.preset(self.doctrine_preset(owner)).holding),
        )
    }

    /// The doctrine that applies to `fleet` for its owner: the override, or the preset's.
    /// `None` for a fleet that does not exist or that no commander owns.
    pub fn fleet_doctrine(&self, fleet: FleetId) -> Option<FleetDoctrine> {
        let owner = self
            .entities
            .with::<Fleet, _>(fleet.sim_id(), |f| f.owner)?
            .commander()?;
        let set = self.doctrine.commanders.get(&owner);
        Some(
            set.and_then(|d| d.fleets.get(&fleet).cloned())
                .unwrap_or_else(|| self.rules.ai.preset(self.doctrine_preset(owner)).fleet()),
        )
    }

    /// Whether `fleet` is under direct control, and with which retreat opt-in (RK-10).
    pub fn direct_control(&self, fleet: FleetId) -> Option<DirectControl> {
        self.doctrine.direct.get(&fleet).copied()
    }
}

/// Checks a doctrine command (D-077).
pub(crate) fn validate(
    state: &UniverseState,
    issuer: Owner,
    source: CommandSource,
    command: &DoctrineCommand,
) -> Result<(), CommandRejection> {
    use CommandRejection as R;
    if source == CommandSource::Ai(nomad_ai::Layer::Deputy) {
        return Err(R::DoctrineByDeputy);
    }
    let me = issuer
        .commander()
        .filter(|id| state.commanders.records.contains_key(id))
        .ok_or(R::UnknownCommander)?;
    match command {
        DoctrineCommand::SetPreset { .. } => Ok(()),
        DoctrineCommand::SetHolding { holding, doctrine } => {
            own_holding(state, me, *holding, R::NotOwner)?;
            doctrine.as_ref().map_or(Ok(()), check_holding_doctrine)
        }
        DoctrineCommand::SetFleet { fleet, doctrine } => {
            own_fleet(state, issuer, *fleet)?;
            let Some(doctrine) = doctrine else {
                return Ok(());
            };
            if doctrine.retreat_below_permille > 1_000 || doctrine.leash_m == 0 {
                return Err(R::InvalidDoctrine);
            }
            doctrine
                .role
                .as_ref()
                .map_or(Ok(()), |role| check_role(state, me, Some(*fleet), role))
        }
        DoctrineCommand::SetReplacement { entries } => {
            check_replacement(state, issuer, me, entries)
        }
        DoctrineCommand::Release { fleet } => {
            own_fleet(state, issuer, *fleet)?;
            if state.doctrine.direct.contains_key(fleet) {
                Ok(())
            } else {
                Err(R::NotUnderDirectControl)
            }
        }
    }
}

fn own_fleet(state: &UniverseState, issuer: Owner, fleet: FleetId) -> Result<(), CommandRejection> {
    let owner = state
        .entities
        .with::<Fleet, _>(fleet.sim_id(), |f| f.owner)
        .ok_or(CommandRejection::UnknownFleet)?;
    if owner == issuer {
        Ok(())
    } else {
        Err(CommandRejection::NotOwner)
    }
}

fn own_holding(
    state: &UniverseState,
    me: CommanderId,
    holding: HoldingId,
    not_mine: CommandRejection,
) -> Result<(), CommandRejection> {
    let h = state
        .holdings
        .get(&holding)
        .ok_or(CommandRejection::UnknownHolding)?;
    if h.owner == me { Ok(()) } else { Err(not_mine) }
}

fn check_holding_doctrine(doctrine: &HoldingDoctrine) -> Result<(), CommandRejection> {
    if doctrine.surge_below_permille > 1_000
        || doctrine.zone_radius_m == 0
        || doctrine.evacuation_radius_m == 0
        || doctrine.flak_light_hostiles == Some(0)
    {
        return Err(CommandRejection::InvalidDoctrine);
    }
    Ok(())
}

/// A role names an own holding, an own other fleet, or a patrol route of valid points in one
/// system.
fn check_role(
    state: &UniverseState,
    me: CommanderId,
    fleet: Option<FleetId>,
    role: &FleetRole,
) -> Result<(), CommandRejection> {
    use CommandRejection as R;
    match role {
        FleetRole::Garrison { holding } | FleetRole::Reserve { holding } => {
            own_holding(state, me, *holding, R::HoldingNotOwned)
        }
        FleetRole::Escort { fleet: escorted } => {
            if Some(*escorted) == fleet {
                return Err(R::SelfTarget);
            }
            own_fleet(state, Owner::Commander(me), *escorted)
        }
        FleetRole::Patrol { route } => {
            let first = route.first().ok_or(R::EmptyRoute)?;
            route
                .iter()
                .try_for_each(|to| crate::command::check_route_point(state, first.system, to))
        }
    }
}

fn check_replacement(
    state: &UniverseState,
    issuer: Owner,
    me: CommanderId,
    entries: &[ReplacementEntry],
) -> Result<(), CommandRejection> {
    use CommandRejection as R;
    let max = usize::try_from(state.rules.ai.max_replacement_entries).unwrap_or(usize::MAX);
    if entries.len() > max {
        return Err(R::InvalidDoctrine);
    }
    let mut seen = BTreeSet::new();
    for entry in entries {
        own_fleet(state, issuer, entry.fleet)?;
        if !state.rules.economy.ships.contains_key(&entry.class) {
            return Err(R::NotBuildable);
        }
        if entry.count == 0 || !seen.insert((entry.fleet, entry.class)) {
            return Err(R::InvalidDoctrine);
        }
        if let Some(role) = &entry.role {
            check_role(state, me, None, role)?;
        }
    }
    Ok(())
}

/// Carries out a validated doctrine command.
pub(crate) fn execute(state: &mut UniverseState, issuer: Owner, command: &DoctrineCommand) {
    let Some(me) = issuer.commander() else {
        return;
    };
    if let DoctrineCommand::Release { fleet } = command {
        state.doctrine.direct.remove(fleet);
        return;
    }
    let doctrine = state.doctrine.commanders.entry(me).or_default();
    match command {
        DoctrineCommand::SetPreset { preset } => doctrine.preset = *preset,
        DoctrineCommand::SetHolding {
            holding,
            doctrine: set,
        } => match set {
            Some(set) => {
                doctrine.holdings.insert(*holding, *set);
            }
            None => {
                doctrine.holdings.remove(holding);
            }
        },
        DoctrineCommand::SetFleet {
            fleet,
            doctrine: set,
        } => match set {
            Some(set) => {
                doctrine.fleets.insert(*fleet, set.clone());
            }
            None => {
                doctrine.fleets.remove(fleet);
            }
        },
        DoctrineCommand::SetReplacement { entries } => doctrine.replacement.clone_from(entries),
        DoctrineCommand::Release { .. } => {}
    }
}

/// `fleet` was given a direct order with this retreat opt-in (RK-10).
pub(crate) fn take_control(state: &mut UniverseState, fleet: FleetId, retreat: bool) {
    state
        .doctrine
        .direct
        .insert(fleet, DirectControl { retreat });
}

/// `fleet` returns to doctrine.
pub(crate) fn release(state: &mut UniverseState, fleet: FleetId) {
    state.doctrine.direct.remove(&fleet);
}

/// The direct control of the fleet `ship` is in, if any.
pub(crate) fn control_of_ship(
    state: &UniverseState,
    ship: Option<ShipId>,
) -> Option<DirectControl> {
    let fleet = state
        .entities
        .with::<Ship, _>(ship?.sim_id(), |s| s.fleet)?;
    state.direct_control(fleet)
}

/// A fleet formed from ships takes the direct control their first fleet had.
pub(crate) fn inherit_control(
    state: &mut UniverseState,
    fleet: FleetId,
    control: Option<DirectControl>,
) {
    if let Some(control) = control {
        state.doctrine.direct.insert(fleet, control);
    }
}

/// Whether a Deputy command must be dropped because it is for a fleet under direct control
/// (RK-10, D-200). The opted-in retreat passes.
pub(crate) fn blocks_deputy(state: &UniverseState, command: &Command) -> bool {
    let direct = |fleet: &FleetId| state.doctrine.direct.get(fleet).copied();
    match command {
        Command::Fleet { fleet, order, .. } => match direct(fleet) {
            Some(control) => !(control.retreat && matches!(order, FleetOrder::Retreat { .. })),
            None => false,
        },
        Command::CreateFleet { ships } => ships
            .iter()
            .any(|&ship| control_of_ship(state, Some(ship)).is_some()),
        Command::MergeFleets { fleet, into } => direct(fleet).is_some() || direct(into).is_some(),
        Command::Economy(
            EconomyCommand::Convoy { fleet, .. }
            | EconomyCommand::LoadCoreKit { fleet }
            | EconomyCommand::DeployCoreKit { fleet, .. },
        ) => direct(fleet).is_some(),
        Command::Holding { .. }
        | Command::Economy(_)
        | Command::SetExposureWindow { .. }
        | Command::Doctrine(_) => false,
    }
}

/// The grace of `commander` ended (D-162): every fleet of theirs leaves direct control.
/// Returns those fleets, in identity order.
pub(crate) fn lapse_all(state: &mut UniverseState, commander: CommanderId) -> Vec<FleetId> {
    let owner = Owner::Commander(commander);
    let lapsed: Vec<FleetId> = state
        .doctrine
        .direct
        .keys()
        .copied()
        .filter(|fleet| {
            state
                .entities
                .with::<Fleet, _>(fleet.sim_id(), |f| f.owner == owner)
                == Some(true)
        })
        .collect();
    for fleet in &lapsed {
        state.doctrine.direct.remove(fleet);
    }
    lapsed
}

/// End of step: drops direct control of fleets that are gone and overrides that no longer
/// apply.
pub(crate) fn prune(state: &mut UniverseState) {
    let entities = &state.entities;
    let alive = |fleet: &FleetId| entities.has::<Fleet>(fleet.sim_id());
    state.doctrine.direct.retain(|fleet, _| alive(fleet));
    let holdings = &state.holdings;
    for (me, doctrine) in &mut state.doctrine.commanders {
        doctrine.fleets.retain(|fleet, _| alive(fleet));
        doctrine
            .holdings
            .retain(|holding, _| holdings.get(holding).is_some_and(|h| h.owner == *me));
    }
}
