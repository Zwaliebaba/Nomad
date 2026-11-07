//! Turrets and STOP (doc 15 §5.3, §12; D-128, D-144; S5 plan §4.8).
//!
//! A turret is a module with a Turret-class weapon. It fires while it is complete, not
//! disabled, powered (S4 gives it its full 40 MW or nothing, D-128) and its holding's STOP is
//! not set; otherwise it drops its target and lock. It takes the auto-target rule (D-144):
//! the hostile ship in range that its owner detects (S7) with the highest multiplier, then
//! the nearest, then the lowest `SimId`, kept while it stays valid. Its shots start at the
//! module and pass outward through the holding's own dome (D-053). Lock, spread and lead aim
//! work as for ships' weapons ([`crate::combat`]); turrets fire after the ships, holding by
//! holding and in module `SimId` order, and draw from the same combat stream of the system
//! (D-049).

use std::collections::BTreeSet;

use nomad_types::{ModuleKind, Owner};

use crate::combat::{
    Aim, Gun, ShipEntry, ShipIndex, Shooter, Shot, WeaponSlot, hostile, launch, millis, next_ready,
};
use crate::economy::Module;
use crate::holding::HoldingId;
use crate::id::SimId;
use crate::rules::combat::TargetClass;
use crate::sensors::Seen;
use crate::state::UniverseState;
use crate::time::{TICK_MILLIS, Tick};

/// The turret modules of `holding`, in `SimId` order.
fn turrets_of(state: &UniverseState, holding: HoldingId) -> Vec<SimId> {
    let mut ids: Vec<SimId> = state
        .economy
        .holdings
        .get(&holding)
        .map(|e| {
            e.slots
                .values()
                .copied()
                .filter(|&id| state.module_kind(id) == Some(ModuleKind::Turret))
                .collect()
        })
        .unwrap_or_default();
    ids.sort_unstable();
    ids
}

/// Fires every turret, holding by holding. Only holdings in systems with ships in space have
/// anything to aim at; turrets elsewhere drop their targets and lock.
pub(crate) fn run(state: &mut UniverseState, now: Tick, index: &ShipIndex, seen: &Seen) {
    state
        .defence
        .turrets
        .retain(|&id, _| state.entities.has::<Module>(id));
    let in_play: BTreeSet<HoldingId> = state
        .holdings
        .iter()
        .filter(|(_, h)| {
            state
                .galaxy
                .site(h.site)
                .is_some_and(|site| index.systems.contains_key(&site.system))
        })
        .map(|(&id, _)| id)
        .collect();
    for &holding in &in_play {
        for turret in turrets_of(state, holding) {
            fire_turret(state, now, index, seen, holding, turret);
        }
    }
    let idle: Vec<SimId> = state
        .defence
        .turrets
        .iter()
        .filter(|&(&id, slot)| {
            slot.target.is_some()
                && state
                    .entities
                    .with::<Module, _>(id, |m| !in_play.contains(&m.holding))
                    .unwrap_or(true)
        })
        .map(|(&id, _)| id)
        .collect();
    for id in idle {
        if let Some(slot) = state.defence.turrets.get(&id).copied() {
            store(
                state,
                id,
                WeaponSlot {
                    target: None,
                    lock_ms: 0,
                    ..slot
                },
            );
        }
    }
}

/// Whether turret `id` of `holding` may fire: working, powered, and STOP not set.
fn may_fire(state: &UniverseState, holding: HoldingId, id: SimId) -> bool {
    state.is_working(id)
        && state
            .economy
            .holdings
            .get(&holding)
            .is_some_and(|e| e.allocation.powered(id))
        && !state
            .defence
            .holdings
            .get(&holding)
            .is_some_and(|d| d.turrets_hold)
}

fn fire_turret(
    state: &mut UniverseState,
    now: Tick,
    index: &ShipIndex,
    seen: &Seen,
    holding: HoldingId,
    id: SimId,
) {
    let mut slot = state.defence.turrets.get(&id).copied().unwrap_or_default();
    let place = state.module_position(id);
    let owner = state
        .holdings
        .get(&holding)
        .map(|h| Owner::Commander(h.owner));
    let (Some((system, position, _)), Some(owner), true) =
        (place, owner, may_fire(state, holding, id))
    else {
        // It may not fire: it drops its target and lock (as ships' weapons do).
        slot.target = None;
        slot.lock_ms = 0;
        store(state, id, slot);
        return;
    };
    let position = position.into();
    let rules = state.rules.defence.turret.clone();
    let weapon = &rules.weapon;
    let valid = |t: &ShipEntry| {
        hostile(owner, t.owner)
            && !t.shrouded
            && (t.position - position).length() <= weapon.range_m
            && seen.ship(owner, system, t.id)
    };
    let current = slot
        .ship_target()
        .and_then(|s| index.get(s))
        .filter(|&(sys, t)| sys == system && valid(t))
        .map(|(_, t)| t.id);
    let target = current.or_else(|| {
        let ships = index.systems.get(&system)?;
        ships
            .near(position, weapon.range_m)
            .map(|j| &ships.entries[j])
            .filter(|t| valid(t))
            .map(|t| {
                let size = state.rules.combat.class(t.class).size;
                let multiplier = state
                    .rules
                    .combat
                    .multipliers(weapon.kind)
                    .against(TargetClass::Ship(size));
                (multiplier, (t.position - position).length(), t.id)
            })
            .min_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)))
            .map(|(_, _, ship)| ship)
    });
    let target = target.map(Aim::Ship);
    if target != slot.target {
        slot.lock_ms = 0;
    }
    slot.target = target;
    let Some(Aim::Ship(ship)) = target else {
        store(state, id, slot);
        return;
    };
    slot.lock_ms = (slot.lock_ms + TICK_MILLIS).min(rules.lock_ms);
    let now_ms = millis(now);
    if slot.ready_at_ms <= now_ms
        && let Some((_, t)) = index.get(ship)
    {
        let lock = slot.lock_ms as f64 / rules.lock_ms as f64;
        let (at, velocity) = (t.position, t.velocity);
        launch(
            state,
            system,
            weapon,
            Shot {
                owner,
                shooter: Shooter::Module(id),
                gun: Gun::Turret,
                from: position,
                at,
                velocity,
                lock,
            },
        );
        slot.ready_at_ms = next_ready(slot.ready_at_ms, now_ms, weapon.interval_ms);
    }
    store(state, id, slot);
}

/// Keeps a turret's weapon state, or drops it when idle and ready.
fn store(state: &mut UniverseState, id: SimId, slot: WeaponSlot) {
    if slot == WeaponSlot::default() {
        state.defence.turrets.remove(&id);
    } else {
        state.defence.turrets.insert(id, slot);
    }
}

/// `holding`'s working turrets, and how many of them are powered.
pub(crate) fn counts(state: &UniverseState, holding: HoldingId) -> (u32, u32) {
    let working: Vec<SimId> = turrets_of(state, holding)
        .into_iter()
        .filter(|&id| state.is_working(id))
        .collect();
    let powered = working
        .iter()
        .filter(|&&id| {
            state
                .economy
                .holdings
                .get(&holding)
                .is_some_and(|e| e.allocation.powered(id))
        })
        .count();
    let n = |count: usize| u32::try_from(count).unwrap_or(u32::MAX);
    (n(working.len()), n(powered))
}

/// How many of `holding`'s turrets have a target ("2 FIRING").
pub(crate) fn firing(state: &UniverseState, holding: HoldingId) -> u32 {
    let count = turrets_of(state, holding)
        .into_iter()
        .filter(|id| {
            state
                .defence
                .turrets
                .get(id)
                .is_some_and(|s| s.target.is_some())
        })
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}
