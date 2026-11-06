//! Projectiles in flight (doc 15 §4; S5 plan §4.3).
//!
//! Projectiles are short-lived and numerous, so they are not `hecs` entities: they live in
//! one list in [`ProjectileId`] order, and the id takes the place doc 15 gives the
//! "projectile `SimId`" in the damage order. Each tick a projectile's swept segment is tested
//! against ship circles (D-143) through the combat grid; the first hostile ship along the
//! segment is hit. Flak shells instead burst at their aim point after their flight time.

use nomad_types::Owner;
use serde::{Deserialize, Serialize};

use super::{Hit, ShipIndex, hostile};
use crate::fleet::movement::DT;
use crate::fleet::{ShipClass, ShipId};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::rules::combat::{SizeClass, TargetClass, WeaponRules};
use crate::state::UniverseState;

/// Identifies a projectile: a Universe-wide counter, never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProjectileId(pub u64);

/// Every projectile in flight, in id order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Projectiles {
    pub(crate) next_id: u64,
    pub(crate) flying: Vec<Projectile>,
}

/// One projectile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Projectile {
    pub(crate) id: ProjectileId,
    pub(crate) system: SystemId,
    pub(crate) owner: Owner,
    /// The ship that fired it (it may be gone by the time it hits).
    pub(crate) shooter: ShipId,
    /// The shooter's class and the weapon's index there, which give its numbers.
    pub(crate) class: ShipClass,
    pub(crate) weapon: u8,
    pub(crate) position: Vec2,
    pub(crate) velocity: Vec2,
    /// Distance it may still fly before it expires.
    pub(crate) remaining_m: f64,
    /// Flak: ticks until it bursts, and where.
    pub(crate) burst: Option<Burst>,
}

/// When and where a flak shell bursts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Burst {
    pub(crate) ticks: u32,
    pub(crate) at: Vec2,
}

impl Projectiles {
    /// Adds a projectile and returns its id.
    pub(crate) fn launch(&mut self, mut projectile: Projectile) -> ProjectileId {
        let id = ProjectileId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        projectile.id = id;
        self.flying.push(projectile);
        id
    }
}

/// Phase 7, step 2: moves every projectile one tick and returns the hits.
pub(crate) fn advance(state: &mut UniverseState, index: &ShipIndex) -> Vec<Hit> {
    let mut hits = Vec::new();
    let flying = std::mem::take(&mut state.projectiles.flying);
    let mut kept = Vec::with_capacity(flying.len());
    for mut p in flying {
        let Some(weapon) = weapon_of(state, p.class, p.weapon).cloned() else {
            continue;
        };
        let keep = match p.burst {
            Some(mut burst) => {
                p.position += p.velocity * DT;
                burst.ticks = burst.ticks.saturating_sub(1);
                if burst.ticks == 0 {
                    flak_burst(state, index, &p, &weapon, burst.at, &mut hits);
                    false
                } else {
                    p.burst = Some(burst);
                    true
                }
            }
            None => fly(state, index, &mut p, &weapon, &mut hits),
        };
        if keep {
            kept.push(p);
        }
    }
    state.projectiles.flying = kept;
    hits
}

/// The weapon a projectile came from.
pub(crate) fn weapon_of(
    state: &UniverseState,
    class: ShipClass,
    weapon: u8,
) -> Option<&WeaponRules> {
    state
        .rules
        .combat
        .class(class)
        .weapons
        .get(usize::from(weapon))
}

/// Moves a bolt or torpedo one tick; returns whether it is still flying.
fn fly(
    state: &UniverseState,
    index: &ShipIndex,
    p: &mut Projectile,
    weapon: &WeaponRules,
    hits: &mut Vec<Hit>,
) -> bool {
    let step = p.velocity * DT;
    let length = step.length().min(p.remaining_m);
    let Some(direction) = step.normalized() else {
        return false;
    };
    let segment = direction * length;
    if let Some(ships) = index.systems.get(&p.system) {
        let largest = state
            .rules
            .combat
            .classes
            .values()
            .map(|c| c.hit_radius_m)
            .fold(0.0, f64::max);
        let mut first: Option<(f64, ShipId, SizeClass)> = None;
        for i in ships.near(p.position, length + largest) {
            let entry = ships.entries[i];
            // Friendly ships let it through (D-146); shrouded ones cannot be hit (D-055).
            if !hostile(p.owner, entry.owner) || entry.shrouded {
                continue;
            }
            let combat = state.rules.combat.class(entry.class);
            if let Some(t) = sweep(p.position, segment, entry.position, combat.hit_radius_m) {
                let better = first
                    .is_none_or(|(best, id, _)| t.total_cmp(&best).then(entry.id.cmp(&id)).is_lt());
                if better {
                    first = Some((t, entry.id, combat.size));
                }
            }
        }
        if let Some((_, target, size)) = first {
            hits.push(Hit {
                target,
                projectile: p.id,
                amount: state
                    .rules
                    .combat
                    .damage_by(p.owner, weapon, TargetClass::Ship(size)),
                owner: p.owner,
                shooter: p.shooter,
            });
            return false;
        }
    }
    p.position += segment;
    p.remaining_m -= length;
    p.remaining_m > 0.0
}

/// A flak burst at `at`: every hostile Light and Medium ship whose centre lies within the
/// burst radius takes the flak's damage (doc 15 §4; no friendly fire, D-146).
fn flak_burst(
    state: &UniverseState,
    index: &ShipIndex,
    p: &Projectile,
    weapon: &WeaponRules,
    at: Vec2,
    hits: &mut Vec<Hit>,
) {
    let Some(radius) = weapon.burst_radius_m else {
        return;
    };
    let Some(ships) = index.systems.get(&p.system) else {
        return;
    };
    let mut caught: Vec<(ShipId, SizeClass)> = ships
        .near(at, radius)
        .map(|i| ships.entries[i])
        .filter(|e| hostile(p.owner, e.owner) && !e.shrouded)
        .filter(|e| (e.position - at).length() <= radius)
        .filter_map(|e| {
            let size = state.rules.combat.class(e.class).size;
            matches!(size, SizeClass::Light | SizeClass::Medium).then_some((e.id, size))
        })
        .collect();
    caught.sort_unstable_by_key(|&(id, _)| id);
    for (target, size) in caught {
        hits.push(Hit {
            target,
            projectile: p.id,
            amount: state
                .rules
                .combat
                .damage_by(p.owner, weapon, TargetClass::Ship(size)),
            owner: p.owner,
            shooter: p.shooter,
        });
    }
}

/// Where along `segment` from `start` it first touches the circle at `centre` with
/// `radius`, as a fraction from 0 to 1, or `None` if it does not. A segment that starts
/// inside the circle touches it at 0.
pub(crate) fn sweep(start: Vec2, segment: Vec2, centre: Vec2, radius: f64) -> Option<f64> {
    // |offset + t × segment|² = radius², a quadratic in t.
    let offset = start - centre;
    let constant = offset.dot(offset) - radius * radius;
    if constant <= 0.0 {
        return Some(0.0);
    }
    let quadratic = segment.dot(segment);
    if quadratic <= 0.0 {
        return None;
    }
    let linear = 2.0 * offset.dot(segment);
    let discriminant = linear * linear - 4.0 * quadratic * constant;
    if discriminant < 0.0 {
        return None;
    }
    let first = (-linear - libm::sqrt(discriminant)) / (2.0 * quadratic);
    (0.0..=1.0).contains(&first).then_some(first)
}
