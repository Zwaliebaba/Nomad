//! Phase 7 of the tick: ship combat (Design doc 15 §4; doc 11 §4.1; S5 plan §4.2–§4.4, S5a)
//! and station combat ([`crate::defence`], doc 15 §5; S5 plan §4.5–§4.8, S5b).
//!
//! Every tick, after movement:
//!
//! 1. **Domes** are kept up to date and regenerate from their allocated power
//!    ([`crate::defence`]).
//! 2. **Engagement** (D-114): a fleet with a hostile ship within the cruise-drop range
//!    (20 km; everything counts as visible until S7) has its engagement deadline refreshed
//!    30 s ahead. Expired deadlines and stale attacker records are cleared.
//! 3. **Projectiles in flight** move one tick ([`projectile`]). A bolt or torpedo hits the
//!    first thing its swept segment crosses: a hostile ship's circle (D-143), a hostile
//!    module's footprint, or a hostile dome's circle where its sector is closed (S5b); it
//!    passes through friendly ships (D-146) and through shrouded ones (D-055). A flak shell
//!    bursts at its aim point and damages every hostile Light and Medium ship within its
//!    burst radius; a hostile dome stops it on the way. A projectile that has flown its
//!    weapon's range expires.
//! 4. **Weapons** choose targets, build lock and fire ([`fire`]): an Attack order's target
//!    when it is in range, a structure included; otherwise, under *weapons free*, the hostile
//!    ship in range with the highest multiplier, then the nearest, then the lowest `SimId`
//!    (D-144); under *return fire*, only hostiles that hit the fleet, or one of its
//!    commander's structures within its range, in the last 30 s (D-145). Lock rises over the
//!    class's lock time and narrows the spread; each shot's direction is drawn from the
//!    system's combat stream (D-049) and aimed with linear lead. Then turrets and FLAK
//!    SCREEN fire ([`crate::defence`]).
//! 5. **Damage** of the tick is applied in `(target SimId, projectile id)` order (doc 15
//!    §4). Every hit on a ship refreshes its fleet's engagement deadline ("taking fire") and
//!    records a ship shooter for return fire. Ships at 0 hull are destroyed at the end of the
//!    phase: they leave their fleets, and empty fleets dissolve. Hits on sectors and modules
//!    go to [`crate::defence`]: breaches, disabled and destroyed modules, the ledger.
//!
//! **Who may fire:** a ship in space whose fleet is not cruising (doc 15 §3.3), that is not
//! shrouded (D-055), and whose fleet does not hold fire. Docked ships and fleets in lane
//! transit are not in space and neither fire nor can be hit.
//!
//! **Structures as targets:** only an explicit Attack order aims ships at a dome or a
//! module; the auto-target rule picks among ships (S5a), so a fleet on *weapons free* does not
//! start breaching a dome it passes. **Detection (S7):** until sensors exist every entity
//! counts as visible; S5 plan §4.7 explains why this cannot change who can be fired on.
//!
//! | Component | Entity | Fields |
//! |---|---|---|
//! | [`Damage`] | ship | hull points lost; absent means undamaged |
//! | [`Weapons`] | ship | per weapon: target, lock, when it may fire next |
//! | [`Attackers`] | fleet | ships that hit one of its ships, with the tick of the last hit |

mod engagement;
mod fire;
mod projectile;
#[cfg(test)]
mod tests;

pub(crate) use engagement::{hostile_ship_within, window as engagement_window};
pub(crate) use fire::{Shot, launch, next_ready};
#[cfg(test)]
pub(crate) use projectile::Burst;
pub use projectile::ProjectileId;

pub(crate) use projectile::{Gun, Projectile, Projectiles, sweep};

use std::collections::BTreeMap;

use nomad_types::Owner;
use serde::{Deserialize, Serialize};

use crate::defence::{DefenceEvent, Structures};
use crate::economy::EconomyEvent;
use crate::fleet::{Fleet, FleetId, Motion, Place, Ship, ShipClass, ShipId, Shroud, reform};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::SimId;
use crate::state::UniverseState;
use crate::time::{TICK_MILLIS, Tick};

/// Hull points a ship has lost (component). Only damaged ships have one, so spawning a ship
/// needs no combat state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Damage {
    pub(crate) taken: u32,
}

/// Weapon state of an armed ship (component), one slot per weapon of its class in the
/// order of the rules file. Only ships that have aimed or fired have one; a missing
/// component means every weapon is idle, unlocked and ready.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Weapons {
    pub(crate) slots: Vec<WeaponSlot>,
}

/// One weapon's state: a ship's weapon, or a turret (S5b).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WeaponSlot {
    /// What it aims at.
    pub(crate) target: Option<Aim>,
    /// Lock built on that target, in milliseconds of the class's lock time (doc 15 §4).
    pub(crate) lock_ms: u64,
    /// Universe time, in milliseconds since tick 0, from which it may fire again.
    pub(crate) ready_at_ms: u64,
}

impl WeaponSlot {
    /// The ship it aims at, if it aims at a ship.
    pub(crate) const fn ship_target(&self) -> Option<ShipId> {
        match self.target {
            Some(Aim::Ship(ship)) => Some(ship),
            _ => None,
        }
    }
}

/// What a weapon aims at: a ship, or a structure an Attack order names (S5b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum Aim {
    /// A ship.
    Ship(ShipId),
    /// A holding's dome: shots go at its centre and meet the circle on the shooter's side.
    Dome(HoldingId),
    /// A module, at its centre.
    Module(SimId),
}

/// Who fired a projectile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum Shooter {
    /// A ship's weapon.
    Ship(ShipId),
    /// A turret module (S5b).
    Module(SimId),
    /// A holding's FLAK SCREEN (S5b).
    Holding(HoldingId),
}

impl Shooter {
    /// The ship that fired, if a ship did.
    pub(crate) const fn ship(self) -> Option<ShipId> {
        match self {
            Self::Ship(ship) => Some(ship),
            Self::Module(_) | Self::Holding(_) => None,
        }
    }
}

/// Ships that hit one of a fleet's ships, with the tick of their last hit (component on
/// fleets), for *return fire* (D-145). Entries older than the engagement window are removed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Attackers {
    pub(crate) last_hit: BTreeMap<ShipId, Tick>,
}

/// Something combat produced during a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatEvent {
    /// A ship took damage this tick from one owner's weapons, summed over the tick.
    ShipDamaged {
        /// The ship.
        ship: ShipId,
        /// Its fleet.
        fleet: FleetId,
        /// Whose weapons hit it.
        by: Owner,
        /// Hull points lost.
        amount: u32,
    },
    /// A ship's hull reached 0 and it was destroyed.
    ShipDestroyed {
        /// The ship.
        ship: ShipId,
        /// Its class.
        class: ShipClass,
        /// Its owner.
        owner: Owner,
        /// Whose hit brought it to 0.
        by: Owner,
    },
    /// A fleet lost its last ship and ceased to exist.
    FleetDestroyed {
        /// The fleet.
        fleet: FleetId,
        /// Its owner.
        owner: Owner,
    },
}

/// Whether `a` and `b` are hostile to each other: anyone else is. The MVP has no alliances
/// (D-040), and NPC raiders are hostile to every commander (doc 15 §10).
pub(crate) fn hostile(a: Owner, b: Owner) -> bool {
    a != b
}

/// Universe time of tick `t` in milliseconds since tick 0.
pub(crate) const fn millis(t: Tick) -> u64 {
    t.get().saturating_mul(TICK_MILLIS)
}

/// A ship in space, as combat sees it at the start of the phase.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShipEntry {
    pub(crate) id: ShipId,
    pub(crate) fleet: FleetId,
    pub(crate) owner: Owner,
    pub(crate) class: ShipClass,
    pub(crate) position: Vec2,
    pub(crate) velocity: Vec2,
    pub(crate) shrouded: bool,
}

/// The ships of one system, with a grid for neighbour queries.
#[derive(Default)]
pub(crate) struct SystemShips {
    /// In `ShipId` order.
    pub(crate) entries: Vec<ShipEntry>,
    grid: BTreeMap<(i64, i64), Vec<usize>>,
    cell_m: f64,
}

impl SystemShips {
    /// Indices of the entries whose position may lie within `radius` of `point`: every entry
    /// in the grid cells that the square around the circle touches. Callers check distance.
    pub(crate) fn near(&self, point: Vec2, radius: f64) -> impl Iterator<Item = usize> + '_ {
        let (x0, y0) = cell(point - Vec2::new(radius, radius), self.cell_m);
        let (x1, y1) = cell(point + Vec2::new(radius, radius), self.cell_m);
        (x0..=x1)
            .flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
            .filter_map(|key| self.grid.get(&key))
            .flatten()
            .copied()
    }
}

/// Every ship in space by system, at the start of the combat phase.
#[derive(Default)]
pub(crate) struct ShipIndex {
    pub(crate) systems: BTreeMap<SystemId, SystemShips>,
    /// Where each ship is: its system and its index there.
    pub(crate) by_ship: BTreeMap<ShipId, (SystemId, usize)>,
}

impl ShipIndex {
    pub(crate) fn build(state: &UniverseState) -> Self {
        let cell_m = state.rules.movement.neighbour_cell_m;
        let mut index = Self::default();
        for id in state.entities.ids_with::<Ship>() {
            let ship = ShipId(id);
            let Some(s) = state.entities.get::<Ship>(id) else {
                continue;
            };
            let Some(Place::InSystem(system)) = state
                .entities
                .with::<Fleet, _>(s.fleet.sim_id(), |f| f.place)
            else {
                continue;
            };
            let Some(motion) = state.entities.get::<Motion>(id) else {
                continue;
            };
            let entry = ShipEntry {
                id: ship,
                fleet: s.fleet,
                owner: s.owner,
                class: s.class,
                position: motion.position.into(),
                velocity: motion.velocity,
                shrouded: state.entities.has::<Shroud>(id),
            };
            let ships = index.systems.entry(system).or_insert_with(|| SystemShips {
                cell_m,
                ..SystemShips::default()
            });
            let i = ships.entries.len();
            ships.entries.push(entry);
            ships
                .grid
                .entry(cell(entry.position, cell_m))
                .or_default()
                .push(i);
            index.by_ship.insert(ship, (system, i));
        }
        index
    }

    /// The entry of `ship`, if it is in space.
    pub(crate) fn get(&self, ship: ShipId) -> Option<(SystemId, &ShipEntry)> {
        let &(system, i) = self.by_ship.get(&ship)?;
        Some((system, self.systems.get(&system)?.entries.get(i)?))
    }
}

/// A cell of the combat grid.
#[expect(
    clippy::cast_possible_truncation,
    reason = "positions lie within a system's radius, far inside i64 after division by the cell"
)]
fn cell(position: Vec2, cell_m: f64) -> (i64, i64) {
    (
        libm::floor(position.x / cell_m) as i64,
        libm::floor(position.y / cell_m) as i64,
    )
}

/// One hit on a ship, before damage is applied.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub(crate) target: ShipId,
    pub(crate) projectile: ProjectileId,
    pub(crate) amount: u32,
    pub(crate) owner: Owner,
    pub(crate) shooter: Shooter,
}

/// Runs phase 7 for tick `now`: ship combat, with station combat ([`crate::defence`]) around
/// it. Defence events and the economy's module events (disabled, restored, destroyed) go to
/// their own lists.
pub(crate) fn run(
    state: &mut UniverseState,
    now: Tick,
    events: &mut Vec<CombatEvent>,
    defence_events: &mut Vec<DefenceEvent>,
    economy_events: &mut Vec<EconomyEvent>,
) {
    let index = ShipIndex::build(state);
    crate::defence::upkeep(state, now);
    let structures = Structures::build(state, &index);
    engagement::refresh(state, now, &index);
    let (hits, structure_hits) = projectile::advance(state, &index, &structures);
    fire::run(state, now, &index, &structures);
    crate::defence::fire(state, now, &index);
    apply(state, now, hits, events);
    crate::defence::apply(state, now, structure_hits, defence_events, economy_events);
    crate::defence::settle(state, now, &index, economy_events);
}

/// Applies the tick's hits in `(target SimId, projectile id)` order (doc 15 §4), then
/// destroys ships at 0 hull.
fn apply(state: &mut UniverseState, now: Tick, mut hits: Vec<Hit>, events: &mut Vec<CombatEvent>) {
    hits.sort_by_key(|h| (h.target, h.projectile));
    let mut damaged: BTreeMap<(ShipId, Owner), (FleetId, u32)> = BTreeMap::new();
    let mut killed: Vec<(ShipId, Owner)> = Vec::new();
    for hit in hits {
        let Some(ship) = state.entities.get::<Ship>(hit.target.sim_id()) else {
            continue;
        };
        let max = state.rules.combat.class(ship.class).hull;
        let taken = state
            .entities
            .get::<Damage>(hit.target.sim_id())
            .map_or(0, |d| d.taken);
        if taken >= max {
            // Already at 0 this tick; the first hit that got it there killed it.
            continue;
        }
        let after = taken.saturating_add(hit.amount).min(max);
        if after > taken {
            state
                .entities
                .insert(hit.target.sim_id(), Damage { taken: after });
            let entry = damaged
                .entry((hit.target, hit.owner))
                .or_insert((ship.fleet, 0));
            entry.1 = entry.1.saturating_add(after - taken);
        }
        if after >= max {
            killed.push((hit.target, hit.owner));
        }
        match hit.shooter.ship() {
            Some(shooter) => engagement::taking_fire(state, now, ship.fleet, shooter),
            // A turret or a FLAK SCREEN: engaged, but there is no ship to return fire at.
            None => {
                engagement::engaged_by_fire(state, now, ship.fleet);
            }
        }
    }
    for ((ship, by), (fleet, amount)) in damaged {
        events.push(CombatEvent::ShipDamaged {
            ship,
            fleet,
            by,
            amount,
        });
    }
    for (ship, by) in killed {
        destroy(state, ship, by, events);
    }
}

/// Removes a destroyed ship: it leaves its fleet, which re-forms, or dissolves if empty.
fn destroy(state: &mut UniverseState, ship: ShipId, by: Owner, events: &mut Vec<CombatEvent>) {
    let Some(s) = state.entities.get::<Ship>(ship.sim_id()) else {
        return;
    };
    state.entities.despawn(ship.sim_id());
    events.push(CombatEvent::ShipDestroyed {
        ship,
        class: s.class,
        owner: s.owner,
        by,
    });
    let spacing = state.rules.movement.formation_spacing_m;
    let empty = state
        .entities
        .get_mut::<Fleet>(s.fleet.sim_id())
        .is_some_and(|f| {
            f.members.remove(&ship);
            reform(f, spacing);
            f.members.is_empty()
        });
    if empty {
        state.entities.despawn(s.fleet.sim_id());
        events.push(CombatEvent::FleetDestroyed {
            fleet: s.fleet,
            owner: s.owner,
        });
    }
}

impl UniverseState {
    /// Hull points `ship` has left, out of its class's maximum, or `None` if there is no
    /// such ship.
    pub fn ship_hull(&self, ship: ShipId) -> Option<(u32, u32)> {
        let class = self.entities.get::<Ship>(ship.sim_id())?.class;
        let max = self.rules.combat.class(class).hull;
        let taken = self
            .entities
            .get::<Damage>(ship.sim_id())
            .map_or(0, |d| d.taken);
        Some((max.saturating_sub(taken), max))
    }

    /// Projectiles in flight, in every system.
    pub fn projectile_count(&self) -> usize {
        self.projectiles.flying.len()
    }
}
