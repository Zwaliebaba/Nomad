//! Ships and fleets (Design doc 15 §3; S3 plan §4.2).
//!
//! Ships and fleets are `hecs` entities ([`crate::entities`]) identified by [`ShipId`] and
//! [`FleetId`]. Every ship belongs to exactly one fleet, and a fleet moves as one unit: it
//! docks, spools up and transits together. Its **anchor** is a virtual point that steers
//! towards the order's goal at the fleet's limits, the slowest member's speed (doc 15 §3.4);
//! each ship follows its formation slot around the anchor (S3 plan §4.3).
//!
//! | Component | Entity | Fields |
//! |---|---|---|
//! | `Ship` | ship | class, owner, fleet |
//! | `Motion` | ship, fleet anchor | position (m), velocity (m/s), heading (rad) |
//! | `Shroud` | ship | the tick the jump shroud ends (D-055) |
//! | `Fleet` | fleet | owner, place, members with formation offsets, rules of engagement, the active order and the queue |
//!
//! Where a fleet is, docked, in a system or in lane transit, is its [`Place`]; its ships
//! share it. Tactical or cruise mode is not stored: it follows from where the anchor is and
//! whether the fleet is engaged (doc 15 §3.3), see [`UniverseState::fleet_mode`].

#[cfg(test)]
mod behaviour;
#[cfg(test)]
mod jump_tests;
mod manage;
pub(crate) mod movement;
pub(crate) mod orders;
pub(crate) mod route;
pub mod station;

pub(crate) use manage::{create_fleet, merge_fleets};
pub use nomad_types::{FleetId, ShipClass, ShipId};
pub use orders::FleetEvent;

use std::collections::{BTreeMap, VecDeque};

use nomad_types::{FleetOrder, Owner, PositionMetres, RulesOfEngagement};
use serde::{Deserialize, Serialize};

use crate::galaxy::{GateId, LaneId, SystemId};
use crate::geometry::{Vec2, distance_m};
use crate::holding::HoldingId;
use crate::id::SimId;
use crate::state::UniverseState;
use crate::time::Tick;

/// Rules of engagement of a new fleet: weapons free, as in the default Balanced doctrine
/// (D-082, doc 13 §9). *Proposed:* doc 15 §3.4 sets rules per fleet but names no default.
pub const DEFAULT_RULES_OF_ENGAGEMENT: RulesOfEngagement = RulesOfEngagement::WeaponsFree;

/// A ship (component).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Ship {
    pub(crate) class: ShipClass,
    pub(crate) owner: Owner,
    pub(crate) fleet: FleetId,
}

/// Where a ship or a fleet anchor is and how it moves (component), in the frame of the
/// system its fleet is in. Meaningful only while the fleet is [`Place::InSystem`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Motion {
    pub(crate) position: PositionMetres,
    pub(crate) velocity: Vec2,
    pub(crate) heading_rad: f64,
}

/// A ship's jump shroud (component): until `until`, it cannot be targeted and cannot fire,
/// but it can move (D-055). Weapons and sensors use it from S5 and S7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Shroud {
    pub(crate) until: Tick,
}

/// Where a fleet is (S3 plan §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Place {
    /// In space in this system.
    InSystem(SystemId),
    /// Docked at this holding, inside its dome (D-102).
    Docked(HoldingId),
    /// In lane transit: a timer, invisible and uninterruptible (D-042).
    InTransit {
        /// The lane.
        lane: LaneId,
        /// The gate it arrives at.
        to_gate: GateId,
        /// The tick it arrives.
        arrive: Tick,
    },
}

/// Tactical or cruise mode (doc 15 §3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionMode {
    /// Tactical speed: inside ops areas, or while engaged.
    Tactical,
    /// Cruise speed: outside every ops area and not engaged. The fleet cannot fire.
    Cruise,
}

/// A fleet (component).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Fleet {
    pub(crate) owner: Owner,
    pub(crate) place: Place,
    /// Members with their formation offsets, in the fleet frame: +x ahead, +y to the left.
    pub(crate) members: BTreeMap<ShipId, Vec2>,
    pub(crate) rules_of_engagement: RulesOfEngagement,
    /// Set by the "taking fire" and "hostile detected" hooks; drops the fleet out of cruise
    /// mode (doc 15 §3.3).
    pub(crate) engaged: bool,
    pub(crate) active: Option<ActiveOrder>,
    pub(crate) queue: VecDeque<FleetOrder>,
}

/// The order a fleet is carrying out, with its progress.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ActiveOrder {
    /// The order as given.
    pub(crate) order: FleetOrder,
    /// What the fleet is doing about it.
    pub(crate) task: Task,
}

/// What a fleet does for its active order, with its progress (S3 plan §4.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum Task {
    /// Move, or attack-move, to a point in the fleet's system.
    MoveTo { to: PositionMetres },
    /// Loop through waypoints; `next` is the one being approached.
    Patrol {
        route: Vec<PositionMetres>,
        next: usize,
    },
    /// Hold a position.
    StationKeep { at: PositionMetres },
    /// Hold an offset from a target, following it.
    KeepStationOn { target: SimId, offset: Vec2 },
    /// Stay within the guard radius of an object, following it.
    Guard { object: SimId },
    /// Close to weapon range of a target and hold (S3: no weapons yet).
    Attack { target: SimId },
    /// Approach a holding and dock.
    Dock { holding: HoldingId },
    /// Follow a route over lanes, hop by hop, then dock if `then_dock` is set.
    Travel {
        hops: VecDeque<Hop>,
        stage: TravelStage,
        then_dock: Option<HoldingId>,
    },
}

/// One jump of a route: from a gate, along a lane, to the gate at its far end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Hop {
    pub(crate) from_gate: GateId,
    pub(crate) lane: LaneId,
    pub(crate) to_gate: GateId,
}

/// How far a fleet has got with the current hop of a route (S3 plan §4.4, D-055).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TravelStage {
    /// Moving to within jump range of the departure gate.
    Approach,
    /// Spooling up at the gate; it enters the lane at `until`.
    SpoolUp {
        /// When the spool-up started.
        started: Tick,
        /// When the fleet enters the lane.
        until: Tick,
    },
    /// In the lane.
    Transit,
}

/// What a fleet looks like from outside the simulation: a copy of its state.
#[derive(Clone, Debug, PartialEq)]
pub struct FleetView {
    /// The fleet.
    pub id: FleetId,
    /// Its owner.
    pub owner: Owner,
    /// Where it is.
    pub place: Place,
    /// Its ships, in identity order.
    pub ships: Vec<ShipId>,
    /// Its anchor's position; meaningful in a system.
    pub position: PositionMetres,
    /// Its anchor's velocity.
    pub velocity: Vec2,
    /// Its formation heading, radians counter-clockwise from east.
    pub heading_rad: f64,
    /// Tactical or cruise.
    pub mode: MotionMode,
    /// Its rules of engagement (used from S5).
    pub rules_of_engagement: RulesOfEngagement,
    /// Whether a hook marked it engaged.
    pub engaged: bool,
    /// The order it is carrying out.
    pub order: Option<FleetOrder>,
    /// The orders after it.
    pub queued: Vec<FleetOrder>,
    /// How far the current hop of a jump or retreat route has got.
    pub travel: Option<TravelStage>,
}

/// What a ship looks like from outside the simulation: a copy of its state.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipView {
    /// The ship.
    pub id: ShipId,
    /// Its class.
    pub class: ShipClass,
    /// Its owner.
    pub owner: Owner,
    /// Its fleet.
    pub fleet: FleetId,
    /// Where its fleet is.
    pub place: Place,
    /// Its position; meaningful in a system.
    pub position: PositionMetres,
    /// Its velocity.
    pub velocity: Vec2,
    /// Its heading, radians counter-clockwise from east.
    pub heading_rad: f64,
    /// The tick its jump shroud ends, if it has one.
    pub shroud_until: Option<Tick>,
}

/// Formation offsets of the default compact grid (S3 plan §4.3): about √n columns,
/// `spacing_m` apart, centred on the anchor, rows front to back. A short last row is centred.
pub(crate) fn formation_offsets(count: usize, spacing_m: f64) -> Vec<Vec2> {
    if count == 0 {
        return Vec::new();
    }
    let mut columns = 1;
    while columns * columns < count {
        columns += 1;
    }
    let rows = count.div_ceil(columns);
    let centre = |n: usize, i: usize| (n as f64 - 1.0) / 2.0 - i as f64;
    (0..count)
        .map(|i| {
            let (row, column) = (i / columns, i % columns);
            let in_row = if row + 1 == rows {
                count - row * columns
            } else {
                columns
            };
            Vec2::new(
                centre(rows, row) * spacing_m,
                centre(in_row, column) * spacing_m,
            )
        })
        .collect()
}

/// Gives every member of `fleet` its slot in the default formation, in ship order.
pub(crate) fn reform(fleet: &mut Fleet, spacing_m: f64) {
    let offsets = formation_offsets(fleet.members.len(), spacing_m);
    for (offset, slot) in fleet.members.values_mut().zip(offsets) {
        *offset = slot;
    }
}

/// The world position of a formation slot: the anchor plus the offset, rotated with the
/// fleet's heading (S3 plan §4.3).
pub(crate) fn slot_position(anchor: &Motion, offset: Vec2) -> Vec2 {
    Vec2::from(anchor.position) + offset.rotated(anchor.heading_rad)
}

impl UniverseState {
    /// Every fleet, in identity order.
    pub fn fleet_ids(&self) -> Vec<FleetId> {
        self.entities
            .ids_with::<Fleet>()
            .into_iter()
            .map(FleetId)
            .collect()
    }

    /// Every ship, in identity order.
    pub fn ship_ids(&self) -> Vec<ShipId> {
        self.entities
            .ids_with::<Ship>()
            .into_iter()
            .map(ShipId)
            .collect()
    }

    /// A fleet's state, or `None` if there is no such fleet.
    pub fn fleet(&self, id: FleetId) -> Option<FleetView> {
        let fleet = self.entities.get::<Fleet>(id.sim_id())?;
        let anchor = self.entities.get::<Motion>(id.sim_id())?;
        let mode = self.fleet_mode(id)?;
        let travel = match &fleet.active {
            Some(ActiveOrder {
                task: Task::Travel { stage, .. },
                ..
            }) => Some(*stage),
            _ => None,
        };
        Some(FleetView {
            id,
            owner: fleet.owner,
            place: fleet.place,
            ships: fleet.members.keys().copied().collect(),
            position: anchor.position,
            velocity: anchor.velocity,
            heading_rad: anchor.heading_rad,
            mode,
            rules_of_engagement: fleet.rules_of_engagement,
            engaged: fleet.engaged,
            order: fleet.active.map(|a| a.order),
            queued: fleet.queue.into_iter().collect(),
            travel,
        })
    }

    /// A ship's state, or `None` if there is no such ship.
    pub fn ship(&self, id: ShipId) -> Option<ShipView> {
        let ship = self.entities.get::<Ship>(id.sim_id())?;
        let motion = self.entities.get::<Motion>(id.sim_id())?;
        let place = self
            .entities
            .with::<Fleet, _>(ship.fleet.sim_id(), |f| f.place)?;
        Some(ShipView {
            id,
            class: ship.class,
            owner: ship.owner,
            fleet: ship.fleet,
            place,
            position: motion.position,
            velocity: motion.velocity,
            heading_rad: motion.heading_rad,
            shroud_until: self
                .entities
                .get::<Shroud>(id.sim_id())
                .map(|shroud| shroud.until),
        })
    }

    /// Whether `fleet` travels in tactical or cruise mode now: cruise outside every ops area
    /// of its system while not engaged (doc 15 §3.3, D-103). A fleet that is not in a system
    /// counts as tactical.
    pub fn fleet_mode(&self, fleet: FleetId) -> Option<MotionMode> {
        let (place, engaged) = self
            .entities
            .with::<Fleet, _>(fleet.sim_id(), |f| (f.place, f.engaged))?;
        let anchor = self.entities.get::<Motion>(fleet.sim_id())?;
        let Place::InSystem(system) = place else {
            return Some(MotionMode::Tactical);
        };
        Some(
            if !engaged && self.outside_ops_areas(system, anchor.position) {
                MotionMode::Cruise
            } else {
                MotionMode::Tactical
            },
        )
    }

    /// Whether `position` lies outside the ops area of every site of `system` (D-103).
    pub(crate) fn outside_ops_areas(&self, system: SystemId, position: PositionMetres) -> bool {
        let radius = self.rules.movement.ops_area_radius_m;
        self.galaxy.system(system).is_none_or(|sys| {
            sys.sites.iter().all(|&site| {
                self.galaxy
                    .site(site)
                    .is_none_or(|s| distance_m(s.position, position) >= radius)
            })
        })
    }

    /// Ships docked at `holding`.
    pub fn docked_ships(&self, holding: HoldingId) -> u32 {
        self.entities
            .ids_with::<Fleet>()
            .into_iter()
            .filter_map(|id| {
                self.entities.with::<Fleet, _>(id, |f| {
                    (f.place == Place::Docked(holding)).then_some(f.members.len())
                })?
            })
            .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
            .fold(0, u32::saturating_add)
    }

    /// How many ships `holding` can dock (D-102): 24 at a mother-station core, 8 at any
    /// other core, plus 6 per dry dock. An outpost, whose core is not complete, docks none.
    pub fn docking_capacity(&self, holding: HoldingId) -> u32 {
        let Some(h) = self.holdings.get(&holding) else {
            return 0;
        };
        if !h.core_complete {
            return 0;
        }
        let docking = &self.rules.docking;
        let core = if h.mother_station {
            docking.mother_station_core_ships
        } else {
            docking.core_ships
        };
        core.saturating_add(docking.dry_dock_ships.saturating_mul(h.dry_docks))
    }

    /// Checks the ships and fleets for broken invariants and returns what is wrong, if
    /// anything: membership both ways, finite positions, no ship inside an obstacle
    /// footprint, no holding over its docking capacity, a pending transit for every fleet in
    /// a lane. For tests and the harness; an empty list means consistent.
    pub fn fleet_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for ship in self.ship_ids() {
            let Some(view) = self.ship(ship) else {
                problems.push(format!("{ship:?} has no fleet or no motion"));
                continue;
            };
            let listed = self
                .entities
                .with::<Fleet, _>(view.fleet.sim_id(), |f| f.members.contains_key(&ship));
            if listed != Some(true) {
                problems.push(format!("{ship:?} is not listed by {:?}", view.fleet));
            }
            if !view.position.is_finite() || !view.velocity.is_finite() {
                problems.push(format!("{ship:?} has a non-finite position or velocity"));
            }
            if let Place::InSystem(system) = view.place {
                let p = Vec2::from(view.position);
                for obstacle in station::obstacles_in(self, system) {
                    if (p - obstacle.centre).length() < obstacle.radius_m - 1e-6 {
                        problems.push(format!("{ship:?} is inside an obstacle footprint"));
                    }
                }
            }
        }
        for id in self.fleet_ids() {
            let Some(fleet) = self.entities.get::<Fleet>(id.sim_id()) else {
                continue;
            };
            if fleet.members.is_empty() {
                problems.push(format!("{id:?} has no ships"));
            }
            for &ship in fleet.members.keys() {
                if self.entities.get::<Ship>(ship.sim_id()).map(|s| s.fleet) != Some(id) {
                    problems.push(format!("{id:?} lists {ship:?}, which is not in it"));
                }
            }
            if let Place::InTransit { arrive, .. } = fleet.place
                && !self.transits_pending(id, arrive)
            {
                problems.push(format!("{id:?} is in a lane with no transit timer"));
            }
        }
        for &holding in self.holdings.keys() {
            if self.docked_ships(holding) > self.docking_capacity(holding) {
                problems.push(format!("{holding:?} holds more ships than it can dock"));
            }
        }
        problems
    }

    /// Whether the jump-transit queue holds `fleet` with arrival tick `arrive`.
    fn transits_pending(&self, fleet: FleetId, arrive: Tick) -> bool {
        self.transits
            .pending()
            .any(|(deadline, f)| f == &fleet && deadline == arrive)
    }

    /// Marks `fleet` as engaged or not. Engaged fleets drop to tactical speed (doc 15 §3.3).
    ///
    /// The hook for "taking fire" (S5) and "hostile detected within 20 km" (S7). Until those
    /// packages exist only tests call it. When an engagement ends is not designed yet; the
    /// caller clears the flag.
    pub fn set_fleet_engaged(&mut self, fleet: FleetId, engaged: bool) {
        if let Some(f) = self.entities.get_mut::<Fleet>(fleet.sim_id()) {
            f.engaged = engaged;
        }
    }
}

/// Spawns the start-kit fleet of every mother station, docked there (doc 14 §12; S3 plan §3).
/// Identities follow holding order, then the fleet before its ships, by class.
pub(crate) fn spawn_start_kits(state: &mut UniverseState) {
    let mothers: Vec<(HoldingId, Owner)> = state
        .holdings
        .iter()
        .filter(|(_, h)| h.mother_station)
        .map(|(&id, h)| (id, Owner::Commander(h.owner)))
        .collect();
    let kit: Vec<(ShipClass, u32)> = state
        .rules
        .start_kit
        .fleet
        .iter()
        .map(|(&class, &count)| (class, count))
        .collect();
    for (holding, owner) in mothers {
        let anchor = Motion {
            position: station::launch_point(state, holding).unwrap_or_default(),
            velocity: Vec2::ZERO,
            heading_rad: 0.0,
        };
        let fleet_id = FleetId(state.ids.allocate());
        let mut fleet = Fleet {
            owner,
            place: Place::Docked(holding),
            members: BTreeMap::new(),
            rules_of_engagement: DEFAULT_RULES_OF_ENGAGEMENT,
            engaged: false,
            active: None,
            queue: VecDeque::new(),
        };
        let mut ships = Vec::new();
        for &(class, count) in &kit {
            for _ in 0..count {
                let id = ShipId(state.ids.allocate());
                fleet.members.insert(id, Vec2::ZERO);
                ships.push((id, class));
            }
        }
        reform(&mut fleet, state.rules.movement.formation_spacing_m);
        for (id, class) in ships {
            let offset = fleet.members.get(&id).copied().unwrap_or_default();
            let motion = Motion {
                position: slot_position(&anchor, offset).into(),
                ..anchor
            };
            state.entities.spawn(
                id.sim_id(),
                (
                    Ship {
                        class,
                        owner,
                        fleet: fleet_id,
                    },
                    motion,
                ),
            );
        }
        state.entities.spawn(fleet_id.sim_id(), (fleet, anchor));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formation_is_a_compact_centred_grid() {
        assert!(formation_offsets(0, 150.0).is_empty());
        assert_eq!(formation_offsets(1, 150.0), vec![Vec2::ZERO]);
        // Five ships: 3 columns, 2 rows; the second row of 2 is centred.
        let five = formation_offsets(5, 150.0);
        assert_eq!(
            five,
            vec![
                Vec2::new(75.0, 150.0),
                Vec2::new(75.0, 0.0),
                Vec2::new(75.0, -150.0),
                Vec2::new(-75.0, 75.0),
                Vec2::new(-75.0, -75.0),
            ]
        );
        // Nine ships make a 3 × 3 square around the anchor.
        let nine = formation_offsets(9, 150.0);
        assert_eq!(nine[4], Vec2::ZERO);
        for (i, a) in nine.iter().enumerate() {
            for b in &nine[i + 1..] {
                assert!((*a - *b).length() >= 150.0 - 1e-9);
            }
        }
    }
}
