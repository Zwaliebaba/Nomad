//! Targeting, lock and firing (doc 15 §4; S5 plan §4.2; D-144, D-145).
//!
//! Weapons are processed system by system, then in `(ship SimId, weapon index)` order, and
//! the only random draws (the spread) come from the system's combat stream in that order
//! (D-049).
//!
//! **Target choice per weapon:**
//!
//! - an Attack order's target (a ship, or any ship of a target fleet) when it is in the
//!   weapon's range; an Attack target that comes into range replaces another target;
//! - otherwise, under *weapons free* or during an Attack-move, the hostile ship in range with
//!   the highest multiplier for this weapon, then the nearest, then the lowest `SimId`
//!   (D-144); under *return fire* the same, among ships that hit the fleet in the last
//!   30 s (D-145); under *hold fire* nothing.
//!
//! A weapon keeps its target until the target leaves range, is destroyed, is shrouded, or
//! is no longer allowed, so lock is not thrown away. Flak only takes Light and Medium
//! targets, the only ones its bursts damage (doc 15 §4).
//!
//! **Lock** rises by one tick's worth each tick on the same target, up to the class's lock
//! time, and resets on a target change (doc 15 §4). An allied HSTL's ranging laser on the
//! target makes it rise 50% faster: the laser designates the target of the cruiser's first
//! weapon while that target is within the laser's range. **Spread** narrows with lock.
//! **Fire control** aims with linear lead (*Derived* from doc 15 §4), and a shot's direction
//! is drawn uniformly across the spread cone. A weapon fires at most once a tick, and its
//! average rate is exactly its interval, without bursts after it was idle.

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{FleetOrder, Owner, RulesOfEngagement};

use super::engagement::recent_attackers;
use super::projectile::{Burst, Projectile, ProjectileId};
use super::{ShipEntry, ShipIndex, WeaponSlot, Weapons, hostile, millis};
use crate::fleet::movement::DT;
use crate::fleet::{Fleet, FleetId, MotionMode, ShipId, Task};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::rng::{StreamKey, purpose};
use crate::rules::combat::{SizeClass, TargetClass, WeaponKind, WeaponRules};
use crate::state::UniverseState;
use crate::time::{TICK_MILLIS, Tick};

/// What a fleet's weapons may shoot at this tick.
pub(crate) struct Permission {
    /// Ships its Attack order names, preferred when in range.
    preferred: BTreeSet<ShipId>,
    /// Which other hostiles it may choose itself.
    auto: Auto,
}

enum Auto {
    /// Any hostile (weapons free, or an Attack-move).
    Any,
    /// Only these (return fire, D-145).
    Only(BTreeSet<ShipId>),
}

impl Permission {
    fn allows(&self, ship: ShipId) -> bool {
        self.preferred.contains(&ship)
            || match &self.auto {
                Auto::Any => true,
                Auto::Only(set) => set.contains(&ship),
            }
    }
}

/// Phase 7, step 3.
pub(crate) fn run(state: &mut UniverseState, now: Tick, index: &ShipIndex) {
    let designated = designations(state, index);
    // What each fleet may fire at, once per tick: it does not change while weapons fire.
    let mut permissions: BTreeMap<FleetId, Option<Permission>> = BTreeMap::new();
    for ships in index.systems.values() {
        for entry in &ships.entries {
            permissions
                .entry(entry.fleet)
                .or_insert_with(|| may_fire(state, entry.fleet));
        }
    }
    for (&system, ships) in &index.systems {
        for entry in &ships.entries {
            let permission = if entry.shrouded {
                None
            } else {
                permissions.get(&entry.fleet).and_then(Option::as_ref)
            };
            fire_ship(state, now, index, system, entry, permission, &designated);
        }
    }
}

/// Targets designated by ranging lasers, with the laser's owner (doc 15 §4): the target of
/// an HSTL's first weapon, while it is within the laser's range. From the weapon state at the
/// start of the step, so the result does not depend on processing order.
fn designations(state: &UniverseState, index: &ShipIndex) -> BTreeSet<(Owner, ShipId)> {
    let mut designated = BTreeSet::new();
    for ships in index.systems.values() {
        for entry in &ships.entries {
            let Some(laser) = state.rules.combat.class(entry.class).ranging_laser_m else {
                continue;
            };
            if entry.shrouded {
                continue;
            }
            let target = state
                .entities
                .with::<Weapons, _>(entry.id.sim_id(), |w| {
                    w.slots.first().and_then(|s| s.target)
                })
                .flatten();
            if let Some(target) = target
                && let Some((_, t)) = index.get(target)
                && (t.position - entry.position).length() <= laser
            {
                designated.insert((entry.owner, target));
            }
        }
    }
    designated
}

/// What `fleet` may fire at this tick, or `None` if it may not fire: it cruises, holds fire
/// or does not exist.
pub(crate) fn may_fire(state: &UniverseState, fleet: FleetId) -> Option<Permission> {
    let f = state.entities.get::<Fleet>(fleet.sim_id())?;
    if state.fleet_mode(fleet)? == MotionMode::Cruise {
        return None;
    }
    let attack_move = matches!(
        f.active.as_ref().map(|a| &a.order),
        Some(FleetOrder::AttackMove { .. })
    );
    let auto = match f.rules_of_engagement {
        RulesOfEngagement::HoldFire => return None,
        RulesOfEngagement::WeaponsFree => Auto::Any,
        RulesOfEngagement::ReturnFire if attack_move => Auto::Any,
        RulesOfEngagement::ReturnFire => Auto::Only(recent_attackers(state, fleet)),
    };
    let preferred = match f.active.as_ref().map(|a| &a.task) {
        Some(Task::Attack { target }) => {
            if let Some(members) = state
                .entities
                .with::<Fleet, _>(*target, |t| t.members.keys().copied().collect())
            {
                members
            } else if state.entities.has::<crate::fleet::Ship>(*target) {
                BTreeSet::from([ShipId(*target)])
            } else {
                // A holding or another object: S5b.
                BTreeSet::new()
            }
        }
        _ => BTreeSet::new(),
    };
    Some(Permission { preferred, auto })
}

fn fire_ship(
    state: &mut UniverseState,
    now: Tick,
    index: &ShipIndex,
    system: SystemId,
    shooter: &ShipEntry,
    permission: Option<&Permission>,
    designated: &BTreeSet<(Owner, ShipId)>,
) {
    let class = state.rules.combat.class(shooter.class).clone();
    if class.weapons.is_empty() {
        return;
    }
    let id = shooter.id.sim_id();
    let stored = state.entities.get::<Weapons>(id);
    let Some(permission) = permission else {
        // It may not fire: weapons drop their targets and lock.
        if stored.is_some() {
            let slots: Vec<WeaponSlot> = stored
                .map(|w| w.slots)
                .unwrap_or_default()
                .into_iter()
                .map(|s| WeaponSlot {
                    target: None,
                    lock_ms: 0,
                    ..s
                })
                .collect();
            store(state, shooter.id, slots);
        }
        return;
    };
    let mut slots = stored.map_or_else(Vec::new, |w| w.slots);
    slots.resize(class.weapons.len(), WeaponSlot::default());
    let Some(ships) = index.systems.get(&system) else {
        return;
    };
    let now_ms = millis(now);
    for (i, weapon) in class.weapons.iter().enumerate() {
        let slot = &mut slots[i];
        let valid = |target: ShipId| {
            index.get(target).is_some_and(|(sys, t)| {
                sys == system && can_target(shooter, t, weapon, state) && permission.allows(target)
            })
        };
        let preferred_in_range = || {
            best(
                state,
                shooter,
                weapon,
                permission
                    .preferred
                    .iter()
                    .filter_map(|&s| index.get(s))
                    .filter(|&(sys, _)| sys == system)
                    .map(|(_, t)| t),
            )
        };
        let mut target = slot.target.filter(|&t| valid(t));
        if let Some(current) = target
            && !permission.preferred.contains(&current)
            && let Some(p) = preferred_in_range()
        {
            target = Some(p);
        }
        if target.is_none() {
            target = preferred_in_range().or_else(|| {
                let candidates = ships
                    .near(shooter.position, weapon.range_m)
                    .map(|j| &ships.entries[j])
                    .filter(|t| permission.allows(t.id));
                best(state, shooter, weapon, candidates)
            });
        }
        if target != slot.target {
            slot.lock_ms = 0;
        }
        slot.target = target;
        let Some(target) = target else {
            continue;
        };
        let rate = if designated.contains(&(shooter.owner, target)) {
            u64::from(state.rules.combat.ranging_laser_lock_rate_percent)
        } else {
            100
        };
        slot.lock_ms = (slot.lock_ms + TICK_MILLIS * rate / 100).min(class.lock_ms);
        if slot.ready_at_ms <= now_ms {
            let Some((_, t)) = index.get(target) else {
                continue;
            };
            let lock = slot.lock_ms as f64 / class.lock_ms as f64;
            launch(state, system, shooter, t, i, weapon, lock);
            // At most one shot a tick. While firing steadily, the next shot is due one
            // interval after this one was due, so the average rate is exact; a weapon that
            // was idle (it could have fired a tick ago) starts its cycle now, without a burst.
            let steady = slot.ready_at_ms > now_ms.saturating_sub(TICK_MILLIS);
            let base = if steady { slot.ready_at_ms } else { now_ms };
            slot.ready_at_ms = base + weapon.interval_ms;
        }
    }
    store(state, shooter.id, slots);
}

/// Keeps a ship's weapon state, or drops it when every weapon is idle and ready.
fn store(state: &mut UniverseState, ship: ShipId, slots: Vec<WeaponSlot>) {
    if slots.iter().all(|s| *s == WeaponSlot::default()) {
        state.entities.remove::<Weapons>(ship.sim_id());
    } else {
        state.entities.insert(ship.sim_id(), Weapons { slots });
    }
}

/// Whether `shooter`'s `weapon` can fire at `target` now: hostile, not shrouded (D-055), in
/// range, and, for flak, Light or Medium.
fn can_target(
    shooter: &ShipEntry,
    target: &ShipEntry,
    weapon: &WeaponRules,
    state: &UniverseState,
) -> bool {
    hostile(shooter.owner, target.owner)
        && !target.shrouded
        && (target.position - shooter.position).length() <= weapon.range_m
        && (weapon.kind != WeaponKind::Flak
            || matches!(
                state.rules.combat.class(target.class).size,
                SizeClass::Light | SizeClass::Medium
            ))
}

/// The best target among `candidates` (D-144): the highest multiplier for `weapon`, then the
/// nearest, then the lowest `SimId`. Candidates it cannot fire at are skipped.
fn best<'a>(
    state: &UniverseState,
    shooter: &ShipEntry,
    weapon: &WeaponRules,
    candidates: impl Iterator<Item = &'a ShipEntry>,
) -> Option<ShipId> {
    candidates
        .filter(|t| can_target(shooter, t, weapon, state))
        .map(|t| {
            let size = state.rules.combat.class(t.class).size;
            let multiplier = state
                .rules
                .combat
                .multipliers(weapon.kind)
                .against(TargetClass::Ship(size));
            let distance = (t.position - shooter.position).length();
            (multiplier, distance, t.id)
        })
        .min_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)))
        .map(|(_, _, id)| id)
}

/// Fires weapon `index` of `shooter` at `target` with lock `lock` (0 to 1).
fn launch(
    state: &mut UniverseState,
    system: SystemId,
    shooter: &ShipEntry,
    target: &ShipEntry,
    index: usize,
    weapon: &WeaponRules,
    lock: f64,
) {
    let (aim, distance) = lead(
        shooter.position,
        target.position,
        target.velocity,
        weapon.speed_m_per_s,
    );
    let key = StreamKey {
        scope: system.sim_id().get(),
        purpose: purpose::COMBAT,
    };
    let spread = weapon.spread_rad(lock);
    let offset = (state.rng.unit_f64(key) - 0.5) * spread;
    let direction = Vec2::from_angle(aim + offset);
    let burst = weapon.burst_radius_m.map(|_| {
        let distance = distance.min(weapon.range_m);
        let per_tick = weapon.speed_m_per_s * DT;
        Burst {
            ticks: flight_ticks(distance, per_tick),
            at: shooter.position + direction * distance,
        }
    });
    state.projectiles.launch(Projectile {
        id: ProjectileId(0),
        system,
        owner: shooter.owner,
        shooter: shooter.id,
        class: shooter.class,
        weapon: u8::try_from(index).unwrap_or(u8::MAX),
        position: shooter.position,
        velocity: direction * weapon.speed_m_per_s,
        remaining_m: weapon.range_m,
        burst,
    });
}

/// Ticks a shell needs to fly `distance` at `per_tick` metres a tick; at least one.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a flight within a weapon's range takes a few hundred ticks at most"
)]
fn flight_ticks(distance: f64, per_tick: f64) -> u32 {
    (libm::ceil(distance / per_tick) as u32).max(1)
}

/// Linear-lead fire control: the direction (radians counter-clockwise from east) in which a
/// projectile at `speed` from `from` meets a target at `at` moving with `velocity`, and the
/// distance to that point. If no interception is possible it aims at the target's present
/// position.
pub(crate) fn lead(from: Vec2, at: Vec2, velocity: Vec2, speed: f64) -> (f64, f64) {
    let d = at - from;
    let a = velocity.dot(velocity) - speed * speed;
    let b = 2.0 * d.dot(velocity);
    let c = d.dot(d);
    let time = if a.abs() < 1e-9 {
        (b < 0.0).then(|| -c / b)
    } else {
        let discriminant = b * b - 4.0 * a * c;
        (discriminant >= 0.0)
            .then(|| {
                let root = libm::sqrt(discriminant);
                let t1 = (-b - root) / (2.0 * a);
                let t2 = (-b + root) / (2.0 * a);
                [t1, t2].into_iter().filter(|t| *t > 0.0).reduce(f64::min)
            })
            .flatten()
    };
    let point = time.map_or(at, |t| at + velocity * t);
    let to = point - from;
    (to.angle(), to.length())
}
