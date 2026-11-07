//! Phase 6 of the tick: sensors, visibility, last-known memory and lane traffic (Design
//! doc 16 §3–§5, §7; doc 11 §4.1; D-072 to D-077, D-173 to D-181; S7 plan §4).
//!
//! **Observers** are owners: each commander and each NPC faction (doc 16 §7), in `Owner`
//! order. An owner is one whenever it has a sensor, and keeps its entry while it has a
//! picture, memory or lane traffic.
//!
//! **Sensors** (D-176): the core of each completed holding (40 km at signature 1.0), each
//! ship in space by its class's sensor kind (including ships under the arrival shroud), and
//! **system control**: a commander who controls a system detects everything in it, gate
//! approaches included (D-041, D-073). Docked ships, fleets in lane transit and outposts
//! under construction sense nothing.
//!
//! **Targets:** ships in space, each with its size class's signature (D-174), and holdings
//! (outposts too), one target each at the dome centre with the Structure signature (D-175).
//! Docked ships and fleets in transit are not in space and are not targets (*Derived*: they
//! have no position in the system). Lanes, gates and sites are public, not targets.
//!
//! **Detection** (D-072): observer O detects target T when O controls T's system, or some
//! sensor S of O in T's system has `distance²(S, T) ≤ (range(S) × signature(T))²`. Ranges are
//! whole metres and signatures thousandths, so the comparison is exact (D-049). An observer
//! never detects its own entities: they are own information, always known.
//!
//! **The pass** (D-173): on every tick divisible by 5 (2 Hz), all observers at once, system by
//! system in `SystemId` order. The result per observer is its *picture*: per system, the
//! sorted identities of the detected ships, of the fleets with at least one detected ship, and
//! of the detected holdings. Everything else reads the pictures of the last pass, so each half
//! second has one consistent picture for every reader; while an entity is in a picture its
//! live state is what the observer sees (S7 plan §4.3, §4.4). The pictures are part of the
//! state and of snapshots, because commands and triggers between passes depend on them.
//!
//! **Transitions** give the fleets and holdings that came into and went out of view, as
//! [`VisibilityEvent`]s in the tick's outputs, and drive last-known memory (commanders only,
//! D-181) and lane traffic.
//!
//! **Departures** (D-179) are checked on every tick, before the pass: an observer whose
//! picture holds a ship of a fleet that enters a lane sees the departure.
//!
//! The read-only projection over all of this is [`CommanderView`] ([`view`]).

mod detect;
pub(crate) mod memory;
#[cfg(test)]
mod switch_tests;
#[cfg(test)]
mod tests;
pub(crate) mod traffic;
pub mod view;

pub use detect::DetectionStats;
pub use memory::{
    ClassSeen, HoldingSighting, LastKnownFleet, LastKnownHolding, ModuleSeen, SectorSeen,
};
pub use traffic::LaneTraffic;
pub use view::{CommanderView, PublicHolding, SensedFleet, SensedShip};

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{FleetId, GateId, HoldingId, Owner, ShipId, SimId, SiteId, SystemId};
use serde::{Deserialize, Serialize};

use crate::fleet::{Fleet, FleetEvent, Motion, Place, Ship};
use crate::state::UniverseState;
use crate::time::Tick;

/// The sensors pass runs on every tick divisible by this: 2 Hz (doc 11 §4.1, D-173).
pub const PASS_INTERVAL_TICKS: u64 = 5;

/// Whether the sensors pass runs on `tick` (D-173).
pub const fn is_pass_tick(tick: Tick) -> bool {
    tick.get().is_multiple_of(PASS_INTERVAL_TICKS)
}

/// Every observer's picture, memory and lane traffic (S7 plan §4.3–§4.5).
///
/// An observer that knows nothing beyond public and own information has no entry, so a pass
/// over a Universe with nothing to see leaves the state as it was.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Sensors {
    /// Every observer with a picture, memory or lane traffic, in `Owner` order.
    pub(crate) observers: BTreeMap<Owner, Observer>,
}

/// The last pass tick at or before `tick` (D-173).
pub const fn last_pass_at(tick: Tick) -> Tick {
    Tick::new(tick.get() - tick.get() % PASS_INTERVAL_TICKS)
}

/// One observer's knowledge beyond public and own information.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Observer {
    /// What the last pass detected.
    pub(crate) picture: Picture,
    /// Last-known memory; `None` for NPC factions, which keep none (D-181).
    pub(crate) memory: Option<memory::Memory>,
    /// Observed departures, by fleet (D-179).
    pub(crate) traffic: BTreeMap<FleetId, LaneTraffic>,
}

impl Observer {
    fn new(owner: Owner) -> Self {
        Self {
            picture: Picture::default(),
            memory: match owner {
                Owner::Commander(_) => Some(memory::Memory::default()),
                Owner::Npc(_) => None,
            },
            traffic: BTreeMap::new(),
        }
    }

    /// Whether it holds nothing worth keeping.
    fn is_empty(&self) -> bool {
        self.picture.systems.is_empty()
            && self.traffic.is_empty()
            && self.memory.as_ref().is_none_or(memory::Memory::is_empty)
    }
}

/// An observer's visibility set: what the last pass detected, per system (S7 plan §4.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Picture {
    /// Only systems where something is detected.
    pub(crate) systems: BTreeMap<SystemId, SystemPicture>,
}

impl Picture {
    pub(crate) fn system(&self, system: SystemId) -> Option<&SystemPicture> {
        self.systems.get(&system)
    }

    /// Every detected fleet, with its system.
    fn fleets(&self) -> BTreeSet<(FleetId, SystemId)> {
        self.systems
            .iter()
            .flat_map(|(&system, p)| p.fleets.iter().map(move |&f| (f, system)))
            .collect()
    }

    /// Every detected holding.
    fn holdings(&self) -> BTreeSet<HoldingId> {
        self.systems
            .values()
            .flat_map(|p| p.holdings.iter().copied())
            .collect()
    }
}

/// What an observer detects in one system: sorted identities (S7 plan §4.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SystemPicture {
    /// Detected ships, in `ShipId` order.
    pub(crate) ships: Vec<ShipId>,
    /// Fleets with at least one detected ship, in `FleetId` order (D-174).
    pub(crate) fleets: Vec<FleetId>,
    /// Detected holdings, in `HoldingId` order (D-175).
    pub(crate) holdings: Vec<HoldingId>,
}

impl SystemPicture {
    pub(crate) fn has_ship(&self, ship: ShipId) -> bool {
        self.ships.binary_search(&ship).is_ok()
    }

    pub(crate) fn has_fleet(&self, fleet: FleetId) -> bool {
        self.fleets.binary_search(&fleet).is_ok()
    }

    pub(crate) fn has_holding(&self, holding: HoldingId) -> bool {
        self.holdings.binary_search(&holding).is_ok()
    }

    fn is_empty(&self) -> bool {
        self.ships.is_empty() && self.fleets.is_empty() && self.holdings.is_empty()
    }
}

/// Something that changed in an observer's knowledge during a tick (S7 plan §4.3, §4.5), for
/// replication (N4), the AI's inbox (A1) and the harness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VisibilityEvent {
    /// At least one ship of the fleet came into view.
    FleetSighted {
        /// Who sees it.
        observer: Owner,
        /// The fleet.
        fleet: FleetId,
    },
    /// No ship of the fleet is in view any more.
    FleetLost {
        /// Who lost it.
        observer: Owner,
        /// The fleet.
        fleet: FleetId,
    },
    /// The holding came into view: its modules are seen (D-175).
    HoldingSighted {
        /// Who sees it.
        observer: Owner,
        /// The holding.
        holding: HoldingId,
    },
    /// The holding went out of view.
    HoldingLost {
        /// Who lost it.
        observer: Owner,
        /// The holding.
        holding: HoldingId,
    },
    /// A departure was seen (D-179); the record is in the observer's lane traffic.
    DepartureObserved {
        /// Who saw it.
        observer: Owner,
        /// The departing fleet.
        fleet: FleetId,
        /// Where it goes.
        to: SystemId,
        /// When it arrives.
        arrives: Tick,
    },
}

/// Where each fleet in space is before this tick's movement: what an observer last saw of a
/// fleet that the pass then loses (S7 plan §4.4). Captured on pass ticks only, and not kept.
pub(crate) struct Anchors(BTreeMap<FleetId, (SystemId, Motion)>);

/// Captures [`Anchors`] before phase 5 on a pass tick; `None` on other ticks.
pub(crate) fn before_movement(state: &UniverseState, now: Tick) -> Option<Anchors> {
    is_pass_tick(now).then(|| capture(state))
}

fn capture(state: &UniverseState) -> Anchors {
    let mut anchors = BTreeMap::new();
    for id in state.entities.ids_with::<Fleet>() {
        let Some(Place::InSystem(system)) = state.entities.with::<Fleet, _>(id, |f| f.place) else {
            continue;
        };
        if let Some(motion) = state.entities.get::<Motion>(id) {
            anchors.insert(FleetId(id), (system, motion));
        }
    }
    Anchors(anchors)
}

/// Runs phase 6 for tick `now`: departures seen this tick, the memory sweep, and on a pass
/// tick the pass itself, with `anchors` from before the tick's movement.
pub(crate) fn run(
    state: &mut UniverseState,
    now: Tick,
    anchors: Option<&Anchors>,
    fleet_events: &[FleetEvent],
    events: &mut Vec<VisibilityEvent>,
) {
    traffic::observe_departures(state, now, fleet_events, events);
    memory::sweep(state, now);
    if let Some(anchors) = anchors {
        pass(state, now, anchors, events);
    }
}

/// A pass at `now`: detection, then each observer's transitions.
fn pass(
    state: &mut UniverseState,
    now: Tick,
    anchors: &Anchors,
    events: &mut Vec<VisibilityEvent>,
) {
    let detection = detect::detect(state);
    let mut owners: BTreeSet<Owner> = state.sensors.observers.keys().copied().collect();
    owners.extend(detection.pictures.keys().copied());
    let mut pictures = detection.pictures;
    for owner in owners {
        let picture = pictures.remove(&owner).unwrap_or_default();
        let mut observer = state
            .sensors
            .observers
            .remove(&owner)
            .unwrap_or_else(|| Observer::new(owner));
        let old = std::mem::replace(&mut observer.picture, picture);
        let change = Change::between(&old, &observer.picture);
        change.report(owner, events);
        if let Some(memory) = &mut observer.memory {
            memory.update(
                state,
                now,
                memory::Context {
                    observer: owner,
                    old: &old,
                    new: &observer.picture,
                    change: &change,
                    anchors,
                    coverage: &detection.coverage,
                },
            );
        }
        traffic::expire(state, now, &mut observer);
        if !observer.is_empty() {
            state.sensors.observers.insert(owner, observer);
        }
    }
}

/// What came into and went out of an observer's view at a pass.
pub(crate) struct Change {
    /// Fleets no longer detected, with the system they were detected in.
    pub(crate) lost_fleets: Vec<(FleetId, SystemId)>,
    pub(crate) new_fleets: Vec<FleetId>,
    pub(crate) lost_holdings: Vec<HoldingId>,
    pub(crate) new_holdings: Vec<HoldingId>,
}

impl Change {
    fn between(old: &Picture, new: &Picture) -> Self {
        let (old_fleets, new_fleets) = (old.fleets(), new.fleets());
        let ids = |set: &BTreeSet<(FleetId, SystemId)>| -> BTreeSet<FleetId> {
            set.iter().map(|&(f, _)| f).collect()
        };
        let (old_ids, new_ids) = (ids(&old_fleets), ids(&new_fleets));
        let (old_holdings, new_holdings) = (old.holdings(), new.holdings());
        Self {
            lost_fleets: old_fleets
                .iter()
                .filter(|(f, _)| !new_ids.contains(f))
                .copied()
                .collect(),
            new_fleets: new_ids.difference(&old_ids).copied().collect(),
            lost_holdings: old_holdings.difference(&new_holdings).copied().collect(),
            new_holdings: new_holdings.difference(&old_holdings).copied().collect(),
        }
    }

    fn report(&self, observer: Owner, events: &mut Vec<VisibilityEvent>) {
        events.extend(
            self.new_fleets
                .iter()
                .map(|&fleet| VisibilityEvent::FleetSighted { observer, fleet }),
        );
        events.extend(
            self.lost_fleets
                .iter()
                .map(|&(fleet, _)| VisibilityEvent::FleetLost { observer, fleet }),
        );
        events.extend(
            self.new_holdings
                .iter()
                .map(|&holding| VisibilityEvent::HoldingSighted { observer, holding }),
        );
        events.extend(
            self.lost_holdings
                .iter()
                .map(|&holding| VisibilityEvent::HoldingLost { observer, holding }),
        );
    }
}

/// Runs a pass now, outside the step, with no outputs: for the generator at tick 0 (a pass
/// tick) and for fixtures that place fleets directly (the harness's scenarios, tests).
pub(crate) fn refresh(state: &mut UniverseState) {
    let now = state.tick;
    let anchors = capture(state);
    pass(state, now, &anchors, &mut Vec::new());
}

/// The picture of `observer` in `system`, if it detects anything there.
pub(crate) fn picture_of(
    state: &UniverseState,
    observer: Owner,
    system: SystemId,
) -> Option<&SystemPicture> {
    state
        .sensors
        .observers
        .get(&observer)?
        .picture
        .system(system)
}

/// Whether `observer` may know where `target`, an object in `system`, is now, by the last
/// pass (doc 16 §2–§3): public objects (holdings, sites and gates; D-044) and its own
/// entities always; another owner's ship or fleet while it is in the picture (D-174); a
/// module while its holding is (D-175). An identity the picture holds counts even if its
/// entity was destroyed since the pass, so callers can tell a target destroyed in sight from
/// one that went out of view (D-180).
pub(crate) fn sees(
    state: &UniverseState,
    observer: Owner,
    system: SystemId,
    target: SimId,
) -> bool {
    if state.holdings.contains_key(&HoldingId(target))
        || state.galaxy.site(SiteId(target)).is_some()
        || state.galaxy.gate(GateId(target)).is_some()
        || owner_of(state, target) == Some(observer)
    {
        return true;
    }
    let Some(picture) = picture_of(state, observer, system) else {
        return false;
    };
    picture.has_ship(ShipId(target))
        || picture.has_fleet(FleetId(target))
        || state
            .entities
            .with::<crate::economy::Module, _>(target, |m| m.holding)
            .is_some_and(|holding| picture.has_holding(holding))
}

/// The owner of a ship, fleet or module, if it exists.
fn owner_of(state: &UniverseState, id: SimId) -> Option<Owner> {
    if let Some(owner) = state.entities.with::<Ship, _>(id, |s| s.owner) {
        return Some(owner);
    }
    if let Some(owner) = state.entities.with::<Fleet, _>(id, |f| f.owner) {
        return Some(owner);
    }
    let holding = state
        .entities
        .with::<crate::economy::Module, _>(id, |m| m.holding)?;
    state
        .holdings
        .get(&holding)
        .map(|h| Owner::Commander(h.owner))
}

/// The pictures of the last pass for one phase that reads them while it changes the rest of
/// the state: combat (phase 7) takes them out of the state and puts them back, so its weapons
/// can ask what their owner sees without borrowing the state (doc 15 §4, D-114).
pub(crate) struct Seen(Sensors);

impl Seen {
    /// Takes the pictures out of `state`; [`Seen::restore`] puts them back.
    pub(crate) fn take(state: &mut UniverseState) -> Self {
        Self(std::mem::take(&mut state.sensors))
    }

    /// Puts the pictures back.
    pub(crate) fn restore(self, state: &mut UniverseState) {
        state.sensors = self.0;
    }

    /// What `observer` detects in `system`.
    pub(crate) fn picture(&self, observer: Owner, system: SystemId) -> Option<&SystemPicture> {
        self.0.observers.get(&observer)?.picture.system(system)
    }

    /// Whether `observer` detects `ship` in `system`.
    pub(crate) fn ship(&self, observer: Owner, system: SystemId, ship: ShipId) -> bool {
        self.picture(observer, system)
            .is_some_and(|p| p.has_ship(ship))
    }
}

impl UniverseState {
    /// What one sensors pass costs on the current state, without changing it: the same
    /// detection the step runs (S7 plan §6 task 9). For the harness, which times it.
    pub fn sensors_detection_stats(&self) -> DetectionStats {
        detect::detect(self).stats
    }

    /// The tick of the last sensors pass, which every picture is from (D-173).
    pub const fn last_sensors_pass(&self) -> Tick {
        last_pass_at(self.tick)
    }

    /// The read-only view of `observer`: what it may know (doc 16 §7, D-077; S7 plan §4.7).
    pub fn view(&self, observer: Owner) -> CommanderView<'_> {
        CommanderView::new(self, observer)
    }
}
