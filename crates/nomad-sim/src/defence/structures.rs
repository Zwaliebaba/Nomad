//! Structures as targets: dome circles and module footprints, hit tests, and damage to
//! sectors and modules (doc 15 §4–§6; D-046, D-053, D-056, D-100, D-163; S5 plan §4.5–§4.6).
//!
//! [`Structures`] is built at the start of each combat phase for the systems where something
//! can be hit (ships in space or projectiles in flight). Per holding it holds the dome, if the
//! core is complete, and the modules that can be hit:
//!
//! | Module | Can be hit |
//! |---|---|
//! | Solar array, harvester, extractor (outside the dome) | Always, disabled ones too (D-169) |
//! | Core, inner modules, dry dock | While a sector of the dome is open (S6's siege, doc 15 §6) |
//! | Pylon | Never: a sector's strength is its pylon's projection (D-053) |
//! | The core of an outpost under construction | Always: it has no dome yet (D-054) |
//!
//! **Damage:** `base × multiplier / 100`, rounded down (D-052), with the Dome multiplier
//! against sectors and the Structure multiplier against modules. NPC weapons do nothing to
//! domes (D-056) or to modules inside the dome (D-163). A hit with no damage still stops the
//! projectile.

use std::collections::BTreeMap;

use nomad_types::{ModuleKind, Owner, Slot};

use super::DefenceEvent;
use super::dome::{self, GridFrame};
use super::hooks::{self, StructureOutcome};
use crate::combat::{Aim, Projectile, ProjectileId, ShipIndex, Shooter, hostile, sweep};
use crate::economy::rules::{MILLI, SlotRadii};
use crate::economy::{self, EconomyEvent, Hull, Module};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::SimId;
use crate::rules::combat::{TargetClass, WeaponKind, WeaponRules};
use crate::state::UniverseState;
use crate::time::Tick;

/// What a projectile hit, other than a ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StructureTarget {
    /// A dome sector.
    Sector { holding: HoldingId, sector: u8 },
    /// A module.
    Module { holding: HoldingId, module: SimId },
}

impl StructureTarget {
    /// The damage-order key (doc 15 §4): the target's `SimId` (a sector's is its holding's),
    /// then the sector.
    fn key(self) -> (SimId, u8) {
        match self {
            Self::Sector { holding, sector } => (holding.sim_id(), sector),
            Self::Module { module, .. } => (module, 0),
        }
    }
}

/// One hit on a structure, before damage is applied.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StructureHit {
    pub(crate) target: StructureTarget,
    pub(crate) projectile: ProjectileId,
    /// Damage after the multiplier, in hull points (sector points for a sector).
    pub(crate) amount: u32,
    pub(crate) owner: Owner,
    pub(crate) shooter: Shooter,
}

/// A holding as a set of circles, for one tick.
#[derive(Clone, Debug)]
struct Station {
    holding: HoldingId,
    owner: Owner,
    centre: Vec2,
    /// Every structure of the holding lies within this distance of its centre.
    bound_m: f64,
    dome: Option<DomeShape>,
    modules: Vec<ModuleCircle>,
}

#[derive(Clone, Copy, Debug)]
struct DomeShape {
    radius_m: f64,
    open: [bool; 4],
}

#[derive(Clone, Copy, Debug)]
struct ModuleCircle {
    id: SimId,
    centre: Vec2,
    radius_m: f64,
    /// Inside a dome: the core, inner modules and the dry dock of a holding that projects
    /// one.
    inner: bool,
    hittable: bool,
}

/// The structures of the systems where something can be hit this tick.
#[derive(Default)]
pub(crate) struct Structures {
    systems: BTreeMap<SystemId, Vec<Station>>,
    directions: [Vec2; 4],
}

/// A module's hit radius in voxels by slot (S4, D-130).
pub(crate) const fn slot_radius_voxels(radii: &SlotRadii, slot: Slot) -> i32 {
    match slot {
        Slot::Core => radii.core,
        Slot::Pylon(_) => radii.pylon,
        Slot::Inner(_) => radii.inner,
        Slot::Dock => radii.dock,
        Slot::Solar => radii.solar,
        Slot::Deposit(_) => radii.deposit,
    }
}

/// Whether `slot` lies inside the dome (D-130).
const fn inside_dome(slot: Slot) -> bool {
    matches!(
        slot,
        Slot::Core | Slot::Pylon(_) | Slot::Inner(_) | Slot::Dock
    )
}

impl Structures {
    /// The structures of every system with ships in space (`index`) or projectiles in
    /// flight.
    pub(crate) fn build(state: &UniverseState, index: &ShipIndex) -> Self {
        let mut active: std::collections::BTreeSet<SystemId> =
            state.projectiles.flying.iter().map(|p| p.system).collect();
        active.extend(index.systems.keys().copied());
        if active.is_empty() {
            return Self::default();
        }
        let mut structures = Self {
            systems: BTreeMap::new(),
            directions: dome::pylon_directions(&state.rules),
        };
        let frame = GridFrame::new(&state.rules);
        let template = &state.rules.economy.slots;
        let voxel_m = state.rules.mother_station.voxel_m;
        for (&holding, h) in &state.holdings {
            let Some(site) = state.galaxy.site(h.site) else {
                continue;
            };
            if !active.contains(&site.system) {
                continue;
            }
            let centre = Vec2::from(site.position);
            let dome = dome::has_dome(state, holding).then(|| {
                let open = state
                    .defence
                    .holdings
                    .get(&holding)
                    .filter(|d| d.dome_up)
                    .map_or([false; 4], |d| d.sectors.map(|s| s.open));
                DomeShape {
                    radius_m: dome::radius_m(&state.rules),
                    open,
                }
            });
            let any_open = dome.is_some_and(|d| d.open.contains(&true));
            let mut bound_m = dome.map_or(0.0, |d| d.radius_m);
            let mut modules = Vec::new();
            for (&slot, &id) in state
                .economy
                .holdings
                .get(&holding)
                .map(|e| &e.slots)
                .into_iter()
                .flatten()
            {
                let Some(kind) = state.module_kind(id) else {
                    continue;
                };
                let Some(grid) = economy::slot_position(template, slot) else {
                    continue;
                };
                let offset = frame.to_system(grid);
                let radius_m =
                    f64::from(slot_radius_voxels(&template.radius_voxels, slot)) * voxel_m;
                bound_m = f64::max(bound_m, offset.length() + radius_m);
                // Inside a dome: an outpost's core has none yet, so raiders can hit it like
                // a peripheral structure (D-054, D-056; D-163 spares only what a dome covers).
                let inner = inside_dome(slot) && dome.is_some();
                let hittable = kind != ModuleKind::Pylon && (!inner || any_open);
                modules.push(ModuleCircle {
                    id,
                    centre: centre + offset,
                    radius_m,
                    inner,
                    hittable,
                });
            }
            modules.sort_by_key(|m| m.id);
            structures
                .systems
                .entry(site.system)
                .or_default()
                .push(Station {
                    holding,
                    owner: Owner::Commander(h.owner),
                    centre,
                    bound_m,
                    dome,
                    modules,
                });
        }
        structures
    }

    fn stations(&self, system: SystemId) -> impl Iterator<Item = &Station> {
        self.systems.get(&system).into_iter().flatten()
    }

    /// The first structure `p`'s segment hits this tick, with where along it (0 to 1): a
    /// closed sector of a hostile dome it enters, or a hostile module it can hit.
    pub(crate) fn first_hit(
        &self,
        state: &UniverseState,
        p: &Projectile,
        weapon: &WeaponRules,
        segment: Vec2,
    ) -> Option<(f64, StructureHit)> {
        let mut first: Option<(f64, (SimId, u8), StructureTarget, u32)> = None;
        let mut consider = |t: f64, target: StructureTarget, amount: u32| {
            let key = target.key();
            if first.is_none_or(|(best, k, ..)| t.total_cmp(&best).then(key.cmp(&k)).is_lt()) {
                first = Some((t, key, target, amount));
            }
        };
        for station in self.stations(p.system) {
            if !hostile(p.owner, station.owner)
                || sweep(p.position, segment, station.centre, station.bound_m).is_none()
            {
                continue;
            }
            if let Some(dome) = station.dome
                && let Some(t) = entry(p.position, segment, station.centre, dome.radius_m)
            {
                let crossing = p.position + segment * t - station.centre;
                let sector = dome::sector_at(&self.directions, crossing);
                if !dome.open[usize::from(sector)] {
                    let amount = state
                        .rules
                        .combat
                        .damage_by(p.owner, weapon, TargetClass::Dome);
                    consider(
                        t,
                        StructureTarget::Sector {
                            holding: station.holding,
                            sector,
                        },
                        amount,
                    );
                }
            }
            for module in station.modules.iter().filter(|m| m.hittable) {
                if let Some(t) = sweep(p.position, segment, module.centre, module.radius_m) {
                    consider(
                        t,
                        StructureTarget::Module {
                            holding: station.holding,
                            module: module.id,
                        },
                        structure_damage(state, p.owner, weapon, module.inner),
                    );
                }
            }
        }
        first.map(|(t, _, target, amount)| {
            (
                t,
                StructureHit {
                    target,
                    projectile: p.id,
                    amount,
                    owner: p.owner,
                    shooter: p.shooter,
                },
            )
        })
    }

    /// Whether a shell of `owner` moving by `step` from `from` in `system` enters a closed
    /// sector of a hostile dome, which stops it.
    pub(crate) fn blocks(&self, system: SystemId, owner: Owner, from: Vec2, step: Vec2) -> bool {
        self.stations(system).any(|station| {
            hostile(owner, station.owner)
                && station.dome.is_some_and(|dome| {
                    entry(from, step, station.centre, dome.radius_m).is_some_and(|t| {
                        let crossing = from + step * t - station.centre;
                        !dome.open[usize::from(dome::sector_at(&self.directions, crossing))]
                    })
                })
        })
    }

    /// Whether `weapon` of a ship of `owner` at `from` in `system` can fire at the structure
    /// `aim`: the structure is hostile, the weapon damages it (so never flak, which only
    /// bursts, and never an NPC weapon against a dome, D-056), and it is within the weapon's
    /// range of the structure's edge. A dome is aimed at from outside only.
    pub(crate) fn reachable(
        &self,
        state: &UniverseState,
        system: SystemId,
        aim: Aim,
        owner: Owner,
        from: Vec2,
        weapon: &WeaponRules,
    ) -> bool {
        if weapon.kind == WeaponKind::Flak {
            return false;
        }
        match aim {
            Aim::Ship(_) => false,
            Aim::Dome(holding) => self.stations(system).any(|station| {
                station.holding == holding
                    && hostile(owner, station.owner)
                    && station.dome.is_some_and(|dome| {
                        let distance = (from - station.centre).length();
                        distance > dome.radius_m
                            && distance - dome.radius_m <= weapon.range_m
                            && state
                                .rules
                                .combat
                                .damage_by(owner, weapon, TargetClass::Dome)
                                > 0
                    })
            }),
            Aim::Module(id) => self.stations(system).any(|station| {
                hostile(owner, station.owner)
                    && station.modules.iter().any(|m| {
                        m.id == id
                            && (from - m.centre).length() - m.radius_m <= weapon.range_m
                            && structure_damage(state, owner, weapon, m.inner) > 0
                    })
            }),
        }
    }

    /// Where a shot at the structure `aim` in `system` is aimed: a dome's centre, or a
    /// module's.
    pub(crate) fn aim_point(&self, system: SystemId, aim: Aim) -> Option<Vec2> {
        match aim {
            Aim::Ship(_) => None,
            Aim::Dome(holding) => self
                .stations(system)
                .find(|s| s.holding == holding)
                .map(|s| s.centre),
            Aim::Module(id) => self
                .stations(system)
                .flat_map(|s| &s.modules)
                .find(|m| m.id == id)
                .map(|m| m.centre),
        }
    }
}

/// Where along `segment` from `start` it enters the circle at `centre`, as a fraction from
/// 0 to 1; `None` if it does not, or if it starts inside (outgoing fire, D-053).
fn entry(start: Vec2, segment: Vec2, centre: Vec2, radius: f64) -> Option<f64> {
    let offset = start - centre;
    if offset.dot(offset) <= radius * radius {
        return None;
    }
    sweep(start, segment, centre, radius)
}

/// Damage of one hit of `weapon`, fired by `owner`, on a module: the Structure multiplier,
/// except that NPC weapons do nothing to modules inside the dome (D-163).
fn structure_damage(state: &UniverseState, owner: Owner, weapon: &WeaponRules, inner: bool) -> u32 {
    if inner && matches!(owner, Owner::Npc(_)) {
        0
    } else {
        state
            .rules
            .combat
            .damage_by(owner, weapon, TargetClass::Structure)
    }
}

/// What an Attack order on `target` aims at (S5b): a holding's dome, or its core while it
/// is an outpost without one; a module other than a pylon (D-053). `None` for anything else.
pub(crate) fn attack_aim(state: &UniverseState, target: SimId) -> Option<Aim> {
    let holding = HoldingId(target);
    if state.holdings.contains_key(&holding) {
        if dome::has_dome(state, holding) {
            return Some(Aim::Dome(holding));
        }
        return state
            .economy
            .holdings
            .get(&holding)
            .and_then(|e| e.slots.get(&Slot::Core))
            .map(|&core| Aim::Module(core));
    }
    let module = state.entities.get::<Module>(target)?;
    (module.kind != ModuleKind::Pylon).then_some(Aim::Module(target))
}

/// How far from its centre an Attack order on `target` meets it: a holding's dome radius if
/// it has a dome, otherwise 0 (a ship, a module, an outpost), for the distance the attacking
/// fleet closes to (S5b).
pub(crate) fn attack_edge_m(state: &UniverseState, target: SimId) -> f64 {
    if dome::has_dome(state, HoldingId(target)) {
        dome::radius_m(&state.rules)
    } else {
        0.0
    }
}

/// A module as a target in space (for the validator and fleet orders): its system, centre
/// and owner. Pylons cannot be targeted (D-053).
pub(crate) fn locate_module(state: &UniverseState, id: SimId) -> Option<(SystemId, Vec2, Owner)> {
    let module = state.entities.get::<Module>(id)?;
    if module.kind == ModuleKind::Pylon {
        return None;
    }
    let owner = state.holdings.get(&module.holding)?.owner;
    let (system, centre, _) = state.module_position(id)?;
    Some((system, centre.into(), Owner::Commander(owner)))
}

/// The circle of one of `holding`'s structures, for *return fire* (D-145): the dome (keyed
/// by the holding's id) or a module. Its system, centre and radius.
pub(crate) fn circle_of(
    state: &UniverseState,
    holding: HoldingId,
    structure: SimId,
) -> Option<(SystemId, Vec2, f64)> {
    if structure == holding.sim_id() {
        let site = state.galaxy.site(state.holdings.get(&holding)?.site)?;
        return Some((
            site.system,
            site.position.into(),
            dome::radius_m(&state.rules),
        ));
    }
    let (system, centre, radius) = state.module_position(structure)?;
    Some((system, centre.into(), radius))
}

/// Per tick and owner, what each sector and module lost, for the events.
#[derive(Default)]
struct Losses {
    sectors: BTreeMap<(HoldingId, u8, Owner), u64>,
    modules: BTreeMap<(SimId, Owner), (HoldingId, u32)>,
}

/// Applies the tick's structure hits in `(target SimId, projectile id)` order (doc 15 §4).
pub(crate) fn apply(
    state: &mut UniverseState,
    now: Tick,
    mut hits: Vec<StructureHit>,
    events: &mut Vec<DefenceEvent>,
    economy_events: &mut Vec<EconomyEvent>,
) {
    hits.sort_by_key(|h| (h.target.key(), h.projectile));
    let mut losses = Losses::default();
    for hit in hits {
        match hit.target {
            StructureTarget::Sector { holding, sector } => {
                damage_sector(state, now, holding, sector, &hit, &mut losses, events);
            }
            StructureTarget::Module { holding, module } => {
                damage_module(
                    state,
                    now,
                    (holding, module),
                    &hit,
                    &mut losses,
                    events,
                    economy_events,
                );
            }
        }
    }
    for ((holding, sector, by), amount_milli) in losses.sectors {
        events.push(DefenceEvent::SectorDamaged {
            holding,
            sector,
            by,
            amount_milli,
        });
    }
    for ((module, by), (holding, amount)) in losses.modules {
        events.push(DefenceEvent::ModuleDamaged {
            holding,
            module,
            by,
            amount,
        });
    }
}

/// Records a hostile ship's damaging hit on `structure` of `holding`, for return fire
/// (D-145).
fn struck(
    state: &mut UniverseState,
    holding: HoldingId,
    structure: SimId,
    hit: &StructureHit,
    now: Tick,
) {
    if let (Some(ship), Some(d)) = (hit.shooter.ship(), state.defence.holdings.get_mut(&holding)) {
        d.struck_by.entry(structure).or_default().insert(ship, now);
    }
}

fn damage_sector(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    sector: u8,
    hit: &StructureHit,
    losses: &mut Losses,
    events: &mut Vec<DefenceEvent>,
) {
    if hit.amount == 0 {
        // Stopped, but no damage: an NPC weapon (D-056) or one too weak after the multiplier.
        return;
    }
    let breach_allowed = hooks::breach_may_occur(state, holding, now);
    let Some(d) = state.defence.holdings.get_mut(&holding) else {
        return;
    };
    if d.immune || !d.dome_up {
        // Reinforced: the dome blocks but takes no damage (doc 12 §4).
        return;
    }
    let Some(s) = d.sectors.get_mut(usize::from(sector)) else {
        return;
    };
    let before = s.strength;
    s.strength = before.saturating_sub(u64::from(hit.amount).saturating_mul(MILLI));
    let removed = before - s.strength;
    let breach = s.strength == 0 && !s.breach_reported && breach_allowed;
    if breach {
        s.breach_reported = true;
    }
    if removed > 0 {
        let entry = losses
            .sectors
            .entry((holding, sector, hit.owner))
            .or_default();
        *entry = entry.saturating_add(removed);
    }
    // A damaging hit on a sector already at 0 counts too: it is the hit that breaches once
    // protection is over (D-159), and its commander gets breach credit (D-057; S6b).
    if let Owner::Commander(commander) = hit.owner {
        d.ledger.dome_hit(commander, removed, now);
    }
    if breach {
        events.push(DefenceEvent::SectorBreached {
            holding,
            sector,
            tick: now,
        });
    }
    struck(state, holding, holding.sim_id(), hit, now);
}

fn damage_module(
    state: &mut UniverseState,
    now: Tick,
    (holding, module): (HoldingId, SimId),
    hit: &StructureHit,
    losses: &mut Losses,
    events: &mut Vec<DefenceEvent>,
    economy_events: &mut Vec<EconomyEvent>,
) {
    if hit.amount == 0 {
        return;
    }
    let (Some(before), Some(kind)) = (
        state.entities.get::<Hull>(module).map(|h| h.hp),
        state.module_kind(module),
    ) else {
        // Destroyed earlier in this tick.
        return;
    };
    let Some(after) = economy::damage_module(state, module, hit.amount, now) else {
        return;
    };
    let removed = before - after;
    if removed > 0 {
        let entry = losses
            .modules
            .entry((module, hit.owner))
            .or_insert((holding, 0));
        entry.1 = entry.1.saturating_add(removed);
        if let Owner::Commander(commander) = hit.owner {
            state
                .defence
                .holdings
                .entry(holding)
                .or_default()
                .ledger
                .module_hit(commander, removed, kind == ModuleKind::Core);
        }
    }
    struck(state, holding, module, hit, now);
    if after > 0 {
        return;
    }
    match hooks::structure_outcome_at_zero(state, module, hit.owner, now) {
        StructureOutcome::Disabled => economy::disable_module(state, module, now, economy_events),
        StructureOutcome::Destroyed => {
            if economy::destroy_module(state, module, economy_events) {
                state.defence.turrets.remove(&module);
            }
        }
        StructureOutcome::CoreDown => {
            if before > 0 {
                events.push(DefenceEvent::CoreDown {
                    holding,
                    by: hit.owner,
                    tick: now,
                });
            }
        }
    }
}
