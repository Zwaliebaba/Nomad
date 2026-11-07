//! `CommanderView`: everything one observer may know, read-only (doc 16 §2, §7; D-075, D-077;
//! S7 plan §4.7).
//!
//! The AI (the Deputy, the Strategist and the raider director, through `nomad-ai`'s `View`
//! trait, D-199) and replication (N4, D-182) see the Universe only through this projection.
//! It borrows the state and copies nothing until a query asks for it, and no query returns
//! the state or a handle that leads back to it, so "same information as humans" (D-027)
//! holds for whoever holds a view.
//!
//! The queries follow the information classes of doc 16 §2:
//!
//! - **public:** the topology, the season's rules, holdings with their owners and sites,
//!   system control, the commander roster and each commander's exposure window (D-044, D-075),
//!   each holding's `breachable_from` (D-170), and the season: its clock and final tick, the
//!   standings, the early-win threshold, who is one system short and the result (S8 plan
//!   §4.7; D-075, D-191 to D-197);
//! - **own:** own fleets (in transit or in an emergency jump too, with their arrival tick)
//!   and ships in full, own holdings with their siege state, own fallen mother stations in
//!   their emergency jump (D-166), the economy views of S4 §4.8, own defence views, the own
//!   commander record, the own doctrine and which own fleets are under direct control (A1);
//! - **sensed:** per system, the detected fleets with only their detected ships (D-174) and
//!   the detected holdings with their modules and dome (D-175): position, heading, velocity,
//!   class, owner, hull, nothing private (no orders, cargo, doctrine or queues);
//! - **last-known:** memory entries with the tick seen (D-076, D-177);
//! - **lane traffic:** observed departures (D-179);
//! - **announcements** addressed to this observer, filtered from a tick's outputs (the
//!   simulation keeps no announcement log; A1's inbox will, D-202).
//!
//! **Indices for the AI's budget** (D-083): every query that lists other owners' entities
//! goes through the per-system index of the last pass's picture, so a query about one
//! system touches only that system. Sensed entities show their live state: between passes
//! the picture says what is seen, the state says where it is now (S7 plan §4.3).
//!
//! **NPC factions** get the same view with no memory, economy, record or announcements
//! beyond public ones (D-181).

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{
    CommanderId, ControlState, FleetId, HoldingId, Owner, PositionMetres, SeasonResult, ShipClass,
    ShipId, SiteId, SystemId,
};

use super::memory::{
    HoldingSighting, LastKnownFleet, LastKnownHolding, holding_sighting, permille,
};
use super::traffic::LaneTraffic;
use super::{Observer, SystemPicture, VisibilityEvent};
use crate::announce::Announcement;
use crate::commander::{CommanderRecord, ExposureWindow};
use crate::defence::DefenceView;
use crate::economy::{Cargo, CommanderEconomy, HoldingEconomyView};
use crate::fleet::{Fleet, FleetView, Motion, MotionMode, Place, Ship, ShipView, Shroud};
use crate::galaxy::Galaxy;
use crate::geometry::Vec2;
use crate::rules::Rules;
use crate::season::Standings;
use crate::siege::{SiegeView, StationJumpView};
use crate::state::UniverseState;
use crate::time::{SeasonClock, Tick};

/// A holding as everyone sees it: owner and site (D-044).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicHolding {
    /// The holding.
    pub id: HoldingId,
    /// Its owner.
    pub owner: CommanderId,
    /// Its site.
    pub site: SiteId,
    /// Its system.
    pub system: SystemId,
    /// Whether it is an outpost: its core is not complete (D-041).
    pub outpost: bool,
}

/// A detected ship: fully identified (D-072).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SensedShip {
    /// The ship.
    pub id: ShipId,
    /// Its fleet.
    pub fleet: FleetId,
    /// Its owner.
    pub owner: Owner,
    /// Its class.
    pub class: ShipClass,
    /// Its position.
    pub position: PositionMetres,
    /// Its velocity, metres per second.
    pub velocity: Vec2,
    /// Its heading, radians counter-clockwise from east.
    pub heading_rad: f64,
    /// Hull left, in thousandths of its class's maximum.
    pub hull_permille: u32,
    /// Under the arrival shroud: seen, but it cannot be targeted (D-055, doc 16 §4).
    pub shrouded: bool,
}

/// A fleet with at least one detected ship, listing only those (D-174).
#[derive(Clone, Debug, PartialEq)]
pub struct SensedFleet {
    /// The fleet.
    pub id: FleetId,
    /// Its owner.
    pub owner: Owner,
    /// Its system.
    pub system: SystemId,
    /// Its anchor's position (the formation centre).
    pub position: PositionMetres,
    /// Its anchor's velocity, metres per second.
    pub velocity: Vec2,
    /// Its formation heading, radians counter-clockwise from east.
    pub heading_rad: f64,
    /// Tactical or cruise speed (doc 15 §3.3), as its motion shows.
    pub mode: MotionMode,
    /// Its detected ships, in identity order.
    pub ships: Vec<SensedShip>,
}

/// What `observer` may know of the Universe, borrowed from the state (S7 plan §4.7).
#[derive(Clone, Copy)]
pub struct CommanderView<'a> {
    state: &'a UniverseState,
    observer: Owner,
    knowledge: Option<&'a Observer>,
}

impl<'a> CommanderView<'a> {
    pub(super) fn new(state: &'a UniverseState, observer: Owner) -> Self {
        Self {
            state,
            observer,
            knowledge: state.sensors.observers.get(&observer),
        }
    }

    // --- Identity and time.

    /// Whose view this is.
    pub const fn observer(&self) -> Owner {
        self.observer
    }

    /// The state the view borrows, for the AI's `View` implementation in this crate
    /// ([`crate::ai`]), which answers only the observer's own, public and sensed queries.
    pub(crate) const fn universe(&self) -> &'a UniverseState {
        self.state
    }

    /// The last completed tick.
    pub const fn tick(&self) -> Tick {
        self.state.tick
    }

    /// Maps ticks to Universe time.
    pub const fn clock(&self) -> SeasonClock {
        self.state.clock
    }

    /// The tick of the last sensors pass, which the sensed picture is from (D-173).
    pub const fn picture_tick(&self) -> Tick {
        super::last_pass_at(self.state.tick)
    }

    // --- Public (doc 16 §2).

    /// The season's rules: costs, ranges and class numbers, the same for everyone (D-104).
    pub const fn rules(&self) -> &'a Rules {
        &self.state.rules
    }

    /// The season galaxy: fully charted from the start (D-044).
    pub const fn galaxy(&self) -> &'a Galaxy {
        &self.state.galaxy
    }

    /// Every holding and outpost, in identity order (D-044).
    pub fn holdings(&self) -> impl Iterator<Item = PublicHolding> + 'a {
        let state = self.state;
        state
            .holdings
            .keys()
            .filter_map(move |&id| public_holding(state, id))
    }

    /// The holdings and outposts in `system`, in identity order.
    pub fn holdings_in(&self, system: SystemId) -> Vec<PublicHolding> {
        self.holdings().filter(|h| h.system == system).collect()
    }

    /// One holding, if it exists.
    pub fn holding(&self, id: HoldingId) -> Option<PublicHolding> {
        public_holding(self.state, id)
    }

    /// The control state of a claimable system (D-041).
    pub fn control_state(&self, system: SystemId) -> Option<ControlState> {
        self.state.control_state(system)
    }

    /// The control state of every claimable system, in system order.
    pub fn control_states(&self) -> BTreeMap<SystemId, ControlState> {
        self.state.control_states()
    }

    /// The commanders of the Universe, in identity order. Whether each is human or AI and
    /// whether they are online are private (D-075, D-078).
    pub fn commanders(&self) -> Vec<CommanderId> {
        self.state.commander_ids()
    }

    /// A commander's exposure window (D-075).
    pub fn exposure_window(&self, commander: CommanderId) -> Option<ExposureWindow> {
        self.state.exposure_window(commander)
    }

    /// The tick from which a holding can be breached (D-170); `None` if there is no such
    /// holding.
    pub fn breachable_from(&self, holding: HoldingId) -> Option<Tick> {
        self.state.breachable_from(holding)
    }

    /// The standings after the last completed tick, best first (D-191).
    pub fn standings(&self) -> &'a Standings {
        self.state.standings()
    }

    /// The season's result, once fixed (D-116, D-192).
    pub fn season_result(&self) -> Option<&'a SeasonResult> {
        self.state.season_result()
    }

    /// The season's final tick: the tick its result was fixed on, or else the scheduled one
    /// (D-195; doc 17 §6's season clock).
    pub fn final_tick(&self) -> Tick {
        self.state.final_tick()
    }

    /// How many systems a commander must control to win early (D-020, D-192).
    pub fn early_win_threshold(&self) -> u32 {
        self.state.early_win_threshold()
    }

    /// The commanders one system short of the early win, as announced (D-197).
    pub fn near_early_win(&self) -> &'a BTreeSet<CommanderId> {
        self.state.near_early_win()
    }

    // --- Own (doc 16 §2: everything about one's own assets).

    /// The observer, if it is a commander.
    fn commander(&self) -> Option<CommanderId> {
        self.observer.commander()
    }

    /// The observer's own commander record: presence, grace and window choice, private.
    pub fn own_record(&self) -> Option<CommanderRecord> {
        self.state.commander(self.commander()?)
    }

    /// The observer's own doctrine, private (doc 12 §5.2, D-075; A1 plan §5.1). Which own
    /// fleets are under direct control is in their fleet views (RK-10).
    pub fn own_doctrine(&self) -> Option<crate::doctrine::Doctrine> {
        Some(self.state.doctrine(self.commander()?))
    }

    /// Every own fleet, in identity order, wherever it is: in space, docked or in lane
    /// transit with its arrival tick.
    pub fn own_fleets(&self) -> Vec<FleetView> {
        self.own_fleet_ids()
            .into_iter()
            .filter_map(|id| self.state.fleet(id))
            .collect()
    }

    /// The own fleets in space in `system` or docked at a holding there.
    pub fn own_fleets_in(&self, system: SystemId) -> Vec<FleetView> {
        self.own_fleets()
            .into_iter()
            .filter(|f| place_system(self.state, f.place) == Some(system))
            .collect()
    }

    /// An own fleet.
    pub fn own_fleet(&self, id: FleetId) -> Option<FleetView> {
        self.state.fleet(id).filter(|f| f.owner == self.observer)
    }

    /// An own ship.
    pub fn own_ship(&self, id: ShipId) -> Option<ShipView> {
        self.state.ship(id).filter(|s| s.owner == self.observer)
    }

    /// An own ship's cargo (S4).
    pub fn own_cargo(&self, ship: ShipId) -> Option<Cargo> {
        self.own_ship(ship).map(|_| self.state.cargo(ship))
    }

    pub(crate) fn own_fleet_ids(&self) -> Vec<FleetId> {
        let observer = self.observer;
        self.state
            .entities
            .ids_with::<Fleet>()
            .into_iter()
            .filter(|&id| {
                self.state
                    .entities
                    .with::<Fleet, _>(id, |f| f.owner == observer)
                    == Some(true)
            })
            .map(FleetId)
            .collect()
    }

    /// The observer's holdings and outposts, in identity order.
    pub fn own_holdings(&self) -> Vec<PublicHolding> {
        let Some(me) = self.commander() else {
            return Vec::new();
        };
        self.holdings().filter(|h| h.owner == me).collect()
    }

    /// The observer's economy (S4 plan §4.8); `None` for an NPC faction.
    pub fn economy(&self) -> Option<CommanderEconomy> {
        self.commander().map(|me| self.state.economy_of(me))
    }

    /// An own holding's economy: slots and modules, queue, power, buffers.
    pub fn holding_economy(&self, id: HoldingId) -> Option<HoldingEconomyView> {
        self.owns_holding(id)
            .then(|| self.state.holding_economy(id))
            .flatten()
    }

    /// An own holding's defence: sectors, switches, turrets and abilities (S5b).
    pub fn defence(&self, id: HoldingId) -> Option<DefenceView> {
        self.owns_holding(id)
            .then(|| self.state.defence(id))
            .flatten()
    }

    /// An own holding's siege state with its deadlines (S6 plan §4.8). Others hear of
    /// breaches, sieges and Held through announcements addressed to them (D-075).
    pub fn siege(&self, id: HoldingId) -> Option<SiegeView> {
        self.owns_holding(id)
            .then(|| self.state.siege(id))
            .flatten()
    }

    /// The observer's fallen mother stations between their fall and their landing (D-166).
    pub fn own_station_jumps(&self) -> Vec<StationJumpView> {
        let Some(me) = self.commander() else {
            return Vec::new();
        };
        self.state
            .station_jumps()
            .into_iter()
            .filter(|j| j.owner == me)
            .collect()
    }

    fn owns_holding(&self, id: HoldingId) -> bool {
        self.state
            .holdings
            .get(&id)
            .is_some_and(|h| Some(h.owner) == self.commander())
    }

    // --- Sensed (doc 16 §3), through the per-system index.

    fn picture(&self, system: SystemId) -> Option<&'a SystemPicture> {
        self.knowledge?.picture.system(system)
    }

    /// The systems where the last pass detected something, in system order.
    pub fn sensed_systems(&self) -> impl Iterator<Item = SystemId> + 'a {
        self.knowledge
            .into_iter()
            .flat_map(|k| k.picture.systems.keys().copied())
    }

    /// The other owners' ships detected in `system` by the last pass, in identity order: the
    /// cheap check for "anything here" (D-083). Some may since have docked, jumped or been
    /// destroyed; the queries below skip those.
    pub fn sensed_ship_ids_in(&self, system: SystemId) -> &'a [ShipId] {
        self.picture(system).map_or(&[], |p| p.ships.as_slice())
    }

    /// The detected fleets in `system`, in identity order, each with its detected ships.
    pub fn sensed_fleets_in(&self, system: SystemId) -> Vec<SensedFleet> {
        let Some(picture) = self.picture(system) else {
            return Vec::new();
        };
        picture
            .fleets
            .iter()
            .filter_map(|&fleet| self.sensed_fleet_at(system, fleet, picture))
            .collect()
    }

    /// A detected fleet.
    pub fn sensed_fleet(&self, id: FleetId) -> Option<SensedFleet> {
        let system = self.in_space(id)?;
        let picture = self.picture(system)?;
        picture
            .has_fleet(id)
            .then(|| self.sensed_fleet_at(system, id, picture))
            .flatten()
    }

    /// A detected ship.
    pub fn sensed_ship(&self, id: ShipId) -> Option<SensedShip> {
        let fleet = self
            .state
            .entities
            .with::<Ship, _>(id.sim_id(), |s| s.fleet)?;
        let system = self.in_space(fleet)?;
        self.picture(system)?
            .has_ship(id)
            .then(|| sensed_ship(self.state, id))
            .flatten()
    }

    /// The detected holdings in `system`, with their modules and dome (D-175).
    pub fn sensed_holdings_in(&self, system: SystemId) -> Vec<HoldingSighting> {
        self.picture(system).map_or_else(Vec::new, |p| {
            p.holdings
                .iter()
                .filter_map(|&h| holding_sighting(self.state, h))
                .collect()
        })
    }

    /// A detected holding, with its modules and dome.
    pub fn sensed_holding(&self, id: HoldingId) -> Option<HoldingSighting> {
        let system = public_holding(self.state, id)?.system;
        self.picture(system)?
            .has_holding(id)
            .then(|| holding_sighting(self.state, id))
            .flatten()
    }

    /// Whether the last pass detected `id`, another owner's ship, fleet or holding.
    pub fn is_sensed(&self, id: nomad_types::SimId) -> bool {
        self.knowledge.is_some_and(|k| {
            k.picture.systems.values().any(|p| {
                p.has_ship(ShipId(id)) || p.has_fleet(FleetId(id)) || p.has_holding(HoldingId(id))
            })
        })
    }

    /// The system `fleet` is in, if it is in space.
    fn in_space(&self, fleet: FleetId) -> Option<SystemId> {
        match self
            .state
            .entities
            .with::<Fleet, _>(fleet.sim_id(), |f| f.place)?
        {
            Place::InSystem(system) => Some(system),
            Place::Docked(_) | Place::InTransit { .. } | Place::EmergencyJump { .. } => None,
        }
    }

    fn sensed_fleet_at(
        &self,
        system: SystemId,
        id: FleetId,
        picture: &SystemPicture,
    ) -> Option<SensedFleet> {
        let (owner, place, members) = self.state.entities.with::<Fleet, _>(id.sim_id(), |f| {
            (
                f.owner,
                f.place,
                f.members.keys().copied().collect::<Vec<_>>(),
            )
        })?;
        if place != Place::InSystem(system) {
            return None;
        }
        let anchor = self.state.entities.get::<Motion>(id.sim_id())?;
        let ships: Vec<SensedShip> = members
            .into_iter()
            .filter(|&ship| picture.has_ship(ship))
            .filter_map(|ship| sensed_ship(self.state, ship))
            .collect();
        if ships.is_empty() {
            return None;
        }
        Some(SensedFleet {
            id,
            owner,
            system,
            position: anchor.position,
            velocity: anchor.velocity,
            heading_rad: anchor.heading_rad,
            mode: self.state.fleet_mode(id)?,
            ships,
        })
    }

    // --- Last-known (doc 16 §5).

    /// Every remembered fleet, in identity order (commanders only, D-181).
    pub fn last_known_fleets(&self) -> impl Iterator<Item = &'a LastKnownFleet> + 'a {
        self.knowledge
            .and_then(|k| k.memory.as_ref())
            .into_iter()
            .flat_map(|m| m.fleets.values())
    }

    /// The remembered fleets last seen in `system`.
    pub fn last_known_fleets_in(
        &self,
        system: SystemId,
    ) -> impl Iterator<Item = &'a LastKnownFleet> + 'a {
        self.last_known_fleets().filter(move |f| f.system == system)
    }

    /// A remembered fleet.
    pub fn last_known_fleet(&self, id: FleetId) -> Option<&'a LastKnownFleet> {
        self.knowledge?.memory.as_ref()?.fleets.get(&id)
    }

    /// Every remembered holding, in identity order.
    pub fn last_known_holdings(&self) -> impl Iterator<Item = &'a LastKnownHolding> + 'a {
        self.knowledge
            .and_then(|k| k.memory.as_ref())
            .into_iter()
            .flat_map(|m| m.holdings.values())
    }

    /// A remembered holding.
    pub fn last_known_holding(&self, id: HoldingId) -> Option<&'a LastKnownHolding> {
        self.knowledge?.memory.as_ref()?.holdings.get(&id)
    }

    // --- Lane traffic (doc 16 §4).

    /// Observed departures, by fleet (D-179).
    pub fn lane_traffic(&self) -> impl Iterator<Item = &'a LaneTraffic> + 'a {
        self.knowledge.into_iter().flat_map(|k| k.traffic.values())
    }

    // --- Announcements and events.

    /// The announcements among `announcements` (a tick's outputs) addressed to this
    /// observer: public ones, and for a commander those that name it (doc 16 §2).
    pub fn addressed<'b>(
        &self,
        announcements: &'b [Announcement],
    ) -> impl Iterator<Item = &'b Announcement> + 'b {
        let me = self.commander();
        announcements.iter().filter(move |a| match me {
            Some(me) => a.to.includes(me),
            None => matches!(a.to, crate::announce::Recipients::Public),
        })
    }

    /// This observer's visibility events among `events` (a tick's outputs).
    pub fn visibility_events<'b>(
        &self,
        events: &'b [VisibilityEvent],
    ) -> impl Iterator<Item = &'b VisibilityEvent> + 'b {
        let me = self.observer;
        events.iter().filter(move |e| event_observer(e) == me)
    }
}

/// Whose event `event` is.
const fn event_observer(event: &VisibilityEvent) -> Owner {
    match *event {
        VisibilityEvent::FleetSighted { observer, .. }
        | VisibilityEvent::FleetLost { observer, .. }
        | VisibilityEvent::HoldingSighted { observer, .. }
        | VisibilityEvent::HoldingLost { observer, .. }
        | VisibilityEvent::DepartureObserved { observer, .. } => observer,
    }
}

fn public_holding(state: &UniverseState, id: HoldingId) -> Option<PublicHolding> {
    let h = state.holdings.get(&id)?;
    let site = state.galaxy.site(h.site)?;
    Some(PublicHolding {
        id,
        owner: h.owner,
        site: h.site,
        system: site.system,
        outpost: !h.core_complete,
    })
}

/// The system a fleet at `place` is in: in space, or docked at a holding there.
fn place_system(state: &UniverseState, place: Place) -> Option<SystemId> {
    match place {
        Place::InSystem(system) => Some(system),
        Place::Docked(holding) => public_holding(state, holding).map(|h| h.system),
        Place::InTransit { .. } | Place::EmergencyJump { .. } => None,
    }
}

/// A ship as a sensor shows it.
fn sensed_ship(state: &UniverseState, id: ShipId) -> Option<SensedShip> {
    let (class, owner, fleet) = state
        .entities
        .with::<Ship, _>(id.sim_id(), |s| (s.class, s.owner, s.fleet))?;
    let motion = state.entities.get::<Motion>(id.sim_id())?;
    let (left, max) = state.ship_hull(id)?;
    Some(SensedShip {
        id,
        fleet,
        owner,
        class,
        position: motion.position,
        velocity: motion.velocity,
        heading_rad: motion.heading_rad,
        hull_permille: permille(u64::from(left), u64::from(max)),
        shrouded: state.entities.has::<Shroud>(id.sim_id()),
    })
}
