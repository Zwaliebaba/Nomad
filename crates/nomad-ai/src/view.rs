//! The AI's whole world: read-only queries by information class (D-077, D-199; S7 plan
//! §4.7, A1 plan §4.1).
//!
//! `nomad-sim` implements this trait over its `CommanderView`. Every query answers only what
//! the observer may know: its own assets, public data, what its sensors detect, and its own
//! inbox. No query says whether another commander is human or AI, whether they are online,
//! or what their doctrine is (D-075, D-078; A1 plan §4.8). Calls are monomorphised, so there
//! is no dynamic dispatch, and the AI can be tested against a fake view.
//!
//! Rules values the AI needs (costs, what a holding can build, route times) are queries
//! too, answered by the simulation's own rules and validator (A1 plan §4.1 "Cost"). Times are
//! whole milliseconds, so the AI compares integers (D-049, A1 plan §4.6).

use nomad_types::{
    BuildItem, Command, DirectControl, DoctrinePreset, DoctrineValues, FleetDoctrine, FleetId,
    FleetOrder, GateId, HoldingDoctrine, HoldingId, Location, Owner, PositionMetres, PowerProfile,
    ReplacementEntry, RulesOfEngagement, ShipClass, ShipId, Stock, SystemId, Tick,
};
use serde::{Deserialize, Serialize};

use crate::inbox::InboxEntry;

/// An own fleet as the AI reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnFleet {
    /// The fleet.
    pub id: FleetId,
    /// Where it is.
    pub place: FleetPlace,
    /// Its ships with their classes, in identity order.
    pub ships: Vec<(ShipId, ShipClass)>,
    /// Whether it has an active order or a queue.
    pub busy: bool,
    /// Hull points left, summed over its ships.
    pub hull: u64,
    /// Hull points of its ships undamaged, summed.
    pub max_hull: u64,
    /// The order it is carrying out.
    pub order: Option<FleetOrder>,
    /// Its rules of engagement.
    pub rules: RulesOfEngagement,
    /// Whether it is engaged (D-114).
    pub engaged: bool,
}

/// Another owner's ship that the observer detects (D-072, D-174).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SensedShip {
    /// The ship.
    pub id: ShipId,
    /// Its class.
    pub class: ShipClass,
    /// Where it is now.
    pub position: PositionMetres,
    /// Under the arrival shroud: seen, but it cannot be targeted (D-055).
    pub shrouded: bool,
}

/// Another owner's fleet with at least one detected ship, listing only those (D-174).
#[derive(Clone, Debug, PartialEq)]
pub struct SensedFleet {
    /// The fleet.
    pub id: FleetId,
    /// Its owner: hostile, as every other owner is (D-040).
    pub owner: Owner,
    /// Its anchor.
    pub position: PositionMetres,
    /// Its detected ships, in identity order.
    pub ships: Vec<SensedShip>,
}

/// One shield sector of an own holding (S5b).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sector {
    /// Strength, milli-points.
    pub strength_milli: u64,
    /// Maximum, milli-points.
    pub max_milli: u64,
    /// Open in a siege: SHLD SURGE is refused on it (doc 15 §5.3).
    pub open: bool,
}

/// An own holding's dome (S5b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoldingDefence {
    /// Whether it projects a dome.
    pub dome: bool,
    /// The sectors, in pylon slot order.
    pub sectors: [Sector; 4],
}

/// The Deputy's timings and distances from the rules file (D-211; A1 plan §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeputyParams {
    /// An alert ends this long after its last event (doc 13 §4).
    pub alert_window_s: u64,
    /// Target priority 1 and *return fire* look this far back in the inbox (A1 plan §5.5,
    /// D-145).
    pub attacker_window_s: u64,
    /// FLAK SCREEN counts light hostiles within this distance of the dome centre (doc 13
    /// §9).
    pub flak_trigger_radius_m: u32,
    /// An evacuated convoy resumes this long after no hostile is within its radius (D-210).
    pub evacuation_resume_s: u64,
    /// A fleet within this distance of its holding's launch point is by the core
    /// (*Proposed*).
    pub anchor_radius_m: u32,
}

/// An own fleet's convoy route (D-134), as the AI reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnConvoy {
    /// The fleet.
    pub fleet: FleetId,
    /// The stops, in order.
    pub route: Vec<HoldingId>,
    /// The index of the stop it heads for.
    pub next: usize,
    /// The contested gate it holds for at its last stop, if it is holding (D-055).
    pub held_at: Option<GateId>,
}

/// Where an own fleet is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FleetPlace {
    /// In space, at its anchor.
    InSpace(Location),
    /// Docked at a holding.
    Docked(HoldingId),
    /// In lane transit or an emergency jump: it takes no orders until it arrives.
    Travelling,
}

/// An own holding as the AI reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OwnHolding {
    /// The holding.
    pub id: HoldingId,
    /// Its system.
    pub system: SystemId,
    /// Whether its core is complete (an outpost's is not, D-041).
    pub complete: bool,
    /// Its site: the centre of its dome and defended zone.
    pub centre: PositionMetres,
}

/// Whether a holding could queue an item now, as the validator would answer (D-077).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildCheck {
    /// It could.
    Ok,
    /// Its queue is full.
    QueueFull,
    /// The slot has a module or one is queued for it.
    SlotTaken,
    /// It cannot build the item at all: not its holding, an outpost, no dry dock for a
    /// frigate, a class or slot it cannot take.
    CannotBuild,
}

/// Where a route query ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteTo {
    /// Docked at this holding's launch point.
    Holding(HoldingId),
    /// Anywhere in this system: the arrival is the end.
    System(SystemId),
}

/// Whose cruise speed times a route's in-system legs (doc 15 §3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteSpeed {
    /// A ship of this class.
    Class(ShipClass),
    /// An own fleet: its slowest ship, as its own orders are routed.
    Fleet(FleetId),
}

/// A route query over lanes, timed as the simulation times routes (doc 15 §3.4).
#[derive(Clone, Debug, PartialEq)]
pub struct RouteQuery {
    /// Where it starts.
    pub from: Location,
    /// Where it ends.
    pub to: RouteTo,
    /// Whose cruise speed times the in-system legs.
    pub speed: RouteSpeed,
    /// Gates the route may neither depart from nor arrive at.
    pub avoid: Vec<GateId>,
}

/// One jump of a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteHop {
    /// The gate it departs from.
    pub from_gate: GateId,
    /// The gate it arrives at.
    pub to_gate: GateId,
    /// The system it arrives in.
    pub to_system: SystemId,
}

/// The fastest route a query found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteSummary {
    /// Its duration, whole milliseconds.
    pub duration_ms: u64,
    /// Its jumps, in order; empty inside one system.
    pub hops: Vec<RouteHop>,
}

/// What one observer may know of the Universe, as the AI reads it.
pub trait View {
    /// The tick being computed. What the AI issues now applies on a later tick (doc 11
    /// §4.1, D-200).
    fn now(&self) -> Tick;

    /// Whose view this is.
    fn observer(&self) -> Owner;

    /// The observer's inbox, oldest first (D-202).
    fn inbox(&self) -> &[InboxEntry];

    // --- Doctrine: the observer's own (doc 12 §5.2; D-075), and the public presets.

    /// The observer's doctrine preset; `None` for an NPC faction.
    fn doctrine_preset(&self) -> Option<DoctrinePreset>;

    /// What a preset sets: rules data, the same for everyone (D-082, D-211).
    fn preset_values(&self, preset: DoctrinePreset) -> DoctrineValues;

    /// The doctrine that applies to an own holding: its override or the preset's.
    fn holding_doctrine(&self, holding: HoldingId) -> Option<HoldingDoctrine>;

    /// The doctrine that applies to an own fleet: its override or the preset's.
    fn fleet_doctrine(&self, fleet: FleetId) -> Option<FleetDoctrine>;

    /// The observer's replacement list, in order (D-208).
    fn replacement_list(&self) -> Vec<ReplacementEntry>;

    /// Whether an own fleet is under direct control (RK-10).
    fn direct_control(&self, fleet: FleetId) -> Option<DirectControl>;

    /// The Deputy's timings and distances: rules data (D-211).
    fn deputy_params(&self) -> DeputyParams;

    // --- Own assets and economy (doc 16 §2).

    /// The observer's fleets, in identity order.
    fn own_fleet_ids(&self) -> Vec<FleetId>;

    /// An own fleet.
    fn own_fleet(&self, fleet: FleetId) -> Option<OwnFleet>;

    /// An own holding's dome and sectors.
    fn defence(&self, holding: HoldingId) -> Option<HoldingDefence>;

    /// An own holding's power profile.
    fn power_profile(&self, holding: HoldingId) -> Option<PowerProfile>;

    /// Whether an own fleet could dock at an own holding now: room (D-102), a dry dock for
    /// a frigate.
    fn can_dock(&self, fleet: FleetId, holding: HoldingId) -> bool;

    /// Whether the validator would accept `command` from the observer now, as it would
    /// answer a client (D-077, D-123).
    fn accepts(&self, command: &Command) -> bool;

    // --- Sensed (doc 16 §3; D-173): the last sensors pass, through the per-system index.

    /// The tick of the last sensors pass.
    fn picture_tick(&self) -> Tick;

    /// Whether the last pass detected anything of another owner in `system`: the cheap
    /// check of a calm evaluation (A1 plan §4.5).
    fn anything_sensed_in(&self, system: SystemId) -> bool;

    /// The detected fleets in `system`, in identity order.
    fn sensed_fleets_in(&self, system: SystemId) -> Vec<SensedFleet>;

    // --- Rules (public, D-104).

    /// Whether a class carries a weapon.
    fn class_armed(&self, class: ShipClass) -> bool;

    /// The hull points of an undamaged ship of a class.
    fn class_hull(&self, class: ShipClass) -> u64;

    /// Whether a class is of the Light size (doc 15 §3.1).
    fn class_light(&self, class: ShipClass) -> bool;

    /// The multiplier, percent, of `shooter`'s first weapon against a ship of `target`'s
    /// size (D-052); 0 for an unarmed class.
    fn multiplier_percent(&self, shooter: ShipClass, target: ShipClass) -> u32;

    /// The observer's holdings and outposts, in identity order.
    fn own_holdings(&self) -> Vec<OwnHolding>;

    /// The observer's convoys, by fleet identity.
    fn own_convoys(&self) -> Vec<OwnConvoy>;

    /// The observer's production over the last hour (S4 plan §4.8); `None` for an NPC
    /// faction.
    fn income_last_hour(&self) -> Option<Stock>;

    /// The fleet new ships of an own holding join (RALLY).
    fn rally(&self, holding: HoldingId) -> Option<FleetId>;

    /// What `item` would cost queued at an own holding now (D-068, D-189).
    fn build_cost(&self, holding: HoldingId, item: BuildItem) -> Option<Stock>;

    /// Whether an own holding could queue `item` now.
    fn build_check(&self, holding: HoldingId, item: BuildItem) -> BuildCheck;

    // --- Public topology and routes (D-044).

    /// Where a gate is.
    fn gate_location(&self, gate: GateId) -> Option<Location>;

    /// Where an own holding's ships launch and dock: its launch point.
    fn holding_location(&self, holding: HoldingId) -> Option<Location>;

    /// The fastest route of a query, or `None` if there is none.
    fn route(&self, query: &RouteQuery) -> Option<RouteSummary>;
}
