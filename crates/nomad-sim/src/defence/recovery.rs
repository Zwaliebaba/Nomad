//! Recovery of disabled structures (doc 12 §3; D-149).
//!
//! A disabled structure comes back online once no hostile ship has been inside its holding's
//! ops area (20 km, D-103) for 10 minutes, with at least 10% of its hull, and is then
//! repaired normally (D-071). REPAIR brings it back at once, with the same hull. The clock
//! starts when it was disabled or when the last hostile was seen, whichever is later. Every
//! ship counts as visible until S7.
//!
//! The check runs once a second per holding, in the holding's economy slot (doc 11 §4.1),
//! and does nothing at holdings without disabled structures.

use nomad_types::Owner;

use crate::combat::{ShipIndex, hostile};
use crate::economy::{self, Disabled, EconomyEvent, Hull};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::SimId;
use crate::state::UniverseState;
use crate::time::{Tick, Ticks};

/// Recovery at `holding`, in its economy slot.
pub(crate) fn run(
    state: &mut UniverseState,
    now: Tick,
    index: &ShipIndex,
    holding: HoldingId,
    events: &mut Vec<EconomyEvent>,
) {
    {
        let disabled: Vec<(SimId, Tick)> = state
            .economy
            .holdings
            .get(&holding)
            .map(|e| {
                e.slots
                    .values()
                    .filter_map(|&id| state.entities.get::<Disabled>(id).map(|d| (id, d.since)))
                    .collect()
            })
            .unwrap_or_default();
        if disabled.is_empty() {
            return;
        }
        if hostile_in_ops_area(state, index, holding) {
            state
                .defence
                .holdings
                .entry(holding)
                .or_default()
                .last_hostile = Some(now);
        }
        let last_hostile = state
            .defence
            .holdings
            .get(&holding)
            .and_then(|d| d.last_hostile);
        let repairing = state
            .economy
            .holdings
            .get(&holding)
            .is_some_and(|e| e.repairing);
        let quiet = Ticks::from_secs(state.rules.defence.recovery.quiet_s);
        for (id, since) in disabled {
            let from = last_hostile.map_or(since, |t| t.max(since));
            if repairing || now >= from.after(quiet) {
                restore(state, id, events);
            }
        }
    }
}

/// Whether a ship hostile to `holding`'s owner is in space inside its ops area.
fn hostile_in_ops_area(state: &UniverseState, index: &ShipIndex, holding: HoldingId) -> bool {
    let Some(h) = state.holdings.get(&holding) else {
        return false;
    };
    let Some(site) = state.galaxy.site(h.site) else {
        return false;
    };
    let Some(ships) = index.systems.get(&site.system) else {
        return false;
    };
    let owner = Owner::Commander(h.owner);
    let centre = Vec2::from(site.position);
    let radius = state.rules.movement.ops_area_radius_m;
    ships.near(centre, radius).any(|i| {
        let entry = &ships.entries[i];
        hostile(owner, entry.owner) && (entry.position - centre).length() <= radius
    })
}

/// Brings module `id` back online with at least the recovery hull (D-149).
fn restore(state: &mut UniverseState, id: SimId, events: &mut Vec<EconomyEvent>) {
    let percent = state.rules.defence.recovery.hull_percent;
    if let Some(hull) = state.entities.get_mut::<Hull>(id) {
        let floor =
            u32::try_from(u64::from(hull.max) * u64::from(percent) / 100).unwrap_or(hull.max);
        hull.hp = hull.hp.max(floor);
    }
    economy::restore_module(state, id, events);
}
