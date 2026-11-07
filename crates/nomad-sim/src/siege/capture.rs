//! Capture of a normal holding and the destruction of an outpost (doc 15 §7–§8; D-054,
//! D-057, D-163, D-168; S6 plan §4.5).
//!
//! **Capture** (D-057, D-168):
//! - the holding changes owner; the command caps follow, as they count completed holdings
//!   (D-129);
//! - the core is restored to 25% of its hull and every sector raised to at least 25%;
//!   nothing stays open or immune, and the damage ledger starts afresh;
//! - surviving modules transfer with it; queued and running builds are cancelled without a
//!   refund; the local buffers (ore, alloy and crystal waiting for a convoy) and stored core
//!   kits stay with the holding (*Proposed* for the kits: D-168 does not name them; they are
//!   stock at the holding, like its buffers);
//! - the old owner's standing orders at the holding go: its rally point, REPAIR and active
//!   ability draws, and STOP (*Derived*: they were the old owner's orders);
//! - the old owner's docked ships emergency-jump to their nearest holding ([`super::jump`]);
//! - the new owner is protected for 48 h.
//!
//! **An outpost** under construction whose core reaches 0 is destroyed at any time (D-054):
//! the holding is removed with its core, and its site is free again. A fallen mother station
//! waiting for a free site may then take it (D-164).

use nomad_types::ModuleKind;

use super::{Deadline, DefenceState, HoldingSiege, jump};
use crate::announce::{Announcement, AnnouncementKind, Recipients};
use crate::economy::{EconomyEvent, EntryState, Hull, Module, Stock};
use crate::holding::HoldingId;
use crate::id::{CommanderId, SimId};
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::time::{Tick, Ticks};

/// `to` captures the fallen `holding` on `now` (D-057, D-163, D-168).
pub(super) fn capture(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    to: CommanderId,
    out: &mut TickOutputs,
) {
    let Some(from) = state.holdings.get(&holding).map(|h| h.owner) else {
        return;
    };
    if let Some(h) = state.holdings.get_mut(&holding) {
        h.owner = to;
    }
    cancel_builds(state, holding, &mut out.economy_events);
    clear_standing_orders(state, holding);
    let rules = &state.rules.sieges;
    let (core_percent, sector_percent, protection) = (
        rules.capture_core_percent,
        rules.capture_sector_percent,
        Ticks::from_hours(u64::from(rules.capture_protection_hours)),
    );
    restore_core(state, holding, core_percent);
    state.restore_sectors(holding, sector_percent);
    end_siege(state, holding);
    let breachable_from = now.after(protection);
    state.sieges.holdings.insert(
        holding,
        HoldingSiege {
            state: DefenceState::Nominal,
            breachable_from,
        },
    );
    // The old owner's docked ships, now docked at someone else's holding (D-057).
    jump::docked_fleets_jump(state, now, holding, from, out);
    // The owner changed: the standings are counted again in phase 9 (D-191).
    crate::season::mark_dirty(state);
    out.announcements.push(Announcement {
        tick: now,
        to: Recipients::Public,
        kind: AnnouncementKind::Captured {
            holding,
            from,
            to,
            breachable_from,
        },
    });
}

/// Cancels every queued and running build at `holding` without a refund (D-168): a module
/// being built goes with its slot.
pub(super) fn cancel_builds(
    state: &mut UniverseState,
    holding: HoldingId,
    events: &mut Vec<EconomyEvent>,
) {
    let Some(queue) = state
        .economy
        .holdings
        .get_mut(&holding)
        .map(|e| std::mem::take(&mut e.queue))
    else {
        return;
    };
    for entry in queue {
        if let EntryState::Building {
            module: Some(module),
            ..
        } = entry.state
        {
            let slot = state.entities.get::<Module>(module).map(|m| m.slot);
            if let (Some(slot), Some(e)) = (slot, state.economy.holdings.get_mut(&holding)) {
                e.slots.remove(&slot);
            }
            state.defence.turrets.remove(&module);
            state.entities.despawn(module);
        }
        events.push(EconomyEvent::ItemCancelled {
            holding,
            item: entry.id,
            refund: Stock::ZERO,
        });
    }
}

/// Clears the old owner's standing orders at `holding`: the rally point, REPAIR, active
/// ability draws, STOP and FLAK SCREEN, turret aims and return-fire records, and the damage
/// ledger. Sectors close and the dome is no longer immune.
pub(super) fn clear_standing_orders(state: &mut UniverseState, holding: HoldingId) {
    let modules: Vec<SimId> = state
        .economy
        .holdings
        .get(&holding)
        .map(|e| e.slots.values().copied().collect())
        .unwrap_or_default();
    for module in modules {
        state.defence.turrets.remove(&module);
    }
    if let Some(e) = state.economy.holdings.get_mut(&holding) {
        e.rally = None;
        e.repairing = false;
        e.abilities.clear();
    }
    if let Some(d) = state.defence.holdings.get_mut(&holding) {
        d.immune = false;
        d.turrets_hold = false;
        d.flak_screen.firing = None;
        d.struck_by.clear();
        d.ledger = crate::defence::ledger::Ledger::default();
        for sector in &mut d.sectors {
            sector.open = false;
        }
    }
}

/// Raises `holding`'s core to `percent` of its hull (D-057).
fn restore_core(state: &mut UniverseState, holding: HoldingId, percent: u32) {
    let Some(core) = state
        .economy
        .holdings
        .get(&holding)
        .and_then(|e| e.slots.get(&nomad_types::Slot::Core).copied())
    else {
        return;
    };
    if let Some(hull) = state.entities.get_mut::<Hull>(core) {
        let restored = u32::try_from(u64::from(hull.max) * u64::from(percent.min(100)) / 100)
            .unwrap_or(hull.max);
        hull.hp = hull.hp.max(restored);
    }
}

/// Removes `holding`'s siege state and its deadlines.
pub(super) fn end_siege(state: &mut UniverseState, holding: HoldingId) {
    if let Some(record) = state.sieges.holdings.remove(&holding) {
        match record.state {
            DefenceState::Reinforced { exit, .. } => {
                state
                    .sieges
                    .deadlines
                    .remove(&(exit, Deadline::Exit(holding)));
            }
            DefenceState::Siege { ends, .. } => {
                state
                    .sieges
                    .deadlines
                    .remove(&(ends, Deadline::SiegeEnds(holding)));
            }
            DefenceState::Nominal => {}
        }
    }
}

/// The outpost `holding` lost its core under construction on `now`: it is destroyed, and its
/// site is free again (D-054).
pub(super) fn destroy_outpost(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    out: &mut TickOutputs,
) {
    let Some(h) = state.holdings.remove(&holding) else {
        return;
    };
    if let Some(e) = state.economy.holdings.remove(&holding) {
        for (slot, module) in e.slots {
            let kind = state
                .entities
                .get::<Module>(module)
                .map_or(ModuleKind::Core, |m| m.kind);
            state.defence.turrets.remove(&module);
            state.entities.despawn(module);
            out.economy_events.push(EconomyEvent::ModuleDestroyed {
                module,
                holding,
                kind,
                slot,
            });
        }
    }
    state.defence.holdings.remove(&holding);
    end_siege(state, holding);
    out.announcements.push(Announcement {
        tick: now,
        to: Recipients::Public,
        kind: AnnouncementKind::OutpostDestroyed {
            holding,
            site: h.site,
            owner: h.owner,
        },
    });
    // A site is free: a fallen mother station waiting for one may take it (D-164).
    jump::retry_waiting(state, now, out);
}
