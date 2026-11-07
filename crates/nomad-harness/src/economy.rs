//! `--economy`: scripted builder commanders over simulated days, reporting the D-070 pacing
//! measures (S4 plan task 11, §8).
//!
//! Each of the four stage-1 commanders runs the same script once a minute, through ordinary
//! commands and the single validator (D-077): it splits a MULE off for core kits and one for
//! convoys, expands to free sites in its home system and the systems next to it (crystal
//! veins first, then the richest), builds every holding out (deposit modules, solar,
//! refinery, storage, 4 pylons, 2 turrets, a dry dock: the plan's standard build-out), keeps a
//! convoy running between remote holdings and the mother station, and keeps a core-kit
//! reserve so expansion is not starved. It is a measuring script, not an AI (A2 is).
//!
//! A core kit's price grows with the commander's size and is fixed when the kit is queued
//! (D-189), so the reserve is the next kit's price, and the report lists every kit with its
//! price. It flags a kit priced above the commander's pool cap when it was queued: until
//! storage raises the cap, the pool cannot hold its price, so it cannot be bought (doc 14
//! §13).
//!
//! Remote producers share D-217's diminishing yield per resource (D-281): alloy output is
//! scaled by min(1, F ÷ alloy-producing holdings other than the mother station), crystal by
//! the same over crystal-producing ones; each commander's line gives both factors and counts.
//!
//! The D-070 measures follow D-218 §5.4 as amended by D-283 ([`crate::pacing`]): storage fill
//! and the cap-fleet rebuild over days 21–35, late income over days 42–56, hours per new
//! holding as 72 h ÷ the holdings completed by hour 72, the rebuild in the Garrison stand-in's
//! composition, and build-out measured from each holding's core completion. A run shorter
//! than a window measures a stand-in stretch and says so. `--csv PATH` writes the hourly
//! samples, one row per commander and hour.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::Instant;

use nomad_sim::ShipClass;

use crate::pacing;
use nomad_sim::galaxy::DepositKind;
use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_with_rules};
use nomad_sim::snapshot::state_hash;
use nomad_sim::{
    BuildItem, Command, CommanderId, CommanderInput, CommanderInputKind, EconomyCommand,
    EconomyEvent, FleetId, FleetOrder, HoldingId, Location, ModuleKind, Owner, Place, Queueing,
    Rules, SeasonConfig, ShipId, SimId, SiteId, Slot, Stock, TICKS_PER_SECOND, TickInputs, Ticks,
    UniverseState, step, validate_command,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];

/// What `--economy` runs.
pub struct EconomyRun {
    /// The season seed.
    pub seed: u64,
    /// Simulated days.
    pub days: u64,
    /// The rules (D-104).
    pub rules: Rules,
    /// Where to write the hourly samples as CSV, if anywhere.
    pub csv: Option<std::path::PathBuf>,
}

/// One commander's script state.
#[derive(Default)]
struct Builder {
    kit_mule: Option<ShipId>,
    haul_mule: Option<ShipId>,
    target: Option<SiteId>,
    first_kit_start: Option<u64>,
    completions: Vec<u64>,
    built_out: BTreeMap<HoldingId, u64>,
    completed_at: BTreeMap<HoldingId, u64>,
    kits: Vec<KitRecord>,
}

/// One core kit the script queued (D-189).
struct KitRecord {
    item: SimId,
    queued: u64,
    /// The n it was priced at.
    n: u64,
    price: Stock,
    /// The pool cap when it was queued.
    cap: Stock,
    /// When it was paid for and started.
    started: Option<u64>,
}

impl KitRecord {
    /// Whether the pool could not hold its price when it was queued.
    fn above_cap(&self) -> bool {
        !self.cap.covers(self.price)
    }
}

/// The modules of a full build-out, in build order (S4 plan §4.2).
fn build_out(state: &UniverseState, holding: HoldingId) -> Vec<(ModuleKind, Slot)> {
    let Some(h) = state.holding(holding) else {
        return Vec::new();
    };
    let deposits = state
        .galaxy()
        .site(h.site)
        .map(|s| s.deposits.clone())
        .unwrap_or_default();
    let mut list = Vec::new();
    for (i, d) in deposits.iter().enumerate().take(2) {
        let kind = match d.kind {
            DepositKind::OreField => ModuleKind::Harvester,
            DepositKind::CrystalVein => ModuleKind::Extractor,
        };
        list.push((kind, Slot::Deposit(u8::try_from(i).unwrap_or(0))));
    }
    list.push((ModuleKind::Solar, Slot::Solar));
    if deposits.iter().any(|d| d.kind == DepositKind::OreField) {
        list.push((ModuleKind::Refinery, Slot::Inner(0)));
    }
    list.push((ModuleKind::Storage, Slot::Inner(1)));
    for i in 0..4 {
        list.push((ModuleKind::Pylon, Slot::Pylon(i)));
    }
    list.push((ModuleKind::Turret, Slot::Inner(2)));
    list.push((ModuleKind::Turret, Slot::Inner(3)));
    list.push((ModuleKind::DryDock, Slot::Dock));
    list
}

fn mother(state: &UniverseState, c: CommanderId) -> Option<HoldingId> {
    state
        .holdings()
        .find(|(_, h)| h.owner == c && h.mother_station)
        .map(|(id, _)| id)
}

fn fleet_of(state: &UniverseState, ship: Option<ShipId>) -> Option<FleetId> {
    state.ship(ship?).map(|s| s.fleet)
}

/// Free sites the commander may claim: home system first, then its neighbours; crystal
/// veins first, then by deposit count.
fn next_target(state: &UniverseState, c: CommanderId, home: nomad_sim::SystemId) -> Option<SiteId> {
    let jumps = state.galaxy().jumps_from(home);
    let mut best: Option<((u32, u8, usize, SiteId), SiteId)> = None;
    for (id, site) in state.galaxy().sites() {
        let Some(&j) = jumps.get(&site.system) else {
            continue;
        };
        if j > 1 || state.holding_at(id).is_some() {
            continue;
        }
        let claimable = state
            .galaxy()
            .system(site.system)
            .is_some_and(|s| nomad_sim::galaxy::is_claimable(s.kind));
        if !claimable || site.deposits.is_empty() {
            continue;
        }
        let crystal = u8::from(
            !site
                .deposits
                .iter()
                .any(|d| d.kind == DepositKind::CrystalVein),
        );
        let key = (j, crystal, usize::MAX - site.deposits.len(), id);
        if best.as_ref().is_none_or(|(k, _)| key < *k) {
            best = Some((key, id));
        }
    }
    let _ = c;
    best.map(|(_, id)| id)
}

/// One minute of commander `c`'s script.
#[expect(
    clippy::too_many_lines,
    reason = "one readable script; split it when A2 replaces it"
)]
fn think(state: &UniverseState, c: CommanderId, b: &mut Builder, tick: u64) -> Vec<Command> {
    let owner = Owner::Commander(c);
    let mut commands = Vec::new();
    let Some(mother) = mother(state, c) else {
        return commands;
    };
    let Some(home) = state
        .holding(mother)
        .and_then(|h| state.galaxy().site(h.site))
        .map(|s| s.system)
    else {
        return commands;
    };
    let economy = state.economy_of(c);

    // Split the start kit's MULEs off once.
    if b.kit_mule.is_none() {
        let start = state.fleet_ids().into_iter().find_map(|f| {
            let v = state.fleet(f)?;
            (v.owner == owner).then_some(v)
        });
        if let Some(fleet) = start {
            let mules: Vec<ShipId> = fleet
                .ships
                .iter()
                .copied()
                .filter(|s| state.ship(*s).is_some_and(|v| v.class == ShipClass::Mule))
                .collect();
            if mules.len() >= 2 {
                b.kit_mule = Some(mules[0]);
                b.haul_mule = Some(mules[1]);
                commands.push(Command::CreateFleet {
                    ships: vec![mules[0]],
                });
                commands.push(Command::CreateFleet {
                    ships: vec![mules[1]],
                });
            }
        }
        return commands;
    }

    // Expansion: a core kit, carried by the kit MULE.
    let mother_view = state.holding_economy(mother);
    let kits = mother_view.as_ref().map_or(0, |v| v.kits);
    let kit_queued = mother_view
        .as_ref()
        .is_some_and(|v| v.queue.iter().any(|q| q.item == BuildItem::CoreKit));
    if b.first_kit_start.is_none()
        && mother_view.as_ref().is_some_and(|v| {
            v.queue
                .iter()
                .any(|q| q.item == BuildItem::CoreKit && q.building.is_some())
        })
    {
        b.first_kit_start = Some(tick);
    }
    let carrying = b.kit_mule.is_some_and(|m| state.cargo(m).core_kit);
    if b.target.is_none_or(|t| state.holding_at(t).is_some()) {
        b.target = next_target(state, c, home);
    }
    let want_kit = b.target.is_some() && !carrying && kits == 0 && !kit_queued;
    if want_kit {
        commands.push(Command::Economy(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::CoreKit,
        }));
    }
    if let Some(kit_fleet) = fleet_of(state, b.kit_mule)
        && let Some(view) = state.fleet(kit_fleet)
    {
        let idle = view.order.is_none() && view.queued.is_empty();
        if !carrying && kits > 0 {
            match view.place {
                Place::Docked(at) if at == mother => {
                    commands.push(Command::Economy(EconomyCommand::LoadCoreKit {
                        fleet: kit_fleet,
                    }));
                }
                _ if idle => commands.push(go_home(view.place, kit_fleet, mother, home)),
                _ => {}
            }
        } else if carrying && let Some(site) = b.target {
            let s = state.galaxy().site(site);
            match (view.place, s) {
                (Place::Docked(_), _) => commands.push(fleet_order(kit_fleet, FleetOrder::Launch)),
                (Place::InSystem(here), Some(s)) if here != s.system && idle => {
                    commands.push(fleet_order(
                        kit_fleet,
                        FleetOrder::Jump { system: s.system },
                    ));
                }
                (Place::InSystem(_), Some(s)) if idle => {
                    let deploy = Command::Economy(EconomyCommand::DeployCoreKit {
                        fleet: kit_fleet,
                        site,
                    });
                    if validate_command(state, owner, &deploy).is_ok() {
                        commands.push(deploy);
                        b.target = None;
                    } else {
                        commands.push(fleet_order(
                            kit_fleet,
                            FleetOrder::Move {
                                to: Location {
                                    system: s.system,
                                    position: s.position,
                                },
                            },
                        ));
                    }
                }
                _ => {}
            }
        } else if !carrying && kits == 0 && idle && !matches!(view.place, Place::Docked(_)) {
            commands.push(go_home(view.place, kit_fleet, mother, home));
        }
    }

    // Convoys: remote holdings outside a depot's system, ending at the mother station.
    let remote: Vec<HoldingId> = economy
        .holdings
        .iter()
        .filter(|h| {
            !h.delivers_to_pool
                && h.slots.iter().any(|(_, m)| {
                    m.is_some_and(|m| m.kind == ModuleKind::Core && m.building_ms.is_none())
                })
        })
        .map(|h| h.holding)
        .take(7)
        .collect();
    if !remote.is_empty()
        && let Some(haul) = fleet_of(state, b.haul_mule)
    {
        let mut route = remote;
        route.push(mother);
        let convoy = Command::Economy(EconomyCommand::Convoy { fleet: haul, route });
        let running = state
            .fleet(haul)
            .is_some_and(|v| v.order.is_some() || !v.queued.is_empty());
        if !running && validate_command(state, owner, &convoy).is_ok() {
            commands.push(convoy);
        }
    }

    // Build-out: one item at a time per holding, keeping a core-kit reserve: the price of
    // the kit in the queue, or else of the next one (D-189).
    let kit_cost = mother_view
        .as_ref()
        .and_then(|v| v.queue.iter().find(|q| q.item == BuildItem::CoreKit))
        .map_or(economy.core_kit_price, |q| q.price);
    let reserve = if b.target.is_some() && !carrying && kits == 0 {
        kit_cost
    } else {
        Stock::ZERO
    };
    let mut budget = economy.pool;
    for h in &economy.holdings {
        let complete = h
            .slots
            .iter()
            .any(|(s, m)| *s == Slot::Core && m.is_some_and(|m| m.building_ms.is_none()));
        if !complete {
            continue;
        }
        b.completed_at.entry(h.holding).or_insert(tick);
        let list = build_out(state, h.holding);
        let missing: Vec<&(ModuleKind, Slot)> = list
            .iter()
            .filter(|(kind, slot)| {
                !h.slots
                    .iter()
                    .any(|(s, m)| s == slot && m.is_some_and(|m| m.kind == *kind))
            })
            .collect();
        let finished = missing.is_empty()
            && h.slots
                .iter()
                .all(|(_, m)| m.is_none_or(|m| m.building_ms.is_none()));
        if finished {
            b.built_out.entry(h.holding).or_insert(tick);
        }
        if !h.queue.is_empty() {
            continue;
        }
        let Some(&&(kind, slot)) = missing.first() else {
            continue;
        };
        let price = module_cost(state, kind);
        // Modules that grow income ignore the kit reserve; the rest keep it, per resource,
        // so a missing crystal reserve never blocks the extractor that would earn it.
        let grows = matches!(
            kind,
            ModuleKind::Harvester
                | ModuleKind::Extractor
                | ModuleKind::Solar
                | ModuleKind::Refinery
                | ModuleKind::Storage
        );
        let keep = if grows {
            Stock::ZERO
        } else {
            Stock {
                alloy: reserve.alloy,
                crystal: if price.crystal > 0 {
                    reserve.crystal
                } else {
                    0
                },
            }
        };
        if !budget.covers(price.plus(keep)) {
            continue;
        }
        let build = Command::Economy(EconomyCommand::Build {
            holding: h.holding,
            item: BuildItem::Module { kind, slot },
        });
        if validate_command(state, owner, &build).is_ok() {
            budget = budget.minus(price);
            commands.push(build);
        }
    }
    commands
}

fn module_cost(state: &UniverseState, kind: ModuleKind) -> Stock {
    let c = state.rules().economy.module(kind).cost;
    Stock::units(c.alloy, c.crystal)
}

/// Docks at the mother station, jumping home first from another system (docking needs the
/// fleet in the holding's system).
fn go_home(place: Place, fleet: FleetId, mother: HoldingId, home: nomad_sim::SystemId) -> Command {
    match place {
        Place::InSystem(here) if here != home => {
            fleet_order(fleet, FleetOrder::Jump { system: home })
        }
        _ => fleet_order(fleet, FleetOrder::Dock { at: mother }),
    }
}

fn fleet_order(fleet: FleetId, order: FleetOrder) -> Command {
    Command::Fleet {
        fleet,
        order,
        queue: Queueing::Replace,
        retreat: false,
    }
}

/// Records the core kits the script queues and starts, with their price (D-189).
fn record_kit(
    state: &UniverseState,
    builders: &mut BTreeMap<CommanderId, Builder>,
    event: &EconomyEvent,
    t: u64,
) {
    match *event {
        EconomyEvent::ItemQueued { holding, item } => {
            let Some(owner) = state.holding(holding).map(|h| h.owner) else {
                return;
            };
            let Some(price) = state
                .holding_economy(holding)
                .and_then(|v| {
                    v.queue
                        .into_iter()
                        .find(|q| q.id == item && q.item == BuildItem::CoreKit)
                })
                .map(|q| q.price)
            else {
                return;
            };
            if let Some(b) = builders.get_mut(&owner) {
                b.kits.push(KitRecord {
                    item,
                    queued: t,
                    // The kit now counts itself. Nothing else in a harness tick changes n:
                    // the script queues one kit at a time and never cancels, and no MULE is
                    // destroyed.
                    n: state.expansion_count(owner).saturating_sub(1),
                    price,
                    cap: state.pool_cap(owner),
                    started: None,
                });
            }
        }
        EconomyEvent::ItemStarted { item, .. } => {
            for b in builders.values_mut() {
                if let Some(kit) = b.kits.iter_mut().find(|k| k.item == item) {
                    kit.started = Some(t);
                }
            }
        }
        _ => {}
    }
}

/// Lists every core kit with its price, flagging those above the pool cap when they were
/// queued (D-189; doc 14 §13).
fn write_kits(
    out: &mut impl Write,
    builders: &BTreeMap<CommanderId, Builder>,
) -> Result<(), String> {
    writeln!(
        out,
        "core kits (D-189: price fixed when queued; pool cap when queued):"
    )
    .map_err(|e| e.to_string())?;
    let (mut queued, mut bought, mut above, mut above_unbought) = (0, 0, 0, 0);
    for (c, b) in builders {
        for (i, kit) in b.kits.iter().enumerate() {
            queued += 1;
            bought += usize::from(kit.started.is_some());
            if kit.above_cap() {
                above += 1;
                above_unbought += usize::from(kit.started.is_none());
            }
            writeln!(
                out,
                "  C{} kit {}: queued {:.2} h | n {} | {:.0} A + {:.0} C | cap {:.0} A / {:.0} C{} | {}",
                c.0,
                i + 1,
                hours(kit.queued),
                kit.n,
                units(kit.price.alloy),
                units(kit.price.crystal),
                units(kit.cap.alloy),
                units(kit.cap.crystal),
                if kit.above_cap() {
                    " | ABOVE THE POOL CAP"
                } else {
                    ""
                },
                kit.started
                    .map_or("never bought".to_owned(), |s| format!("bought {:.2} h", hours(s))),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    writeln!(
        out,
        "  {queued} kits queued, {bought} bought | {above} above the pool cap when queued, {above_unbought} of them never bought"
    )
    .map_err(|e| e.to_string())
}

fn hours(ticks: u64) -> f64 {
    ticks as f64 / (3_600 * TICKS_PER_SECOND) as f64
}

fn units(milli: u64) -> f64 {
    milli as f64 / 1_000.0
}

/// The mother station's income at the season start, the reference for late income (D-070).
const START_INCOME: f64 = 600.0;

/// Each commander's D-070 measures (D-218 §5.4, D-283), collected for the report.
#[derive(Default)]
struct Measures {
    storage_start: Vec<f64>,
    storage_mid: Vec<f64>,
    storage_mid_crystal: Vec<f64>,
    first_kit: Vec<f64>,
    per_holding: Vec<f64>,
    rebuild: Vec<f64>,
    rebuild_alloy: Vec<f64>,
    rebuild_crystal: Vec<f64>,
    mid_income: Vec<(f64, f64)>,
    build_out: Vec<f64>,
    remote_holdings: usize,
    late: Vec<f64>,
}

impl Measures {
    /// Writes each measure as the median over the commanders, with the range and a verdict
    /// against D-070's target (α passes within ±50%, doc 30 §3).
    fn write(
        &self,
        out: &mut impl Write,
        mid: pacing::Window,
        late: pacing::Window,
    ) -> Result<(), String> {
        let verdict = |value: f64, lo: f64, hi: f64| {
            if value.is_nan() {
                "not measured".to_owned()
            } else if value >= lo && value <= hi {
                "on target".to_owned()
            } else if value >= lo * 0.5 && value <= hi * 1.5 {
                "within ±50% (tuning item)".to_owned()
            } else {
                "outside ±50% (tuning item)".to_owned()
            }
        };
        let range = |v: &[f64]| {
            let finite: Vec<f64> = v.iter().copied().filter(|x| !x.is_nan()).collect();
            if finite.is_empty() {
                return String::new();
            }
            let lo = finite.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            format!(" (range {lo:.2}–{hi:.2})")
        };
        writeln!(
            out,
            "D-070 measures, median over the commanders (D-218 §5.4; mid-season {}; late {}):",
            mid.label(),
            late.label()
        )
        .map_err(|e| e.to_string())?;
        let lines: [(&str, &[f64], f64, f64, &str); 7] = [
            (
                "storage fills at start income [8-12 h]",
                &self.storage_start,
                8.0,
                12.0,
                "h",
            ),
            (
                "storage fills at mid-season income, alloy [8-12 h]",
                &self.storage_mid,
                8.0,
                12.0,
                "h",
            ),
            (
                "first expansion started [<= 1 h]",
                &self.first_kit,
                0.0,
                1.0,
                "h",
            ),
            (
                "hours per new holding, days 0-3: 72 h / holdings completed [12-24 h]",
                &self.per_holding,
                12.0,
                24.0,
                "h",
            ),
            (
                "full cap-fleet rebuild, Garrison mix, binding resource [~24 h]",
                &self.rebuild,
                20.0,
                28.0,
                "h",
            ),
            (
                "build-out from core completion, D-283 [48-72 h]",
                &self.build_out,
                48.0,
                72.0,
                "h",
            ),
            ("late income vs start [3-4x]", &self.late, 3.0, 4.0, "x"),
        ];
        for (name, values, lo, hi, unit) in lines {
            let value = pacing::median(values);
            writeln!(
                out,
                "  {name}: {value:.2} {unit}{} -> {}",
                range(values),
                verdict(value, lo, hi)
            )
            .map_err(|e| e.to_string())?;
        }
        let alloy: Vec<f64> = self.mid_income.iter().map(|x| x.0).collect();
        let crystal: Vec<f64> = self.mid_income.iter().map(|x| x.1).collect();
        writeln!(
            out,
            "  also: storage fill, crystal {:.1} h | rebuild on alloy {:.1} h, on crystal {:.1} h | mid-season income {:.0} A/h {:.0} C/h | build-out finished at {} of {} remote holdings",
            pacing::median(&self.storage_mid_crystal),
            pacing::median(&self.rebuild_alloy),
            pacing::median(&self.rebuild_crystal),
            pacing::median(&alloy),
            pacing::median(&crystal),
            self.build_out.len(),
            self.remote_holdings,
        )
        .map_err(|e| e.to_string())
    }
}

/// Runs `--economy` and prints the D-070 measures.
#[expect(
    clippy::too_many_lines,
    reason = "the run loop and its report read best in one place"
)]
pub fn run(out: &mut impl Write, cfg: EconomyRun) -> Result<(), String> {
    let mut state =
        generate_stage1_with_rules(SeasonConfig::new(cfg.seed, 0), COMMANDERS, cfg.rules)
            .map_err(|e| format!("{e:?}"))?;
    let start_cap = state.pool_cap(COMMANDERS[0]);
    let ticks = Ticks::from_days(cfg.days).get();
    let minute = 60 * TICKS_PER_SECOND;
    let mut builders: BTreeMap<CommanderId, Builder> = COMMANDERS
        .iter()
        .map(|&c| (c, Builder::default()))
        .collect();
    let mut seq: u64 = 0;
    let mut rejected: BTreeMap<String, u64> = BTreeMap::new();
    let mut times = Vec::with_capacity(usize::try_from(ticks).unwrap_or(0));
    let mut kinds: BTreeSet<String> = BTreeSet::new();
    let started = Instant::now();
    let hour = 3_600 * TICKS_PER_SECOND;
    let mut samples: BTreeMap<CommanderId, Vec<pacing::Sample>> = BTreeMap::new();
    let mut csv = match &cfg.csv {
        Some(path) => {
            let mut file =
                std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
            writeln!(
                file,
                "hour,commander,produced_alloy,produced_crystal,cap_alloy,cap_crystal,pool_alloy,pool_crystal,lost_alloy,lost_crystal,holdings,alloy_producers,crystal_producers,alloy_yield_permille,crystal_yield_permille,command_cap,kit_alloy,kit_crystal"
            )
            .map_err(|e| e.to_string())?;
            Some(file)
        }
        None => None,
    };
    for t in 0..ticks {
        let mut commanders = Vec::new();
        for (i, &c) in COMMANDERS.iter().enumerate() {
            let offset = u64::try_from(i).unwrap_or(0) * 10 * TICKS_PER_SECOND;
            if t % minute != offset % minute {
                continue;
            }
            let b = builders.get_mut(&c).ok_or("builder")?;
            for command in think(&state, c, b, t) {
                seq += 1;
                commanders.push(CommanderInput {
                    commander: c,
                    client_seq: seq,
                    kind: CommanderInputKind::Command(command),
                });
            }
        }
        let tick_started = Instant::now();
        let outputs = step(
            &mut state,
            &TickInputs {
                operator: vec![],
                commanders,
            },
        );
        times.push(tick_started.elapsed().as_micros());
        for result in &outputs.commands {
            if let Err(reason) = result.outcome {
                *rejected.entry(format!("{reason:?}")).or_default() += 1;
            }
        }
        if (t + 1) % hour == 0 {
            for &c in &COMMANDERS {
                let e = state.economy_of(c);
                let complete = e
                    .holdings
                    .iter()
                    .filter(|h| state.holding(h.holding).is_some_and(|x| x.core_complete))
                    .count();
                let sample = pacing::Sample {
                    hour: (t + 1) / hour,
                    produced: e.produced,
                    cap: e.cap,
                    command_cap: e.command_cap,
                    holdings: u32::try_from(complete).unwrap_or(u32::MAX),
                };
                if let Some(file) = csv.as_mut() {
                    writeln!(
                        file,
                        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                        sample.hour,
                        c.0,
                        e.produced.alloy,
                        e.produced.crystal,
                        e.cap.alloy,
                        e.cap.crystal,
                        e.pool.alloy,
                        e.pool.crystal,
                        e.lost.alloy,
                        e.lost.crystal,
                        complete,
                        e.producing_holdings.alloy,
                        e.producing_holdings.crystal,
                        e.yield_permille.alloy,
                        e.yield_permille.crystal,
                        e.command_cap,
                        e.core_kit_price.alloy,
                        e.core_kit_price.crystal,
                    )
                    .map_err(|e| e.to_string())?;
                }
                samples.entry(c).or_default().push(sample);
            }
        }
        for event in &outputs.economy_events {
            if let EconomyEvent::HoldingCompleted { holding } = event
                && let Some(owner) = state.holding(*holding).map(|h| h.owner)
                && let Some(b) = builders.get_mut(&owner)
            {
                b.completions.push(t);
            }
            record_kit(&state, &mut builders, event, t);
            let name = format!("{event:?}");
            kinds.insert(name.split([' ', '{']).next().unwrap_or_default().to_owned());
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    times.sort_unstable();
    let mean = times.iter().sum::<u128>() as f64 / times.len().max(1) as f64;
    let p99 = times.get(times.len() * 99 / 100).copied().unwrap_or(0);
    let max = times.last().copied().unwrap_or(0);

    let run_h = ticks / (3_600 * TICKS_PER_SECOND);
    let rules = state.rules().economy.clone();
    let per_command = pacing::garrison_per_command(&rules);
    let mid = pacing::Window::days(21, 35, run_h);
    let late = pacing::Window::days(42, 56, run_h);

    writeln!(
        out,
        "economy | seed {} | 4 builder commanders | {} days ({ticks} ticks) | script every 60 s",
        cfg.seed, cfg.days
    )
    .map_err(|e| e.to_string())?;
    let mut m = Measures::default();
    for &c in &COMMANDERS {
        let b = builders.get(&c).ok_or("builder")?;
        let e = state.economy_of(c);
        let series = samples.get(&c).map_or(&[][..], Vec::as_slice);
        let holdings = e
            .holdings
            .iter()
            .filter(|h| state.holding(h.holding).is_some_and(|x| x.core_complete))
            .count();
        let completions: Vec<f64> = b.completions.iter().map(|&t| hours(t)).collect();
        if let Some((a, _)) = pacing::income(
            series,
            pacing::Window {
                from: 0,
                to: 1,
                proxy: false,
            },
        ) {
            m.storage_start.push(units(start_cap.alloy) / a.max(1e-9));
        }
        if let (Some((a, cr)), Some((cap_a, cap_c, command))) =
            (pacing::income(series, mid), pacing::caps(series, mid))
        {
            m.storage_mid.push(cap_a / a.max(1e-9));
            m.storage_mid_crystal.push(cap_c / cr.max(1e-9));
            let (ha, hc) = pacing::rebuild_hours(per_command, command, (a, cr));
            m.rebuild.push(ha.max(hc));
            m.rebuild_alloy.push(ha);
            m.rebuild_crystal.push(hc);
            m.mid_income.push((a, cr));
        }
        if let Some((a, _)) = pacing::income(series, late) {
            m.late.push(a / START_INCOME);
        }
        if let Some(t) = b.first_kit_start {
            m.first_kit.push(hours(t));
        }
        if let Some(h) = pacing::hours_per_holding_days_0_3(&completions) {
            m.per_holding.push(h);
        }
        // D-283: from each remote holding's core completion to its full build-out.
        let mut remote = 0;
        for (holding, &done) in &b.completed_at {
            if state.holding(*holding).is_none_or(|x| x.mother_station) {
                continue;
            }
            remote += 1;
            if let Some(&built) = b.built_out.get(holding) {
                m.build_out.push(hours(built - done));
            }
        }
        m.remote_holdings += remote;
        let by_day: Vec<String> = [3, 7, 14, 21, 28, 35, 42, 49, 56]
            .iter()
            .filter_map(|&d| {
                series
                    .iter()
                    .find(|s| s.hour == d * 24)
                    .map(|s| format!("d{d} {}", s.holdings))
            })
            .collect();
        writeln!(
            out,
            "  C{}: {holdings} holdings (completions at {} h; {}) | income {:.0} A/h {:.0} C/h | D-281 yield {}‰ over {} alloy, {}‰ over {} crystal producers | pool {:.0} A {:.0} C of {:.0}/{:.0} | lost {:.0} A {:.0} C | command {}/{} | first kit {}",
            c.0,
            completions.iter().map(|h| format!("{h:.1}")).collect::<Vec<_>>().join(", "),
            by_day.join(", "),
            units(e.income_last_hour.alloy),
            units(e.income_last_hour.crystal),
            e.yield_permille.alloy,
            e.producing_holdings.alloy,
            e.yield_permille.crystal,
            e.producing_holdings.crystal,
            units(e.pool.alloy),
            units(e.pool.crystal),
            units(e.cap.alloy),
            units(e.cap.crystal),
            units(e.lost.alloy),
            units(e.lost.crystal),
            e.command_used,
            e.command_cap,
            b.first_kit_start.map_or("never".to_owned(), |t| format!("{:.2} h", hours(t))),
        )
        .map_err(|e| e.to_string())?;
    }
    m.write(out, mid, late)?;
    write_kits(out, &builders)?;
    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    writeln!(
        out,
        "commands rejected {rejected:?} | event kinds {} | problems {} | tick time: mean {mean:.0} µs, p99 {p99} µs, max {max} µs | elapsed {elapsed:.1} s | hash {hash:016x}",
        kinds.len(),
        state.economy_problems().len() + state.fleet_problems().len(),
    )
    .map_err(|e| e.to_string())?;
    if let Some(mut file) = csv {
        file.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}
