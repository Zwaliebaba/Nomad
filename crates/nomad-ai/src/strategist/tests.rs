//! The Strategist's unit tests against a fake view (A2 plan §9 "Strategist tests").
//!
//! The rules values are the embedded rules file's, so the tests check the shipped numbers.

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{
    BuildItem, Command, CommanderId, DepositKind, Difficulty, DirectControl, DoctrineCommand,
    DoctrinePreset, DoctrineValues, EconomyCommand, FleetDoctrine, FleetId, FleetOrder, FleetRole,
    HoldingDoctrine, HoldingId, Location, ModuleKind, Owner, PositionMetres, PowerProfile,
    ReplacementEntry, RulesOfEngagement, ShipClass, ShipId, SimId, SiteId, Slot, Stock, SystemId,
    SystemKind, Tick, Ticks,
};

use super::*;
use crate::deputy::fake::idle_fleet;
use crate::inbox::InboxEntry;
use crate::view::{
    BuildCheck, DeputyParams, FleetPlace, HoldingDefence, OwnConvoy, OwnFleet, OwnHolding,
    RouteQuery, RouteSpeed, RouteSummary, SensedFleet, View,
};

fn rules() -> StrategistRules {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("../../../nomad-sim/rules/default.json")).unwrap();
    serde_json::from_value(json["ai"]["strategist"].clone()).unwrap()
}

fn balanced() -> DoctrineValues {
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

fn id(raw: u64) -> SimId {
    SimId::from_raw(raw)
}

const S1: SystemId = SystemId(SimId::from_raw(1));
const S2: SystemId = SystemId(SimId::from_raw(2));
const HAVEN: SystemId = SystemId(SimId::from_raw(3));
const MOTHER: HoldingId = HoldingId(SimId::from_raw(100));
const START: FleetId = FleetId(SimId::from_raw(200));
const HOUR: u64 = 36_000;

fn deposit(kind: DepositKind) -> DepositInfo {
    DepositInfo {
        kind,
        richness_permille: 1_000,
    }
}

fn site(raw: u64, system: SystemId, deposits: Vec<DepositInfo>) -> SiteInfo {
    SiteInfo {
        id: SiteId(id(raw)),
        system,
        position: PositionMetres::new(f64::from(u32::try_from(raw).unwrap()) * 1_000.0, 0.0),
        deposits,
        holder: None,
        free: true,
    }
}

struct Fake {
    now: Tick,
    rules: StrategistRules,
    session: SessionTimes,
    economy: EconomyStatus,
    holdings: Vec<HoldingStatus>,
    fleets: BTreeMap<FleetId, OwnFleet>,
    cargo: BTreeMap<ShipId, MuleCargo>,
    direct: BTreeSet<FleetId>,
    roles: BTreeMap<FleetId, FleetRole>,
    sites: Vec<SiteInfo>,
    systems: Vec<SystemInfo>,
    route_ms: u64,
    /// Added to a route that ends away from a holding (at a site).
    site_extra_ms: u64,
    inbox: Vec<InboxEntry>,
}

impl Fake {
    /// A mother station with an ore field in S1, which also has a crystal-vein site and an
    /// ore site; S2 next to it has an ore site; a raider haven beyond S2. The start kit: 3 VX
    /// and 2 MULE docked at the mother station; 6,000 A and 500 C.
    #[expect(clippy::too_many_lines, reason = "one fixture, field by field")]
    fn new() -> Self {
        let mut sites = vec![
            site(10, S1, vec![deposit(DepositKind::OreField)]),
            site(11, S1, vec![deposit(DepositKind::OreField)]),
            site(12, S1, vec![deposit(DepositKind::CrystalVein)]),
            site(13, S1, vec![]),
            site(20, S2, vec![deposit(DepositKind::OreField)]),
            site(21, S2, vec![deposit(DepositKind::OreField)]),
        ];
        sites[0].free = false;
        sites[0].holder = Some(SiteHolder {
            owner: CommanderId(1),
            holding: MOTHER,
            complete: true,
        });
        let module = |kind, slot| ModuleStatus {
            slot,
            kind,
            building: false,
            disabled: false,
        };
        let mother = HoldingStatus {
            id: MOTHER,
            site: SiteId(id(10)),
            system: S1,
            mother: true,
            complete: true,
            slots: mother_slots(),
            modules: vec![
                module(ModuleKind::Pylon, Slot::Pylon(0)),
                module(ModuleKind::Pylon, Slot::Pylon(1)),
                module(ModuleKind::Pylon, Slot::Pylon(2)),
                module(ModuleKind::Pylon, Slot::Pylon(3)),
                module(ModuleKind::Refinery, Slot::Inner(0)),
                module(ModuleKind::Storage, Slot::Inner(1)),
                module(ModuleKind::DryDock, Slot::Dock),
                module(ModuleKind::Solar, Slot::Solar),
                module(ModuleKind::Harvester, Slot::Deposit(0)),
            ],
            queue: Vec::new(),
            buffer: Stock::ZERO,
            buffer_cap: Stock::ZERO,
            delivers_to_pool: true,
            kits: 0,
            docked: 5,
            docking_capacity: 30,
            rally: None,
        };
        let mut fleets = BTreeMap::new();
        let ships = vec![
            (ShipId(id(201)), ShipClass::Vx),
            (ShipId(id(202)), ShipClass::Vx),
            (ShipId(id(203)), ShipClass::Vx),
            (ShipId(id(204)), ShipClass::Mule),
            (ShipId(id(205)), ShipClass::Mule),
        ];
        let mut cargo = BTreeMap::new();
        cargo.insert(ShipId(id(204)), MuleCargo::default());
        cargo.insert(ShipId(id(205)), MuleCargo::default());
        fleets.insert(
            START,
            OwnFleet {
                id: START,
                place: FleetPlace::Docked(MOTHER),
                ships,
                ..idle_fleet()
            },
        );
        Self {
            now: Tick::new(HOUR),
            rules: rules(),
            session: SessionTimes::default(),
            economy: EconomyStatus {
                pool: Stock::units(6_000, 500),
                cap: Stock::units(8_000, 1_000),
                income_last_hour: Stock::units(600, 0),
                command_used: 26,
                command_cap: 120,
                core_kit_price: Stock::units(6_000, 500),
            },
            holdings: vec![mother],
            fleets,
            cargo,
            direct: BTreeSet::new(),
            roles: BTreeMap::new(),
            sites,
            systems: vec![
                SystemInfo {
                    id: S1,
                    kind: SystemKind::Home,
                    claimable: true,
                    sites: (10..14).map(|r| SiteId(id(r))).collect(),
                    neighbours: vec![S2],
                },
                SystemInfo {
                    id: S2,
                    kind: SystemKind::Standard,
                    claimable: true,
                    sites: (20..22).map(|r| SiteId(id(r))).collect(),
                    neighbours: vec![S1, HAVEN],
                },
                SystemInfo {
                    id: HAVEN,
                    kind: SystemKind::RaiderHaven,
                    claimable: false,
                    sites: Vec::new(),
                    neighbours: vec![S2],
                },
            ],
            route_ms: 600_000,
            site_extra_ms: 0,
            inbox: Vec::new(),
        }
    }

    fn eval(&mut self, memory: &mut StrategistMemory) -> Vec<Command> {
        let mut out = Output::default();
        evaluate(&*self, memory, &mut out);
        out.commands.into_iter().map(|i| i.command).collect()
    }

    fn advance(&mut self, ticks: u64) {
        self.now = self.now.after(Ticks::new(ticks));
    }
}

fn mother_slots() -> Vec<Slot> {
    let mut slots = vec![Slot::Core];
    slots.extend((0..4).map(Slot::Pylon));
    slots.extend((0..6).map(Slot::Inner));
    slots.extend([Slot::Dock, Slot::Solar, Slot::Deposit(0)]);
    slots
}

impl View for Fake {
    fn now(&self) -> Tick {
        self.now
    }
    fn observer(&self) -> Owner {
        Owner::Commander(CommanderId(1))
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
    fn holding_doctrine(&self, _holding: HoldingId) -> Option<HoldingDoctrine> {
        Some(balanced().holding)
    }
    fn fleet_doctrine(&self, fleet: FleetId) -> Option<FleetDoctrine> {
        self.fleets.get(&fleet)?;
        Some(FleetDoctrine {
            role: self.roles.get(&fleet).cloned(),
            ..balanced().fleet()
        })
    }
    fn replacement_list(&self) -> Vec<ReplacementEntry> {
        Vec::new()
    }
    fn direct_control(&self, fleet: FleetId) -> Option<DirectControl> {
        self.direct
            .contains(&fleet)
            .then_some(DirectControl { retreat: false })
    }
    fn own_fleet(&self, fleet: FleetId) -> Option<OwnFleet> {
        self.fleets.get(&fleet).cloned()
    }
    fn deputy_params(&self) -> DeputyParams {
        crate::deputy::fake::PARAMS
    }
    fn own_fleet_ids(&self) -> Vec<FleetId> {
        self.fleets.keys().copied().collect()
    }
    fn defence(&self, _holding: HoldingId) -> Option<HoldingDefence> {
        None
    }
    fn power_profile(&self, holding: HoldingId) -> Option<PowerProfile> {
        self.holdings
            .iter()
            .any(|h| h.id == holding)
            .then(PowerProfile::default)
    }
    fn can_dock(&self, _fleet: FleetId, _holding: HoldingId) -> bool {
        true
    }
    fn accepts(&self, _command: &Command) -> bool {
        true
    }
    fn picture_tick(&self) -> Tick {
        self.now
    }
    fn anything_sensed_in(&self, _system: SystemId) -> bool {
        false
    }
    fn sensed_fleets_in(&self, _system: SystemId) -> Vec<SensedFleet> {
        Vec::new()
    }
    fn class_armed(&self, class: ShipClass) -> bool {
        class != ShipClass::Mule
    }
    fn class_hull(&self, _class: ShipClass) -> u64 {
        100
    }
    fn class_light(&self, class: ShipClass) -> bool {
        matches!(class, ShipClass::Vx | ShipClass::Lancer)
    }
    fn multiplier_percent(&self, _shooter: ShipClass, _target: ShipClass) -> u32 {
        100
    }
    fn own_holdings(&self) -> Vec<OwnHolding> {
        self.holdings
            .iter()
            .map(|h| OwnHolding {
                id: h.id,
                system: h.system,
                complete: h.complete,
                centre: PositionMetres::ORIGIN,
            })
            .collect()
    }
    fn own_convoys(&self) -> Vec<OwnConvoy> {
        Vec::new()
    }
    fn income_last_hour(&self) -> Option<Stock> {
        Some(self.economy.income_last_hour)
    }
    fn rally(&self, _holding: HoldingId) -> Option<FleetId> {
        None
    }
    fn build_cost(&self, _holding: HoldingId, _item: BuildItem) -> Option<Stock> {
        None
    }
    fn build_check(&self, holding: HoldingId, item: BuildItem) -> BuildCheck {
        let Some(h) = self.holdings.iter().find(|h| h.id == holding) else {
            return BuildCheck::CannotBuild;
        };
        if let BuildItem::Module { slot, .. } = item
            && (h.modules.iter().any(|m| m.slot == slot)
                || h.queue
                    .iter()
                    .any(|q| matches!(q.item, BuildItem::Module { slot: s, .. } if s == slot)))
        {
            return BuildCheck::SlotTaken;
        }
        BuildCheck::Ok
    }
    fn gate_location(&self, _gate: nomad_types::GateId) -> Option<Location> {
        None
    }
    fn holding_location(&self, holding: HoldingId) -> Option<Location> {
        let h = self.holdings.iter().find(|h| h.id == holding)?;
        Some(Location {
            system: h.system,
            position: PositionMetres::ORIGIN,
        })
    }
    fn route(&self, _query: &RouteQuery) -> Option<RouteSummary> {
        None
    }
}

impl StrategicView for Fake {
    fn strategist(&self) -> StrategistParams<'_> {
        StrategistParams {
            rules: &self.rules,
            difficulty: Difficulty::Normal,
            noise_permille: 100,
            blueprints: BlueprintMode::PerSiteKind,
            interval_ticks: 150,
        }
    }
    fn session(&self) -> SessionTimes {
        self.session
    }
    fn final_tick(&self) -> Tick {
        Tick::new(56 * 24 * HOUR)
    }
    fn economy(&self) -> Option<EconomyStatus> {
        Some(self.economy)
    }
    fn holding_status(&self, holding: HoldingId) -> Option<HoldingStatus> {
        self.holdings.iter().find(|h| h.id == holding).cloned()
    }
    fn mule_cargo(&self, ship: ShipId) -> Option<MuleCargo> {
        self.cargo.get(&ship).copied()
    }
    fn sites(&self) -> Vec<SiteInfo> {
        self.sites.clone()
    }
    fn systems(&self) -> Vec<SystemInfo> {
        self.systems.clone()
    }
    fn hostile_ships_in(&self, _system: SystemId) -> u32 {
        0
    }
    fn route_to_point(
        &self,
        from: Location,
        to: Location,
        _speed: RouteSpeed,
    ) -> Option<RouteSummary> {
        let mut ms = if from.system == to.system {
            60_000
        } else {
            self.route_ms
        };
        if to.position != PositionMetres::ORIGIN {
            ms += self.site_extra_ms;
        }
        Some(RouteSummary {
            duration_ms: ms,
            hops: Vec::new(),
        })
    }
    fn numbers(&self) -> EconomyNumbers {
        let units = Stock::units;
        EconomyNumbers {
            harvester_ore_per_h: 600,
            refinery_alloy_per_h: 900,
            extractor_crystal_per_h: 60,
            diminishing_yield_permille: 2_500,
            core_supply_mw: 120,
            mother_supply_mw: 520,
            modules: vec![
                (ModuleKind::Pylon, units(3_250, 50), 40, 0),
                (ModuleKind::Solar, units(3_000, 0), 0, 620),
                (ModuleKind::Harvester, units(2_000, 0), 60, 0),
                (ModuleKind::Refinery, units(3_500, 0), 120, 0),
                (ModuleKind::Extractor, units(2_500, 0), 60, 0),
                (ModuleKind::Storage, units(1_500, 0), 0, 0),
                (ModuleKind::Turret, units(2_450, 50), 40, 0),
                (ModuleKind::DryDock, units(5_000, 300), 150, 0),
            ],
            ships: vec![
                (ShipClass::Vx, units(1_200, 0), 6),
                (ShipClass::Lancer, units(2_000, 100), 10),
                (ShipClass::Warden, units(4_000, 600), 25),
                (ShipClass::Mule, units(1_500, 0), 4),
            ],
            rack: units(1_500, 400),
            max_route_stops: 8,
            queue_max: 24,
        }
    }
}

fn builds(commands: &[Command]) -> Vec<(HoldingId, BuildItem)> {
    commands
        .iter()
        .filter_map(|c| match c {
            Command::Economy(EconomyCommand::Build { holding, item }) => Some((*holding, *item)),
            _ => None,
        })
        .collect()
}

fn memory() -> StrategistMemory {
    let mut m = StrategistMemory::default();
    m.seed([3; 32]);
    m
}

#[test]
fn a_crystal_vein_is_claimed_first() {
    let mut fake = Fake::new();
    let mut m = memory();
    let commands = fake.eval(&mut m);
    assert!(
        builds(&commands).contains(&(MOTHER, BuildItem::CoreKit)),
        "{commands:?}"
    );
    let target = m
        .plans
        .iter()
        .find_map(|p| p.expansion.as_ref())
        .map(|e| e.site);
    assert_eq!(target, Some(SiteId(id(12))), "the crystal vein site");
    // The kit's MULE is split off the start kit's fleet meanwhile.
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, Command::CreateFleet { ships } if ships.len() == 1)),
        "{commands:?}"
    );
}

#[test]
fn a_kit_priced_above_the_pool_cap_triggers_storage() {
    let mut fake = Fake::new();
    // A crystal income, so crystal-first does not apply, and a kit above the cap.
    fake.economy.income_last_hour = Stock::units(600, 60);
    fake.economy.core_kit_price = Stock::units(9_000, 750);
    fake.economy.pool = Stock::units(8_000, 900);
    let mut m = memory();
    let commands = fake.eval(&mut m);
    let built = builds(&commands);
    assert!(
        built.iter().any(|(_, item)| matches!(
            item,
            BuildItem::Module {
                kind: ModuleKind::Storage,
                ..
            }
        )),
        "{commands:?}"
    );
    assert!(!built.iter().any(|(_, item)| *item == BuildItem::CoreKit));
}

#[test]
fn a_waiting_kit_above_a_fallen_pool_cap_lets_storage_go_first() {
    let mut fake = Fake::new();
    // A kit queued at 9,000 A, and the cap has since fallen to 8,000 A: it cannot be paid
    // until storage raises the cap, so it must not hold the pool back from that storage.
    fake.economy.income_last_hour = Stock::units(600, 60);
    fake.economy.pool = Stock::units(7_900, 500);
    fake.economy.core_kit_price = Stock::units(9_000, 750);
    fake.holdings[0].queue.push(QueuedItem {
        item: BuildItem::CoreKit,
        price: Stock::units(9_000, 750),
        started: false,
    });
    let mut m = memory();
    m.plans.push(Plan {
        expansion: Some(expand::Expansion {
            site: SiteId(id(20)),
            system: S2,
            holding: MOTHER,
            carrier: None,
            escort: None,
            stage: expand::Stage::Prepare,
        }),
        ..Plan::new(Goal::Expand, fake.now)
    });
    let commands = fake.eval(&mut m);
    assert!(
        builds(&commands).iter().any(|(_, item)| matches!(
            item,
            BuildItem::Module {
                kind: ModuleKind::Storage,
                ..
            }
        )),
        "{commands:?}"
    );
}

/// A loaded MULE fleet docked at the mother station, for the trip tests.
fn loaded(fake: &mut Fake) -> FleetId {
    let carrier = FleetId(id(300));
    let mule = ShipId(id(301));
    fake.fleets.insert(
        carrier,
        OwnFleet {
            id: carrier,
            place: FleetPlace::Docked(MOTHER),
            ships: vec![(mule, ShipClass::Mule)],
            ..idle_fleet()
        },
    );
    fake.cargo.insert(
        mule,
        MuleCargo {
            stock: Stock::ZERO,
            core_kit: true,
        },
    );
    carrier
}

fn with_expansion(m: &mut StrategistMemory, carrier: FleetId, site: u64, system: SystemId) {
    m.plans.push(Plan {
        expansion: Some(expand::Expansion {
            site: SiteId(id(site)),
            system,
            holding: MOTHER,
            carrier: Some(carrier),
            escort: None,
            stage: expand::Stage::Prepare,
        }),
        ..Plan::new(Goal::Expand, Tick::new(HOUR))
    });
}

#[test]
fn a_trip_longer_than_the_session_starts_and_goes_on_offline() {
    let mut fake = Fake::new();
    let carrier = loaded(&mut fake);
    // The trip takes 2 h; the session ends in 30 min. It starts anyway: the simulation keeps
    // it going after the grace and deploys the kit on arrival (D-291).
    fake.route_ms = 2 * 3_600_000;
    fake.session.ends = Some(fake.now.after(Ticks::from_minutes(30)));
    let mut m = memory();
    with_expansion(&mut m, carrier, 20, S2);
    let commands = fake.eval(&mut m);
    let orders: Vec<&FleetOrder> = commands
        .iter()
        .filter_map(|c| match c {
            Command::Fleet { fleet, order, .. } if *fleet == carrier => Some(order),
            _ => None,
        })
        .collect();
    let site = &fake.sites[4];
    assert_eq!(
        orders,
        vec![
            &FleetOrder::Launch,
            &FleetOrder::Jump { system: S2 },
            &FleetOrder::Move {
                to: Location {
                    system: S2,
                    position: site.position
                }
            }
        ],
        "{commands:?}"
    );
    let e = m.plans.iter().find_map(|p| p.expansion.as_ref()).unwrap();
    assert_eq!((e.site, e.stage), (site.id, expand::Stage::Travel));
}

#[test]
fn the_sign_off_leaves_a_claim_trip_under_way_under_direct_control() {
    let mut fake = Fake::new();
    let carrier = loaded(&mut fake);
    let mut m = memory();
    with_expansion(&mut m, carrier, 20, S2);
    fake.route_ms = 2 * 3_600_000;
    fake.eval(&mut m);
    // On its way, under direct control, when the sign-off comes: it stays so, and its orders
    // continue past the grace (D-291).
    fake.direct.insert(carrier);
    if let Some(f) = fake.fleets.get_mut(&carrier) {
        f.place = FleetPlace::Travelling;
        f.busy = true;
    }
    fake.session.ends = Some(fake.now.after(Ticks::new(200)));
    fake.session.next_starts = Some(fake.now.after(Ticks::from_hours(10)));
    let commands = fake.eval(&mut m);
    assert_eq!(m.stats().sign_offs, 1);
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            Command::Doctrine(
                DoctrineCommand::Release { fleet } | DoctrineCommand::SetFleet { fleet, .. }
            ) if *fleet == carrier
        )),
        "{commands:?}"
    );
}

#[test]
fn the_sign_off_releases_every_fleet_with_a_role_and_fills_queues() {
    let mut fake = Fake::new();
    fake.economy.income_last_hour = Stock::units(600, 60);
    fake.economy.pool = Stock::units(1_000, 100);
    fake.direct.insert(START);
    // The session ends within two intervals; the next starts in 10 h.
    fake.session.ends = Some(fake.now.after(Ticks::new(200)));
    fake.session.next_starts = Some(fake.now.after(Ticks::from_hours(10)));
    let mut m = memory();
    let commands = fake.eval(&mut m);
    assert!(
        commands.contains(&Command::Doctrine(DoctrineCommand::Release {
            fleet: START
        }))
    );
    let role = commands.iter().find_map(|c| match c {
        Command::Doctrine(DoctrineCommand::SetFleet {
            fleet,
            doctrine: Some(d),
        }) if *fleet == START => d.role.clone(),
        _ => None,
    });
    assert_eq!(role, Some(FleetRole::Garrison { holding: MOTHER }));
    assert!(commands.iter().any(|c| matches!(
        c,
        Command::Doctrine(DoctrineCommand::SetReplacement { entries }) if entries.iter().any(|e| e.fleet == START && e.class == ShipClass::Vx && e.count == 3)
    )));
    // Queues worth the income until the next session: 1,000 A now and 6,000 A to come.
    let spent = builds(&commands)
        .iter()
        .fold(Stock::ZERO, |sum, (_, item)| {
            sum.plus(match item {
                BuildItem::Module { kind, .. } => fake.numbers().module_cost(*kind),
                BuildItem::Ship { class } => fake.numbers().ship(*class).unwrap().0,
                BuildItem::CoreKit => fake.economy.core_kit_price,
            })
        });
    assert!(spent.alloy > 0, "{commands:?}");
    assert!(spent.alloy <= Stock::units(7_000, 0).alloy, "{spent:?}");
    assert!(commands.len() <= fake.rules.sign_off_commands);
    assert_eq!(m.stats().sign_offs, 1);
}

#[test]
fn hysteresis_keeps_a_plan_for_its_minimum_time() {
    let mut fake = Fake::new();
    // One plan at a time: the crystal-first expansion outranks building out.
    fake.rules.max_plans = 1;
    let mut m = memory();
    m.plans.push(Plan::new(Goal::BuildOut, fake.now));
    fake.advance(150);
    fake.eval(&mut m);
    assert_eq!(
        m.goals(),
        vec![Goal::Expand, Goal::BuildOut],
        "kept for 5 min"
    );
    fake.advance(Ticks::from_minutes(5).get());
    fake.eval(&mut m);
    assert_eq!(m.goals(), vec![Goal::Expand]);
}

#[test]
fn a_stuck_plan_is_abandoned_after_10_min_and_its_fleets_released() {
    let mut fake = Fake::new();
    let carrier = loaded(&mut fake);
    let mut m = memory();
    with_expansion(&mut m, carrier, 20, S2);
    // The trip starts; the MULE then sits in space, busy, in the wrong place.
    fake.eval(&mut m);
    fake.direct.insert(carrier);
    if let Some(f) = fake.fleets.get_mut(&carrier) {
        f.place = FleetPlace::InSpace(Location {
            system: S1,
            position: PositionMetres::new(5_000.0, 0.0),
        });
        f.busy = true;
    }
    for _ in 0..39 {
        fake.advance(150);
        let commands = fake.eval(&mut m);
        assert!(
            !commands.contains(&Command::Doctrine(DoctrineCommand::Release {
                fleet: carrier
            }))
        );
    }
    let mut released = false;
    for _ in 0..3 {
        fake.advance(150);
        let commands = fake.eval(&mut m);
        released |= commands.contains(&Command::Doctrine(DoctrineCommand::Release {
            fleet: carrier,
        }));
    }
    assert!(released);
    assert_eq!(m.stats().plans_abandoned, 1);
}
