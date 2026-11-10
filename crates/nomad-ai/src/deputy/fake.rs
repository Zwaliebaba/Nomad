//! A fake [`View`] for the Deputy's unit tests (A1 plan §10 item 1: the AI can be tested
//! against a fake view).

use std::collections::BTreeMap;

use nomad_types::{
    BuildItem, Command, CommanderId, DirectControl, DoctrinePreset, DoctrineValues, FleetDoctrine,
    FleetId, GateId, HoldingDoctrine, HoldingId, Location, Owner, PositionMetres, PowerProfile,
    ReplacementEntry, RulesOfEngagement, ShipClass, SimId, Stock, SystemId, Tick,
};

use crate::inbox::InboxEntry;
use crate::view::{
    BuildCheck, DeputyParams, FleetPlace, HoldingDefence, OwnConvoy, OwnFleet, OwnHolding,
    RouteHop, RouteQuery, RouteSummary, RouteTo, SensedFleet, View,
};

/// The rules file's Deputy values.
pub(crate) const PARAMS: DeputyParams = DeputyParams {
    alert_window_s: 60,
    attacker_window_s: 30,
    flak_trigger_radius_m: 3_000,
    evacuation_resume_s: 60,
    anchor_radius_m: 1_000,
};

/// A fleet with nothing to report: full hull, no order, weapons free, not engaged. Tests
/// set `id`, `place`, `ships` and `busy`.
pub(crate) fn idle_fleet() -> OwnFleet {
    OwnFleet {
        id: FleetId(id(0)),
        place: FleetPlace::Travelling,
        ships: Vec::new(),
        busy: false,
        hull: 100,
        max_hull: 100,
        order: None,
        rules: RulesOfEngagement::WeaponsFree,
        engaged: false,
    }
}

/// Balanced, as the rules file has it (doc 13 §9).
pub(crate) fn balanced() -> DoctrineValues {
    DoctrineValues {
        holding: HoldingDoctrine {
            zone_radius_m: 10_000,
            zone_rules: RulesOfEngagement::WeaponsFree,
            alert_profile: PowerProfile::ShieldsFirst,
            surge_below_permille: 300,
            flak_light_hostiles: Some(8),
            evacuation_radius_m: 20_000,
            rebuild_modules: true,
        },
        leash_m: 12_000,
        retreat_below_permille: 400,
        repair_at_dock: true,
        replacement_budget_permille: 300,
        garrison_share_permille: 500,
    }
}

pub(crate) fn id(raw: u64) -> SimId {
    SimId::from_raw(raw)
}

pub(crate) fn at(system: u64, x: f64) -> Location {
    Location {
        system: SystemId(id(system)),
        position: PositionMetres::new(x, 0.0),
    }
}

/// Everything a test sets.
#[derive(Clone, Debug)]
pub(crate) struct Fake {
    pub(crate) now: Tick,
    pub(crate) me: CommanderId,
    pub(crate) inbox: Vec<InboxEntry>,
    pub(crate) income: Stock,
    pub(crate) replacement: Vec<ReplacementEntry>,
    pub(crate) holding_doctrine: HoldingDoctrine,
    pub(crate) fleets: BTreeMap<FleetId, OwnFleet>,
    pub(crate) holdings: Vec<OwnHolding>,
    pub(crate) checks: BTreeMap<HoldingId, BuildCheck>,
    pub(crate) rally: BTreeMap<HoldingId, FleetId>,
    pub(crate) price: Stock,
    /// Route times from anywhere to a holding, ms.
    pub(crate) route_ms: BTreeMap<HoldingId, u64>,
    pub(crate) convoys: Vec<OwnConvoy>,
    /// Routes by destination, as `(gates avoided, hops)`: the first whose avoided gates the
    /// query avoids answers.
    pub(crate) routes: Vec<(RouteTo, Vec<GateId>, Vec<RouteHop>)>,
    /// Sensed fleets, by system.
    pub(crate) sensed: Vec<(SystemId, SensedFleet)>,
    pub(crate) picture: Tick,
    pub(crate) defence: BTreeMap<HoldingId, HoldingDefence>,
    pub(crate) profiles: BTreeMap<HoldingId, PowerProfile>,
    /// Commands the validator refuses; everything else it accepts.
    pub(crate) refused: Vec<Command>,
    /// Fleets that cannot dock anywhere.
    pub(crate) no_room: Vec<FleetId>,
    pub(crate) control: BTreeMap<FleetId, DirectControl>,
    /// Fleet doctrine overrides.
    pub(crate) fleet_roles: BTreeMap<FleetId, FleetDoctrine>,
}

impl Fake {
    pub(crate) fn new() -> Self {
        Self {
            now: Tick::new(1),
            me: CommanderId(1),
            inbox: Vec::new(),
            income: Stock::ZERO,
            replacement: Vec::new(),
            holding_doctrine: balanced().holding,
            fleets: BTreeMap::new(),
            holdings: Vec::new(),
            checks: BTreeMap::new(),
            rally: BTreeMap::new(),
            price: Stock::units(100, 0),
            route_ms: BTreeMap::new(),
            convoys: Vec::new(),
            routes: Vec::new(),
            sensed: Vec::new(),
            picture: Tick::new(0),
            defence: BTreeMap::new(),
            profiles: BTreeMap::new(),
            refused: Vec::new(),
            no_room: Vec::new(),
            control: BTreeMap::new(),
            fleet_roles: BTreeMap::new(),
        }
    }
}

impl View for Fake {
    fn now(&self) -> Tick {
        self.now
    }

    fn observer(&self) -> Owner {
        Owner::Commander(self.me)
    }

    fn inbox(&self) -> &[InboxEntry] {
        &self.inbox
    }

    fn doctrine_preset(&self) -> Option<DoctrinePreset> {
        Some(DoctrinePreset::Balanced)
    }

    fn preset_values(&self, _preset: DoctrinePreset) -> DoctrineValues {
        balanced()
    }

    fn holding_doctrine(&self, holding: HoldingId) -> Option<HoldingDoctrine> {
        self.holdings
            .iter()
            .any(|h| h.id == holding)
            .then_some(self.holding_doctrine)
    }

    fn fleet_doctrine(&self, fleet: FleetId) -> Option<FleetDoctrine> {
        self.fleets.get(&fleet).map(|_| {
            self.fleet_roles
                .get(&fleet)
                .cloned()
                .unwrap_or_else(|| balanced().fleet())
        })
    }

    fn replacement_list(&self) -> Vec<ReplacementEntry> {
        self.replacement.clone()
    }

    fn direct_control(&self, fleet: FleetId) -> Option<DirectControl> {
        self.control.get(&fleet).copied()
    }

    fn deputy_params(&self) -> DeputyParams {
        PARAMS
    }

    fn own_fleet_ids(&self) -> Vec<FleetId> {
        self.fleets.keys().copied().collect()
    }

    fn defence(&self, holding: HoldingId) -> Option<HoldingDefence> {
        self.defence.get(&holding).copied()
    }

    fn power_profile(&self, holding: HoldingId) -> Option<PowerProfile> {
        self.holdings
            .iter()
            .any(|h| h.id == holding)
            .then(|| self.profiles.get(&holding).copied().unwrap_or_default())
    }

    fn can_dock(&self, fleet: FleetId, _holding: HoldingId) -> bool {
        !self.no_room.contains(&fleet)
    }

    fn accepts(&self, command: &Command) -> bool {
        !self.refused.contains(command)
    }

    fn picture_tick(&self) -> Tick {
        self.picture
    }

    fn anything_sensed_in(&self, system: SystemId) -> bool {
        self.sensed.iter().any(|(s, _)| *s == system)
    }

    fn sensed_fleets_in(&self, system: SystemId) -> Vec<SensedFleet> {
        self.sensed
            .iter()
            .filter(|(s, _)| *s == system)
            .map(|(_, f)| f.clone())
            .collect()
    }

    fn class_armed(&self, class: ShipClass) -> bool {
        class != ShipClass::Mule
    }

    fn class_hull(&self, _class: ShipClass) -> u64 {
        100
    }

    fn class_light(&self, class: ShipClass) -> bool {
        matches!(
            class,
            ShipClass::Vx | ShipClass::Lancer | ShipClass::RaiderStrikeCraft
        )
    }

    fn multiplier_percent(&self, shooter: ShipClass, target: ShipClass) -> u32 {
        // The VX's light cannon: 100% against Light, 50% otherwise (enough for the tests).
        match (self.class_armed(shooter), self.class_light(target)) {
            (false, _) => 0,
            (true, true) => 100,
            (true, false) => 50,
        }
    }

    fn own_fleet(&self, fleet: FleetId) -> Option<OwnFleet> {
        self.fleets.get(&fleet).cloned()
    }

    fn own_holdings(&self) -> Vec<OwnHolding> {
        self.holdings.clone()
    }

    fn own_convoys(&self) -> Vec<OwnConvoy> {
        self.convoys.clone()
    }

    fn income_last_hour(&self) -> Option<Stock> {
        Some(self.income)
    }

    fn rally(&self, holding: HoldingId) -> Option<FleetId> {
        self.rally.get(&holding).copied()
    }

    fn build_cost(&self, _holding: HoldingId, _item: BuildItem) -> Option<Stock> {
        Some(self.price)
    }

    fn build_check(&self, holding: HoldingId, _item: BuildItem) -> BuildCheck {
        self.checks.get(&holding).copied().unwrap_or(BuildCheck::Ok)
    }

    fn gate_location(&self, gate: GateId) -> Option<Location> {
        Some(Location {
            system: SystemId(gate.sim_id()),
            position: PositionMetres::ORIGIN,
        })
    }

    fn holding_location(&self, holding: HoldingId) -> Option<Location> {
        let system = self.holdings.iter().find(|h| h.id == holding)?.system;
        Some(Location {
            system,
            position: PositionMetres::ORIGIN,
        })
    }

    fn route(&self, query: &RouteQuery) -> Option<RouteSummary> {
        if let Some((_, _, hops)) = self.routes.iter().find(|(to, avoids, _)| {
            *to == query.to && query.avoid.iter().all(|g| avoids.contains(g))
        }) {
            return Some(RouteSummary {
                duration_ms: 1,
                hops: hops.clone(),
            });
        }
        let RouteTo::Holding(holding) = query.to else {
            return None;
        };
        if !self.routes.is_empty() {
            return None;
        }
        Some(RouteSummary {
            duration_ms: *self.route_ms.get(&holding)?,
            hops: Vec::new(),
        })
    }
}
