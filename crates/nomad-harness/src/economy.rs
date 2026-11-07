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
//! Remote producers share D-217's diminishing yield, min(1, F ÷ producing holdings other
//! than the mother station); each commander's line gives the factor and that count.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::Instant;

use nomad_sim::ShipClass;
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

/// Runs `--economy` and prints the D-070 measures.
#[expect(
    clippy::too_many_lines,
    reason = "the run loop and its report read best in one place"
)]
pub fn run(out: &mut impl Write, cfg: EconomyRun) -> Result<(), String> {
    let mut state = generate_stage1_with_rules(
        SeasonConfig {
            seed: cfg.seed,
            season_start_unix_ms: 0,
        },
        COMMANDERS,
        cfg.rules,
    )
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

    let start_income = 600.0;
    let rules = state.rules().economy.clone();
    let cost = |class: ShipClass| {
        rules.ship(class).map_or((0.0, 0.0, 0.0), |s| {
            (
                s.cost.alloy as f64,
                s.cost.crystal as f64,
                f64::from(s.command),
            )
        })
    };
    // The plan's cap-fleet mix (§5): 3 WARDEN, 5 LANCER, 5 VX.
    let mix = [
        (ShipClass::Warden, 3.0),
        (ShipClass::Lancer, 5.0),
        (ShipClass::Vx, 5.0),
    ];
    let (mix_a, mix_c, mix_cmd) = mix.iter().fold((0.0, 0.0, 0.0), |acc, &(class, n)| {
        let (a, c, cmd) = cost(class);
        (acc.0 + a * n, acc.1 + c * n, acc.2 + cmd * n)
    });
    let module_total = |list: &[ModuleKind]| {
        list.iter().fold((0.0, 0.0), |acc, &k| {
            let c = rules.module(k).cost;
            (acc.0 + c.alloy as f64, acc.1 + c.crystal as f64)
        })
    };
    let standard = [
        ModuleKind::Pylon,
        ModuleKind::Pylon,
        ModuleKind::Pylon,
        ModuleKind::Pylon,
        ModuleKind::Solar,
        ModuleKind::Harvester,
        ModuleKind::Refinery,
        ModuleKind::Storage,
        ModuleKind::Turret,
        ModuleKind::Turret,
        ModuleKind::DryDock,
    ];
    let (bo_a, bo_c) = module_total(&standard);
    let bo_a = bo_a + rules.core_kit.alloy as f64;
    let bo_c = bo_c + rules.core_kit.crystal as f64;

    writeln!(
        out,
        "economy | seed {} | 4 builder commanders | {} days ({ticks} ticks) | script every 60 s",
        cfg.seed, cfg.days
    )
    .map_err(|e| e.to_string())?;
    let mut ratios = Vec::new();
    let mut first_kits = Vec::new();
    let mut intervals = Vec::new();
    let mut rebuilds = Vec::new();
    let mut storage_end = Vec::new();
    let mut buildouts = Vec::new();
    for &c in &COMMANDERS {
        let b = builders.get(&c).ok_or("builder")?;
        let e = state.economy_of(c);
        let income = e.income_last_hour;
        let holdings = e
            .holdings
            .iter()
            .filter(|h| state.holding(h.holding).is_some_and(|x| x.core_complete))
            .count();
        let ratio = units(income.alloy) / start_income;
        ratios.push(ratio);
        if let Some(t) = b.first_kit_start {
            first_kits.push(hours(t));
        }
        let early: Vec<u64> = b
            .completions
            .iter()
            .copied()
            .filter(|&t| t <= Ticks::from_days(3).get())
            .collect();
        if let Some(&last) = early.last() {
            intervals.push(hours(last) / early.len() as f64);
        }
        let cap = f64::from(e.command_cap);
        let need_a = mix_a * cap / mix_cmd;
        let need_c = mix_c * cap / mix_cmd;
        let rebuild =
            (need_a / units(income.alloy).max(1e-9)).max(need_c / units(income.crystal).max(1e-9));
        rebuilds.push(rebuild);
        storage_end.push(units(e.cap.alloy) / units(income.alloy).max(1e-9));
        let measured_buildout = b
            .built_out
            .iter()
            .filter(|(h, _)| state.holding(**h).is_some_and(|x| !x.mother_station))
            .find_map(|(h, &done)| Some(hours(done - b.completed_at.get(h)?)));
        if let Some(m) = measured_buildout {
            buildouts.push(m);
        }
        writeln!(
            out,
            "  C{}: {holdings} holdings (completions at {} h) | income {:.0} A/h {:.0} C/h | D-217 yield {}‰ over {} producing | pool {:.0} A {:.0} C of {:.0}/{:.0} | lost {:.0} A {:.0} C | command {}/{} | first kit {} | first remote build-out {}",
            c.0,
            b.completions.iter().map(|&t| format!("{:.1}", hours(t))).collect::<Vec<_>>().join(", "),
            units(income.alloy),
            units(income.crystal),
            e.yield_permille,
            e.producing_holdings,
            units(e.pool.alloy),
            units(e.pool.crystal),
            units(e.cap.alloy),
            units(e.cap.crystal),
            units(e.lost.alloy),
            units(e.lost.crystal),
            e.command_used,
            e.command_cap,
            b.first_kit_start.map_or("never".to_owned(), |t| format!("{:.2} h", hours(t))),
            measured_buildout.map_or("not reached".to_owned(), |h| format!("{h:.1} h")),
        )
        .map_err(|e| e.to_string())?;
    }
    let avg = |v: &[f64]| {
        if v.is_empty() {
            f64::NAN
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    let verdict = |value: f64, lo: f64, hi: f64| {
        if value.is_nan() {
            "not measured".to_owned()
        } else if value >= lo * 0.5 && value <= hi * 1.5 {
            if value >= lo && value <= hi {
                "on target".to_owned()
            } else {
                "within ±50% (tuning item)".to_owned()
            }
        } else {
            "outside ±50% (tuning item)".to_owned()
        }
    };
    let storage_start = units(start_cap.alloy) / start_income;
    writeln!(out, "D-070 measures (targets in brackets):").map_err(|e| e.to_string())?;
    let lines = [
        (
            "storage fills at start income [8-12 h]",
            storage_start,
            8.0,
            12.0,
            "h",
        ),
        (
            "storage fills at end-of-run income [8-12 h]",
            avg(&storage_end),
            8.0,
            12.0,
            "h",
        ),
        (
            "first expansion started [<= 1 h]",
            avg(&first_kits),
            0.0,
            1.0,
            "h",
        ),
        (
            "hours per new holding, days 0-3 [12-24 h]",
            avg(&intervals),
            12.0,
            24.0,
            "h",
        ),
        (
            "cap fleet rebuild at end-of-run income [~24 h]",
            avg(&rebuilds),
            20.0,
            28.0,
            "h",
        ),
        (
            "standard build-out alloy at the holding's own 600 A/h, D-136 [48-72 h]",
            bo_a / 600.0,
            48.0,
            72.0,
            "h",
        ),
        (
            "first remote build-out, measured from core completion [48-72 h]",
            avg(&buildouts),
            48.0,
            72.0,
            "h",
        ),
        (
            "income at the end vs start [3-4x, late season]",
            avg(&ratios),
            3.0,
            4.0,
            "x",
        ),
    ];
    for (name, value, lo, hi, unit) in lines {
        writeln!(
            out,
            "  {name}: {value:.2} {unit} -> {}",
            verdict(value, lo, hi)
        )
        .map_err(|e| e.to_string())?;
    }
    write_kits(out, &builders)?;
    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    writeln!(
        out,
        "commands rejected {rejected:?} | event kinds {} | problems {} | tick time: mean {mean:.0} µs, p99 {p99} µs, max {max} µs | elapsed {elapsed:.1} s | hash {hash:016x}",
        kinds.len(),
        state.economy_problems().len() + state.fleet_problems().len(),
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        out,
        "standard build-out costs {bo_a:.0} A + {bo_c:.0} C, with the core kit at its base price"
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
