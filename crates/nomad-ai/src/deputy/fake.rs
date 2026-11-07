//! A fake [`View`] for the Deputy's unit tests (A1 plan §10 item 1: the AI can be tested
//! against a fake view).

use std::collections::BTreeMap;

use nomad_types::{
    BuildItem, CommanderId, DirectControl, DoctrinePreset, DoctrineValues, FleetDoctrine, FleetId,
    GateId, HoldingDoctrine, HoldingId, Location, Owner, PositionMetres, PowerProfile,
    ReplacementEntry, RulesOfEngagement, SimId, Stock, SystemId, Tick,
};

use crate::inbox::InboxEntry;
use crate::view::{
    BuildCheck, OwnConvoy, OwnFleet, OwnHolding, RouteHop, RouteQuery, RouteSummary, RouteTo, View,
};

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
        self.fleets.get(&fleet).map(|_| balanced().fleet())
    }

    fn replacement_list(&self) -> Vec<ReplacementEntry> {
        self.replacement.clone()
    }

    fn direct_control(&self, _fleet: FleetId) -> Option<DirectControl> {
        None
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
