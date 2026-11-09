//! A fake [`View`] for the raider director's unit tests (A1 plan §10 item 1: the AI can be
//! tested against a fake view): one NPC faction with its raid groups and what it senses.

use std::collections::BTreeMap;

use nomad_types::{
    BuildItem, Command, DirectControl, DoctrinePreset, DoctrineValues, FactionId, FleetDoctrine,
    FleetId, GateId, HoldingDoctrine, HoldingId, Location, Owner, PositionMetres, PowerProfile,
    ReplacementEntry, ShipClass, ShipId, Stock, SystemId, Tick,
};

use crate::inbox::InboxEntry;
use crate::view::{
    BuildCheck, DeputyParams, HoldingDefence, OwnConvoy, OwnFleet, OwnHolding, PublicHolding,
    RaidOrder, RaidRules, RouteQuery, RouteSummary, SensedFleet, SensedHolding, SensedShip, View,
};

/// A detected ship of another owner, as a test sets it; the fake groups them into fleets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Contact {
    pub(crate) ship: ShipId,
    pub(crate) fleet: FleetId,
    pub(crate) owner: Owner,
    pub(crate) class: ShipClass,
    pub(crate) position: PositionMetres,
    pub(crate) shrouded: bool,
}

/// The rules file's values (D-081, D-222; doc 15 §5.3; D-055).
pub(crate) const RULES: RaidRules = RaidRules {
    clock_ticks: 6_000,
    loss_limit_permille: 500,
    withdraw_ratio_permille: 2_000,
    withdraw_radius_m: 5_000,
    turret_reach_m: 2_500,
    shroud_ticks: 100,
    interval_ticks: 5,
};

/// Everything a test sets.
#[derive(Clone, Debug)]
pub(crate) struct Fake {
    pub(crate) now: Tick,
    pub(crate) faction: FactionId,
    pub(crate) inbox: Vec<InboxEntry>,
    pub(crate) fleets: BTreeMap<FleetId, OwnFleet>,
    pub(crate) orders: Vec<RaidOrder>,
    pub(crate) contacts: BTreeMap<SystemId, Vec<Contact>>,
    pub(crate) sensed: BTreeMap<SystemId, Vec<SensedHolding>>,
    pub(crate) public: BTreeMap<SystemId, Vec<PublicHolding>>,
}

impl Fake {
    pub(crate) fn new() -> Self {
        Self {
            now: Tick::new(1),
            faction: FactionId(1),
            inbox: Vec::new(),
            fleets: BTreeMap::new(),
            orders: Vec::new(),
            contacts: BTreeMap::new(),
            sensed: BTreeMap::new(),
            public: BTreeMap::new(),
        }
    }
}

impl View for Fake {
    fn now(&self) -> Tick {
        self.now
    }

    fn observer(&self) -> Owner {
        Owner::Npc(self.faction)
    }

    fn inbox(&self) -> &[InboxEntry] {
        &self.inbox
    }

    fn doctrine_preset(&self) -> Option<DoctrinePreset> {
        None
    }

    fn preset_values(&self, _preset: DoctrinePreset) -> DoctrineValues {
        panic!("the director reads no doctrine")
    }

    fn holding_doctrine(&self, _holding: HoldingId) -> Option<HoldingDoctrine> {
        None
    }

    fn fleet_doctrine(&self, _fleet: FleetId) -> Option<FleetDoctrine> {
        None
    }

    fn replacement_list(&self) -> Vec<ReplacementEntry> {
        Vec::new()
    }

    fn direct_control(&self, _fleet: FleetId) -> Option<DirectControl> {
        None
    }

    fn deputy_params(&self) -> DeputyParams {
        panic!("the director reads no Deputy parameters")
    }

    fn own_fleet_ids(&self) -> Vec<FleetId> {
        self.fleets.keys().copied().collect()
    }

    fn defence(&self, _holding: HoldingId) -> Option<HoldingDefence> {
        None
    }

    fn power_profile(&self, _holding: HoldingId) -> Option<PowerProfile> {
        None
    }

    fn can_dock(&self, _fleet: FleetId, _holding: HoldingId) -> bool {
        false
    }

    fn accepts(&self, _command: &Command) -> bool {
        true
    }

    fn picture_tick(&self) -> Tick {
        self.now
    }

    fn anything_sensed_in(&self, system: SystemId) -> bool {
        self.contacts.get(&system).is_some_and(|c| !c.is_empty())
            || self.sensed.get(&system).is_some_and(|h| !h.is_empty())
    }

    fn sensed_fleets_in(&self, system: SystemId) -> Vec<SensedFleet> {
        let mut fleets: BTreeMap<FleetId, SensedFleet> = BTreeMap::new();
        for c in self.contacts.get(&system).into_iter().flatten() {
            fleets
                .entry(c.fleet)
                .or_insert_with(|| SensedFleet {
                    id: c.fleet,
                    owner: c.owner,
                    position: c.position,
                    ships: Vec::new(),
                })
                .ships
                .push(SensedShip {
                    id: c.ship,
                    class: c.class,
                    position: c.position,
                    shrouded: c.shrouded,
                });
        }
        fleets.into_values().collect()
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

    fn multiplier_percent(&self, _shooter: ShipClass, _target: ShipClass) -> u32 {
        100
    }

    fn own_fleet(&self, fleet: FleetId) -> Option<OwnFleet> {
        self.fleets.get(&fleet).cloned()
    }

    fn own_holdings(&self) -> Vec<OwnHolding> {
        Vec::new()
    }

    fn own_convoys(&self) -> Vec<OwnConvoy> {
        Vec::new()
    }

    fn income_last_hour(&self) -> Option<Stock> {
        None
    }

    fn rally(&self, _holding: HoldingId) -> Option<FleetId> {
        None
    }

    fn build_cost(&self, _holding: HoldingId, _item: BuildItem) -> Option<Stock> {
        None
    }

    fn build_check(&self, _holding: HoldingId, _item: BuildItem) -> BuildCheck {
        BuildCheck::CannotBuild
    }

    fn gate_location(&self, _gate: GateId) -> Option<Location> {
        None
    }

    fn holding_location(&self, _holding: HoldingId) -> Option<Location> {
        None
    }

    fn route(&self, _query: &RouteQuery) -> Option<RouteSummary> {
        None
    }

    fn raid_orders(&self) -> Vec<RaidOrder> {
        self.orders.clone()
    }

    fn raid_rules(&self) -> Option<RaidRules> {
        Some(RULES)
    }

    fn ship_value(&self, class: ShipClass) -> u64 {
        // D-220.
        match class {
            ShipClass::Vx => 1_200,
            ShipClass::Lancer => 2_300,
            ShipClass::Warden => 5_800,
            ShipClass::Mule => 1_500,
            ShipClass::RaiderStrikeCraft => 1_600,
            ShipClass::HeavyCruiser => 17_000,
        }
    }

    fn holdings_in(&self, system: SystemId) -> Vec<PublicHolding> {
        self.public.get(&system).cloned().unwrap_or_default()
    }

    fn sensed_holdings_in(&self, system: SystemId) -> Vec<SensedHolding> {
        self.sensed.get(&system).cloned().unwrap_or_default()
    }
}
