//! Scenario fixtures for the harness and integration tests: fleets placed directly in space
//! or docked, holdings and modules placed without the economy, a dome sector opened as a
//! siege would, and outposts deployed without a MULE trip.
//!
//! **Not a game rule.** In a season, ships come from the start kit and later from shipyards
//! (S4). Measurement runs such as the time-to-kill scenarios (D-058; S5 plan §7 task 7) need
//! fleets of a chosen composition at a chosen place, so this module puts them there without
//! the economy. Nothing in the simulation calls it.

use std::collections::{BTreeMap, VecDeque};

use nomad_types::{ModuleKind, Owner, Slot};

use crate::fleet::{
    DEFAULT_RULES_OF_ENGAGEMENT, Fleet, FleetId, Motion, Place, Ship, ShipClass, ShipId,
    formation_offsets, slot_position,
};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::SimId;
use crate::state::UniverseState;

/// Places a new fleet of `classes` for `owner` at rest in space in `system`, its anchor at
/// `centre`, in the default formation, facing `heading_rad` (counter-clockwise from east).
/// The fleet has no orders and the default rules of engagement. Observers detect it from
/// the next sensors pass, or at once after [`sense`].
pub fn place_fleet(
    state: &mut UniverseState,
    owner: Owner,
    system: SystemId,
    centre: Vec2,
    heading_rad: f64,
    classes: &[ShipClass],
) -> FleetId {
    let id = FleetId(state.ids.allocate());
    let anchor = Motion {
        position: centre.into(),
        velocity: Vec2::ZERO,
        heading_rad,
    };
    let offsets = formation_offsets(classes.len(), state.rules.movement.formation_spacing_m);
    let mut members = BTreeMap::new();
    for (&class, offset) in classes.iter().zip(offsets) {
        let ship = ShipId(state.ids.allocate());
        members.insert(ship, offset);
        state.entities.spawn(
            ship.sim_id(),
            (
                Ship {
                    class,
                    owner,
                    fleet: id,
                },
                Motion {
                    position: slot_position(&anchor, offset).into(),
                    ..anchor
                },
            ),
        );
    }
    state.entities.spawn(
        id.sim_id(),
        (
            Fleet {
                owner,
                place: Place::InSystem(system),
                members,
                rules_of_engagement: DEFAULT_RULES_OF_ENGAGEMENT,
                engaged_until: None,
                active: None,
                queue: VecDeque::new(),
            },
            anchor,
        ),
    );
    id
}

/// Runs a sensors pass now, as on a pass tick (S7), so fleets placed directly are in every
/// picture that would detect them, as if they had been there at the last pass.
pub fn sense(state: &mut UniverseState) {
    crate::sensors::refresh(state);
}

/// Opens `sector` of `holding`'s dome, as S6's siege will (doc 15 §5.1, §6): it neither
/// blocks nor regenerates, so hostile fire through it reaches the modules inside. For the
/// core-kill measurement (D-058, D-147), which needs a siege before S6 exists. Returns whether
/// the holding has a dome with that sector.
pub fn open_sector(state: &mut UniverseState, holding: HoldingId, sector: u8) -> bool {
    state.open_sector(holding, sector)
}

/// Places a completed module of `kind` in the free `slot` of `holding`, without paying for
/// or building it, for measurements with a chosen station (for example turrets in the
/// core-kill scenario, D-147). Returns the module, or `None` if the holding does not exist,
/// is an outpost, or the slot is taken or wrong for the module (D-130, D-131).
pub fn place_module(
    state: &mut UniverseState,
    holding: HoldingId,
    slot: Slot,
    kind: ModuleKind,
) -> Option<SimId> {
    let h = state.holdings.get(&holding)?;
    if !h.core_complete || kind == ModuleKind::Core || !state.slot_list(holding).contains(&slot) {
        return None;
    }
    let deposit = match slot {
        Slot::Deposit(i) => state
            .galaxy
            .site(h.site)?
            .deposits
            .get(usize::from(i))
            .map(|d| d.kind),
        _ => None,
    };
    let taken = state
        .economy
        .holdings
        .get(&holding)
        .is_some_and(|e| e.slots.contains_key(&slot));
    if taken || !crate::economy::fits(kind, slot, deposit) {
        return None;
    }
    Some(crate::economy::spawn_module(
        state, holding, slot, kind, None,
    ))
}

// S8: claims for the season runs.

/// Deploys an outpost of `owner` on the free, claimable `site`, as a core kit carried there
/// would be (D-054): its core builds for the rules' deployment time, and the economy completes
/// it into a holding, which the standings count (D-191). For `nomad-harness --season`, whose
/// scripted claims stand in for A2's expansion until it exists (S8 plan §7 task 6). Returns
/// `None` if the site cannot be claimed.
pub fn place_outpost(
    state: &mut UniverseState,
    owner: crate::id::CommanderId,
    site: crate::galaxy::SiteId,
) -> Option<HoldingId> {
    let holding = state.insert_holding(site, owner, false).ok()?;
    // Like a deployed kit, its hull grows from 10% over the deployment (D-224).
    crate::economy::spawn_outpost_core(state, holding);
    Some(holding)
}

/// Places a completed holding of `owner`, with its core, on the free, claimable `site`, as if
/// an outpost there had completed: it counts for control and the standings from the next
/// tick's phase 9 (D-041, D-191). For scenarios that start with commanders already spread out.
/// Returns `None` if the site cannot be claimed.
pub fn place_holding(
    state: &mut UniverseState,
    owner: crate::id::CommanderId,
    site: crate::galaxy::SiteId,
) -> Option<HoldingId> {
    let holding = state.insert_holding(site, owner, true).ok()?;
    crate::economy::spawn_module(state, holding, Slot::Core, ModuleKind::Core, None);
    Some(holding)
}

/// Places a new fleet of `classes` for `holding`'s owner, docked there, if the holding can take
/// every ship (D-102). Returns the fleet, or `None` if it cannot.
pub fn place_docked_fleet(
    state: &mut UniverseState,
    holding: HoldingId,
    classes: &[ShipClass],
) -> Option<FleetId> {
    let h = state.holdings.get(&holding)?;
    let owner = Owner::Commander(h.owner);
    let system = state.galaxy.site(h.site)?.system;
    let point = crate::fleet::station::launch_point(state, holding)?;
    let fleet = place_fleet(state, owner, system, point.into(), 0.0, classes);
    let docks = state
        .entities
        .get::<Fleet>(fleet.sim_id())
        .is_some_and(|f| crate::command::dock_check(state, &f, holding).is_ok());
    if !docks {
        for ship in state.fleet(fleet).map(|f| f.ships).unwrap_or_default() {
            state.entities.despawn(ship.sim_id());
        }
        state.entities.despawn(fleet.sim_id());
        return None;
    }
    if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
        f.place = Place::Docked(holding);
    }
    Some(fleet)
}
