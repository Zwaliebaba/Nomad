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
//! §13). Two rules order the spending around that reserve:
//! - **defend before the next kit (D-309):** the pylons and turrets of a holding whose core
//!   completed at least [`DEFEND_AFTER_H`] ago ignore the kit reserve, as the modules that
//!   grow income do;
//! - **storage before an oversized kit (D-310):** while the kit costs more than the pool cap
//!   and nothing else will raise it (a holding or rack lost since its price was fixed), the
//!   script builds storage first, in an inner slot its list leaves free, one at a time.
//!
//! Remote producers share D-217's diminishing yield per resource (D-281): alloy output is
//! scaled by min(1, F ÷ alloy-producing holdings other than the mother station), crystal by
//! the same over crystal-producing ones; each commander's line gives both factors and counts.
//!
//! The D-070 measures follow D-218 §5.4 as amended by D-283 ([`crate::pacing`]): storage fill
//! and the cap-fleet rebuild over days 21–35, late income over days 42–56, hours per new
//! holding as 72 h ÷ the holdings completed by hour 72, the rebuild in the Garrison stand-in's
//! composition, and build-out measured from each holding's core completion to the completion
//! of every pylon and turret of its list (D-295's reading of D-283). A run shorter
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
    /// When it first had no free site left to claim and no kit on the way.
    expansion_end: Option<u64>,
}

/// D-309: a holding's pylons and turrets go before the next kit once its core completed this
/// many hours ago.
const DEFEND_AFTER_H: u64 = 48;

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

    // Split the start kit's MULEs, a reserve fleet of their own (D-286), into one fleet each.
    if b.kit_mule.is_none() {
        let start = state.fleet_ids().into_iter().find_map(|f| {
            let v = state.fleet(f)?;
            let mules = v
                .ships
                .iter()
                .any(|s| state.ship(*s).is_some_and(|v| v.class == ShipClass::Mule));
            (v.owner == owner && mules).then_some(v)
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
        if b.target.is_none() && !carrying && b.expansion_end.is_none() {
            b.expansion_end = Some(tick);
        }
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
    // D-310: storage first while the kit costs more than the pool can hold.
    let mut storage_at = None;
    if !reserve.is_zero()
        && !economy.cap.covers(reserve)
        && let Some((holding, slot)) = storage_first(state, &economy, budget)
    {
        let build = Command::Economy(EconomyCommand::Build {
            holding,
            item: BuildItem::Module {
                kind: ModuleKind::Storage,
                slot,
            },
        });
        if validate_command(state, owner, &build).is_ok() {
            budget = budget.minus(module_cost(state, ModuleKind::Storage));
            storage_at = Some(holding);
            commands.push(build);
        }
    }
    let hour = 3_600 * TICKS_PER_SECOND;
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
        // D-283 as D-295 reads it: every pylon and turret of the list complete.
        let defended = list
            .iter()
            .filter(|(kind, _)| matches!(kind, ModuleKind::Pylon | ModuleKind::Turret))
            .all(|(kind, slot)| {
                h.slots.iter().any(|(s, m)| {
                    s == slot && m.is_some_and(|m| m.kind == *kind && m.building_ms.is_none())
                })
            });
        if defended {
            b.built_out.entry(h.holding).or_insert(tick);
        }
        if !h.queue.is_empty() || storage_at == Some(h.holding) {
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
        let core_age_h = b
            .completed_at
            .get(&h.holding)
            .map_or(0, |&done| (tick - done) / hour);
        let defend =
            matches!(kind, ModuleKind::Pylon | ModuleKind::Turret) && core_age_h >= DEFEND_AFTER_H;
        let keep = if grows || defend {
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

/// D-310: where to build storage for a kit priced above the pool cap, if nothing else will
/// raise the cap: no storage is on its way and every holding, outposts included, has its
/// list's rack (a kit priced above the cap when queued is normal, and the newest holding's
/// rack lifts the cap, D-282). Then the first complete holding (in identity order) with an
/// empty queue and an inner slot its list leaves free, if `budget` pays for it.
fn storage_first(
    state: &UniverseState,
    economy: &nomad_sim::CommanderEconomy,
    budget: Stock,
) -> Option<(HoldingId, Slot)> {
    let storage_in = |h: &nomad_sim::HoldingEconomyView, slot: Slot| {
        h.slots
            .iter()
            .any(|(s, m)| *s == slot && m.is_some_and(|m| m.kind == ModuleKind::Storage))
    };
    let coming = economy.holdings.iter().any(|h| {
        h.slots.iter().any(|(_, m)| {
            m.is_some_and(|m| m.kind == ModuleKind::Storage && m.building_ms.is_some())
        }) || h.queue.iter().any(|q| {
            matches!(
                q.item,
                BuildItem::Module {
                    kind: ModuleKind::Storage,
                    ..
                }
            )
        }) || build_out(state, h.holding)
            .into_iter()
            .any(|(kind, slot)| kind == ModuleKind::Storage && !storage_in(h, slot))
    });
    if coming || !budget.covers(module_cost(state, ModuleKind::Storage)) {
        return None;
    }
    economy
        .holdings
        .iter()
        .filter(|h| h.queue.is_empty())
        .filter(|h| state.holding(h.holding).is_some_and(|x| x.core_complete))
        .find_map(|h| {
            let list = build_out(state, h.holding);
            h.slots
                .iter()
                .filter(|(s, m)| matches!(s, Slot::Inner(_)) && m.is_none())
                .map(|(s, _)| *s)
                .find(|s| !list.iter().any(|(_, used)| used == s))
                .map(|slot| (h.holding, slot))
        })
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
            writeln!(file, "{}", pacing::CSV_HEADER).map_err(|e| e.to_string())?;
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
                    pacing::write_csv_row(file, &sample, c, &e)?;
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
    let mut m = pacing::Measures::default();
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
        // D-283: from each remote holding's core completion to its build-out (D-295).
        let build_outs: Vec<pacing::BuildOut> = b
            .completed_at
            .iter()
            .filter(|(holding, _)| {
                state
                    .holding(**holding)
                    .is_some_and(|x| x.owner == c && !x.mother_station)
            })
            .map(|(holding, &done)| pacing::BuildOut {
                hours: b.built_out.get(holding).map(|&built| hours(built - done)),
                age_h: hours(ticks - done),
            })
            .collect();
        m.add(
            &pacing::CommanderRun {
                samples: series,
                start_cap,
                completions: &completions,
                first_kit: b.first_kit_start.map(hours),
                build_outs: &build_outs,
            },
            per_command,
            mid,
            late,
        );
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
            "  C{}: {holdings} holdings (completions at {} h; {}) | income {:.0} A/h {:.0} C/h | D-281 yield {}‰ over {} alloy, {}‰ over {} crystal producers | pool {:.0} A {:.0} C of {:.0}/{:.0} | lost {:.0} A {:.0} C | command {}/{} | first kit {} | expansion ended {}",
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
            b.expansion_end.map_or("no".to_owned(), |t| format!("{:.1} h", hours(t))),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_for_an_oversized_kit_goes_where_the_list_leaves_room() {
        let state =
            generate_stage1_with_rules(SeasonConfig::new(1, 0), COMMANDERS, Rules::embedded())
                .unwrap();
        let c = COMMANDERS[0];
        let mother = mother(&state, c).unwrap();
        let economy = state.economy_of(c);
        // The mother station's own rack is built at the start, so the extra one takes an
        // inner slot its list leaves free: Inner 4 (the list uses 0 to 3).
        assert_eq!(
            storage_first(&state, &economy, Stock::units(10_000, 0)),
            Some((mother, Slot::Inner(4)))
        );
        // Not while the pool cannot pay for it.
        assert_eq!(
            storage_first(&state, &economy, Stock::units(1_000, 0)),
            None
        );
    }
}
