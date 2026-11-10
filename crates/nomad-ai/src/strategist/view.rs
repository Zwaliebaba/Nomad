//! What the Strategist reads besides the Deputy's [`View`] (A2 plan §4.2; D-077, D-199).
//!
//! Everything comes from the commander's `CommanderView`, by information class (S7 plan
//! §4.7): its own economy, holdings, fleets and schedule; the public sites, systems,
//! ownership and season clock (D-044, D-075); and what its sensors detect or remember
//! (doc 16 §3–§5). Rules values it needs (rates, costs, the Strategist's parameters) are
//! queries too, the same for everyone (A1 plan §4.1 "Cost"). Nothing here says whether
//! another commander is human or AI, whether they are online, or what their doctrine is
//! (D-075, D-078).

use nomad_types::{
    BuildItem, CommanderId, DepositKind, Difficulty, FleetId, HoldingId, Location, ModuleKind,
    PositionMetres, ShipClass, ShipId, SiteId, Slot, Stock, SystemId, SystemKind, Tick,
};

use super::rules::{BlueprintMode, StrategistRules};
use crate::view::{RouteSpeed, RouteSummary, View};

/// The commander's own session, from its schedule (D-080, D-214).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionTimes {
    /// The first tick it is offline again; `None` when no end is known (a harness override
    /// keeps it online, or it has no schedule).
    pub ends: Option<Tick>,
    /// The tick the next session starts, if known.
    pub next_starts: Option<Tick>,
    /// The longest session of the schedule, ticks, if known.
    pub longest_ticks: Option<u64>,
}

/// The commander's own economy (S4 plan §4.8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EconomyStatus {
    /// The pool.
    pub pool: Stock,
    /// The pool cap (D-025).
    pub cap: Stock,
    /// Production over the last hour.
    pub income_last_hour: Stock,
    /// Command cost of ships and ships being built.
    pub command_used: u32,
    /// The command cap (D-129).
    pub command_cap: u32,
    /// What a core kit queued now would cost (D-189).
    pub core_kit_price: Stock,
}

/// A module in a slot of an own holding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleStatus {
    /// Its slot.
    pub slot: Slot,
    /// What it is.
    pub kind: ModuleKind,
    /// Whether it is still being built.
    pub building: bool,
    /// Whether it is knocked offline.
    pub disabled: bool,
}

/// An item of an own holding's build queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueuedItem {
    /// What it makes.
    pub item: BuildItem,
    /// Its price, fixed when it was queued (D-189 for kits).
    pub price: Stock,
    /// Whether it has been paid for and is building.
    pub started: bool,
}

/// An own holding's economy, as the Strategist reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldingStatus {
    /// The holding.
    pub id: HoldingId,
    /// Its site.
    pub site: SiteId,
    /// Its system.
    pub system: SystemId,
    /// Whether it is the mother station.
    pub mother: bool,
    /// Whether its core is complete (an outpost's is not).
    pub complete: bool,
    /// Every slot of its template, in order (D-130, D-131).
    pub slots: Vec<Slot>,
    /// Its modules, built or being built, in slot order.
    pub modules: Vec<ModuleStatus>,
    /// Its queue, in order.
    pub queue: Vec<QueuedItem>,
    /// Its local buffer, and the buffer's capacity (D-064).
    pub buffer: Stock,
    /// The buffer's capacity.
    pub buffer_cap: Stock,
    /// Whether its production goes straight to the pool (D-064).
    pub delivers_to_pool: bool,
    /// Finished core kits stored here.
    pub kits: u32,
    /// Ships docked here, and how many may dock (D-102).
    pub docked: u32,
    /// How many ships may dock here.
    pub docking_capacity: u32,
    /// The fleet new ships join (RALLY).
    pub rally: Option<FleetId>,
    /// When its core completed; `None` for a holding placed complete, such as the mother
    /// station (D-309).
    pub completed_at: Option<Tick>,
}

/// What a MULE carries (D-133).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MuleCargo {
    /// Alloy and crystal.
    pub stock: Stock,
    /// A core kit, which fills the hold.
    pub core_kit: bool,
}

/// A deposit on a site (D-066), public.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositInfo {
    /// Ore field or crystal vein.
    pub kind: DepositKind,
    /// The richness multiplier, per mille (×0.5, ×1, ×1.5).
    pub richness_permille: u32,
}

/// Who holds a site (D-044: ownership is public).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiteHolder {
    /// The commander.
    pub owner: CommanderId,
    /// The holding or outpost.
    pub holding: HoldingId,
    /// Whether its core is complete.
    pub complete: bool,
}

/// A site, public (D-044).
#[derive(Clone, Debug, PartialEq)]
pub struct SiteInfo {
    /// The site.
    pub id: SiteId,
    /// Its system.
    pub system: SystemId,
    /// Where it is.
    pub position: PositionMetres,
    /// Its deposits, in slot order.
    pub deposits: Vec<DepositInfo>,
    /// Who holds it.
    pub holder: Option<SiteHolder>,
    /// Whether a core kit may be deployed on it now: free, in a claimable system, and not
    /// reserved (S6b).
    pub free: bool,
}

/// A system, public (D-044).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemInfo {
    /// The system.
    pub id: SystemId,
    /// Its kind.
    pub kind: SystemKind,
    /// Whether it can be claimed and counts for control (D-041).
    pub claimable: bool,
    /// Its sites, in identity order.
    pub sites: Vec<SiteId>,
    /// The systems one lane away, in identity order.
    pub neighbours: Vec<SystemId>,
}

/// The economy's rules values the Strategist plans with (doc 14; D-104), the same for every
/// commander.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EconomyNumbers {
    /// Ore per hour of a harvester on a standard field, units.
    pub harvester_ore_per_h: u64,
    /// Alloy per hour a refinery can refine, units.
    pub refinery_alloy_per_h: u64,
    /// Crystal per hour of an extractor on a standard vein, units.
    pub extractor_crystal_per_h: u64,
    /// D-217's F, per mille.
    pub diminishing_yield_permille: u32,
    /// Power of a holding's core and of the mother station's, MW.
    pub core_supply_mw: u32,
    /// Power of the mother station's core, MW.
    pub mother_supply_mw: u32,
    /// What modules cost, draw and supply: `(kind, cost, draw MW, supply MW)`.
    pub modules: Vec<(ModuleKind, Stock, u32, u32)>,
    /// What ships cost, and their command cost.
    pub ships: Vec<(ShipClass, Stock, u32)>,
    /// What one storage module adds to the pool cap (D-025).
    pub rack: Stock,
    /// The most stops a convoy route may have (D-134).
    pub max_route_stops: usize,
    /// The longest build queue (D-068).
    pub queue_max: usize,
}

impl EconomyNumbers {
    /// A module's cost.
    pub fn module_cost(&self, kind: ModuleKind) -> Stock {
        self.modules
            .iter()
            .find(|m| m.0 == kind)
            .map_or(Stock::ZERO, |m| m.1)
    }

    /// A module's draw and supply, MW.
    pub fn module_power(&self, kind: ModuleKind) -> (u32, u32) {
        self.modules
            .iter()
            .find(|m| m.0 == kind)
            .map_or((0, 0), |m| (m.2, m.3))
    }

    /// A ship's cost and command cost, if it can be built.
    pub fn ship(&self, class: ShipClass) -> Option<(Stock, u32)> {
        self.ships.iter().find(|s| s.0 == class).map(|s| (s.1, s.2))
    }
}

/// The Strategist's parameters for this commander: the rules section and its difficulty's
/// row (D-079, D-211).
#[derive(Clone, Copy, Debug)]
pub struct StrategistParams<'a> {
    /// The rules section (A2 plan §4.1, §4.5).
    pub rules: &'a StrategistRules,
    /// The commander's difficulty.
    pub difficulty: Difficulty,
    /// The decision noise, per mille of a score (D-079).
    pub noise_permille: u32,
    /// How it chooses build orders (D-079, D-215).
    pub blueprints: BlueprintMode,
    /// The evaluation interval, ticks (D-079).
    pub interval_ticks: u64,
}

/// The Strategist's queries (A2 plan §4.2), on top of the Deputy's.
pub trait StrategicView: View {
    /// The Strategist's parameters for this commander.
    fn strategist(&self) -> StrategistParams<'_>;

    /// The commander's own session (D-214).
    fn session(&self) -> SessionTimes;

    /// The season's scheduled final tick (D-195), public.
    fn final_tick(&self) -> Tick;

    /// The commander's own economy; `None` for an NPC faction.
    fn economy(&self) -> Option<EconomyStatus>;

    /// An own holding's economy.
    fn holding_status(&self, holding: HoldingId) -> Option<HoldingStatus>;

    /// What an own MULE carries.
    fn mule_cargo(&self, ship: ShipId) -> Option<MuleCargo>;

    /// Every site, in identity order (D-044).
    fn sites(&self) -> Vec<SiteInfo>;

    /// Every system, in identity order (D-044).
    fn systems(&self) -> Vec<SystemInfo>;

    /// Ships of other owners the commander detects in `system` now, or remembers there
    /// from the last day (doc 16 §3–§5; D-076).
    fn hostile_ships_in(&self, system: SystemId) -> u32;

    /// The fastest route from `from` to a point, timed as the simulation times routes.
    fn route_to_point(
        &self,
        from: Location,
        to: Location,
        speed: RouteSpeed,
    ) -> Option<RouteSummary>;

    /// The economy's rules values.
    fn numbers(&self) -> EconomyNumbers;
}
