//! Last-known memory per commander (doc 16 §5; D-076, D-177, D-178, D-181; S7 plan §4.4).
//!
//! - **Entries** (D-177): one per fleet (owner, system, position, heading and velocity,
//!   ship count and hull per class of the ships that were detected, the tick seen) and one per
//!   holding (its modules with kind, slot, hull and state, its dome sectors, the tick seen).
//! - **Written on loss of contact:** while an entity is in the picture its live state is what
//!   the commander sees, so an entry is written only at the pass that loses it, from what the
//!   commander last saw: the fleet's anchor before that tick's movement and the ships the
//!   previous pass detected; a holding as it stands, since hull and construction change only
//!   in phases 7 and 8, after the sensors. The tick seen is the tick before that pass. A fleet
//!   that is destroyed or merged in sight leaves no entry, and one that entered a lane leaves
//!   its departure in lane traffic instead (D-179; *Proposed*, see below). An entity that is
//!   sensed again replaces its entry.
//! - **Expiry** (D-076): fleet entries are dropped 24 h after the tick seen, swept once a
//!   minute per commander, staggered by commander. Holding entries stay until the holding is
//!   sensed again or its public owner changes.
//! - **Clearing** (D-178): a fleet entry is dropped when its remembered spot is back in view
//!   without the fleet, that is at a pass where the commander's picture covers the spot for the
//!   largest remembered signature, after a pass where it did not. *Derived* from D-178's "back
//!   in view": at the pass that writes an entry the spot is nearly always still covered (the
//!   fleet left range from there), so clearing at once would leave no memory of fleets that
//!   simply move out of range.
//! - **NPC factions** keep no memory (D-181).
//!
//! *Proposed:* a fleet that leaves view by entering a lane leaves no fleet entry. Its observed
//! departure (D-179), with destination and arrival, is what the commander knows of it; a
//! ghost at the gate would contradict that record.

use std::collections::BTreeMap;

use nomad_types::{
    CommanderId, FleetId, HoldingId, ModuleKind, Owner, PositionMetres, ShipClass, ShipId, SimId,
    SiteId, Slot, SystemId,
};
use serde::{Deserialize, Serialize};

use super::detect::Coverage;
use super::{Anchors, Change, Picture};
use crate::fleet::{Fleet, Motion, Place};
use crate::geometry::Vec2;
use crate::state::UniverseState;
use crate::time::{Tick, Ticks, runs_on};

/// One class of a fleet as an observer saw it: the detected ships of that class and their
/// hull (D-174, D-177).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassSeen {
    /// The class.
    pub class: ShipClass,
    /// How many of its ships were seen.
    pub count: u32,
    /// Their hull points left, in thousandths of their maximum.
    pub hull_permille: u32,
}

/// A fleet as its commander last saw it (D-177).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LastKnownFleet {
    /// The fleet.
    pub fleet: FleetId,
    /// Its owner.
    pub owner: Owner,
    /// The system it was in.
    pub system: SystemId,
    /// Its anchor's position.
    pub position: PositionMetres,
    /// Its anchor's velocity, metres per second.
    pub velocity: Vec2,
    /// Its formation heading, radians counter-clockwise from east.
    pub heading_rad: f64,
    /// The detected ships by class, in class order.
    pub classes: Vec<ClassSeen>,
    /// The last tick it was in view.
    pub seen_at: Tick,
    /// Whether its remembered spot has been out of view since (D-178).
    pub(crate) spot_left_view: bool,
}

/// A module of a holding as seen (D-175).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleSeen {
    /// The module.
    pub id: SimId,
    /// What it is.
    pub kind: ModuleKind,
    /// Its slot.
    pub slot: Slot,
    /// Hull left, in thousandths of its maximum.
    pub hull_permille: u32,
    /// Build time left in milliseconds, while it is being built.
    pub building_ms: Option<u64>,
    /// Whether it is knocked offline.
    pub disabled: bool,
}

/// A dome sector as seen (S5b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectorSeen {
    /// Strength, in thousandths of its maximum.
    pub strength_permille: u32,
    /// Below half its maximum.
    pub weak: bool,
    /// Open in a siege.
    pub open: bool,
    /// Breached and not yet back to its maximum.
    pub breached: bool,
}

/// What a detected holding shows: its modules and dome (D-175). Its existence, site and
/// owner are public anyway (D-044).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldingSighting {
    /// The holding.
    pub holding: HoldingId,
    /// Its owner.
    pub owner: CommanderId,
    /// Its site.
    pub site: SiteId,
    /// Its system.
    pub system: SystemId,
    /// Whether it is an outpost: its core is not complete (D-041).
    pub outpost: bool,
    /// Its modules, in slot order.
    pub modules: Vec<ModuleSeen>,
    /// Its dome's sectors in pylon slot order, if it has a dome.
    pub sectors: Option<[SectorSeen; 4]>,
}

/// A holding as its commander last saw it (D-177).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastKnownHolding {
    /// What was seen.
    pub holding: HoldingSighting,
    /// The last tick it was in view.
    pub seen_at: Tick,
}

/// A commander's last-known memory (D-076, D-177).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Memory {
    pub(crate) fleets: BTreeMap<FleetId, LastKnownFleet>,
    pub(crate) holdings: BTreeMap<HoldingId, LastKnownHolding>,
}

/// What a pass gives memory to work with.
#[derive(Clone, Copy)]
pub(crate) struct Context<'a> {
    pub(crate) observer: Owner,
    pub(crate) old: &'a Picture,
    pub(crate) new: &'a Picture,
    pub(crate) change: &'a Change,
    pub(crate) anchors: &'a Anchors,
    pub(crate) coverage: &'a Coverage,
}

impl Memory {
    pub(crate) fn is_empty(&self) -> bool {
        self.fleets.is_empty() && self.holdings.is_empty()
    }

    /// Applies one pass at `now`.
    pub(crate) fn update(&mut self, state: &UniverseState, now: Tick, ctx: Context<'_>) {
        let seen_at = Tick::new(now.get().saturating_sub(1));
        for &(fleet, system) in &ctx.change.lost_fleets {
            let detected = ctx
                .old
                .system(system)
                .map_or(&[][..], |p| p.ships.as_slice());
            if let Some(entry) = lost_fleet(state, fleet, system, detected, ctx.anchors, seen_at) {
                self.fleets.insert(fleet, entry);
            }
        }
        for &holding in &ctx.change.lost_holdings {
            if let Some(sighting) = holding_sighting(state, holding) {
                self.holdings.insert(
                    holding,
                    LastKnownHolding {
                        holding: sighting,
                        seen_at,
                    },
                );
            }
        }
        // What is in view again replaces its entry: its live state is what is seen.
        for picture in ctx.new.systems.values() {
            for fleet in &picture.fleets {
                self.fleets.remove(fleet);
            }
            for holding in &picture.holdings {
                self.holdings.remove(holding);
            }
        }
        // A holding whose public owner changed is forgotten (D-076).
        self.holdings.retain(|id, entry| {
            state
                .holdings
                .get(id)
                .is_some_and(|h| h.owner == entry.holding.owner)
        });
        // A ghost whose spot is back in view without it is dropped (D-178).
        self.fleets.retain(|_, entry| {
            let signature = largest_signature(state, &entry.classes);
            let covered =
                ctx.coverage
                    .covers(ctx.observer, entry.system, entry.position.into(), signature);
            if covered {
                !entry.spot_left_view
            } else {
                entry.spot_left_view = true;
                true
            }
        });
    }
}

/// The entry for `fleet`, lost from view at this pass after the previous pass detected
/// `detected` of its ships in `system`; `None` if it was destroyed or merged in sight, or
/// entered a lane.
fn lost_fleet(
    state: &UniverseState,
    fleet: FleetId,
    system: SystemId,
    detected: &[ShipId],
    anchors: &Anchors,
    seen_at: Tick,
) -> Option<LastKnownFleet> {
    let (owner, place, members) = state.entities.with::<Fleet, _>(fleet.sim_id(), |f| {
        (
            f.owner,
            f.place,
            f.members.keys().copied().collect::<Vec<_>>(),
        )
    })?;
    let (system, anchor) = match place {
        // Before this tick's movement: what the commander last saw.
        Place::InSystem(now_in) => match anchors.0.get(&fleet) {
            Some(&(was_in, motion)) => (was_in, motion),
            None => (now_in, state.entities.get::<Motion>(fleet.sim_id())?),
        },
        // It docked since: where it docked (*Derived*: its anchor stays at the dock point).
        Place::Docked(_) => (system, state.entities.get::<Motion>(fleet.sim_id())?),
        // Lane traffic records a lane departure (D-179); an emergency jump is invisible like
        // lane transit and starts docked, out of every picture (S6b, D-166).
        Place::InTransit { .. } | Place::EmergencyJump { .. } => return None,
    };
    let seen: Vec<ShipId> = members
        .into_iter()
        .filter(|ship| detected.binary_search(ship).is_ok())
        .collect();
    let classes = classes_seen(state, &seen);
    if classes.is_empty() {
        return None;
    }
    Some(LastKnownFleet {
        fleet,
        owner,
        system,
        position: anchor.position,
        velocity: anchor.velocity,
        heading_rad: anchor.heading_rad,
        classes,
        seen_at,
        spot_left_view: false,
    })
}

/// Ship count and hull per class of `ships` (D-177).
pub(crate) fn classes_seen(state: &UniverseState, ships: &[ShipId]) -> Vec<ClassSeen> {
    let mut by_class: BTreeMap<ShipClass, (u32, u64, u64)> = BTreeMap::new();
    for &ship in ships {
        let Some(class) = state
            .entities
            .with::<crate::fleet::Ship, _>(ship.sim_id(), |s| s.class)
        else {
            continue;
        };
        let (left, max) = state.ship_hull(ship).unwrap_or((0, 0));
        let entry = by_class.entry(class).or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += u64::from(left);
        entry.2 += u64::from(max);
    }
    by_class
        .into_iter()
        .map(|(class, (count, left, max))| ClassSeen {
            class,
            count,
            hull_permille: permille(left, max),
        })
        .collect()
}

/// `part` in thousandths of `whole`, rounded down; 0 for an empty whole.
pub(crate) fn permille(part: u64, whole: u64) -> u32 {
    if whole == 0 {
        return 0;
    }
    u32::try_from(u128::from(part) * 1_000 / u128::from(whole)).unwrap_or(1_000)
}

/// What `holding` shows now to someone who detects it (D-175).
pub(crate) fn holding_sighting(
    state: &UniverseState,
    holding: HoldingId,
) -> Option<HoldingSighting> {
    let h = state.holdings.get(&holding)?;
    let site = state.galaxy.site(h.site)?;
    let modules = state
        .modules_at(holding)
        .into_iter()
        .map(|m| ModuleSeen {
            id: m.id,
            kind: m.kind,
            slot: m.slot,
            hull_permille: permille(u64::from(m.hp), u64::from(m.max_hp)),
            building_ms: m.building_ms,
            disabled: m.disabled,
        })
        .collect();
    let sectors = state.defence(holding).filter(|d| d.dome).map(|d| {
        d.sectors.map(|s| SectorSeen {
            strength_permille: permille(s.strength_milli, s.max_milli),
            weak: s.weak,
            open: s.open,
            breached: s.breached,
        })
    });
    Some(HoldingSighting {
        holding,
        owner: h.owner,
        site: h.site,
        system: site.system,
        outpost: !h.core_complete,
        modules,
        sectors,
    })
}

/// The largest signature among `classes`, in thousandths.
fn largest_signature(state: &UniverseState, classes: &[ClassSeen]) -> u32 {
    classes
        .iter()
        .map(|c| {
            state
                .rules
                .sensors
                .ship_signature(state.rules.combat.class(c.class).size)
        })
        .max()
        .unwrap_or(0)
}

/// The stagger key of `owner`'s memory sweep.
fn sweep_key(owner: Owner) -> u64 {
    const SALT: u64 = 0x5e45_0000_0000_0000;
    match owner {
        Owner::Commander(id) => SALT | u64::from(id.0),
        Owner::Npc(id) => SALT | (1 << 32) | u64::from(id.0),
    }
}

/// Drops fleet entries 24 h after they were seen, once a minute per commander (D-076).
pub(crate) fn sweep(state: &mut UniverseState, now: Tick) {
    let period = Ticks::from_secs(state.rules.sensors.memory_sweep_s);
    let keep = Ticks::from_secs(state.rules.sensors.fleet_memory_s);
    for (&owner, observer) in &mut state.sensors.observers {
        let Some(memory) = &mut observer.memory else {
            continue;
        };
        if memory.fleets.is_empty() || !runs_on(now, period, sweep_key(owner)) {
            continue;
        }
        memory
            .fleets
            .retain(|_, entry| entry.seen_at.after(keep) > now);
    }
}
