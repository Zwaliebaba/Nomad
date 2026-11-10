//! *Build out* and *Logistics* (doc 13 §5; D-064, D-067, D-134, D-215, D-216; A2 plan §4.5,
//! §4.6).
//!
//! **Build out** follows each holding's blueprint (rules data, one per site kind, D-215;
//! Easy uses the generic one), one step at a time per holding:
//! 1. productive modules: a harvester or extractor per deposit and a refinery while the ore
//!    exceeds the refining; solar first where the core cannot power the next module;
//! 2. extra storage when the next kit's price exceeds the pool cap (D-189, D-282: racks gate
//!    each next kit), scored just above the expansion it unlocks;
//! 3. pylons N, E, S, W; 4. two turrets; 5. a dry dock only at the mother station (built)
//!    and one forward holding; empty sites stop after storage and pylons (their blueprint).
//!
//! Across holdings, productive steps go by value per cost ([`super::model`]: the change in
//! the commander's output after D-217 and D-281), so a step that adds nothing is not built.
//! Defensive steps follow, nearest the raider haven and the most hostiles first.
//!
//! **Logistics** (D-216): the first holding in a system without a depot gets storage, which
//! makes the whole system deliver to the pool (D-064). Only where no inner slot is free does
//! a convoy run: a MULE fleet on a loop of those holdings ending at the nearest depot (D-134).
//!
//! *Proposed* readings:
//! - a holding takes a new module only when nothing of its queue waits to be paid, so the
//!   pool is not promised twice;
//! - extra storage goes into a free inner slot no later blueprint step needs, else any;
//! - exposure is 100 per jump closer to a haven than 4, plus 100 per hostile ship there;
//! - the forward dry dock goes to the complete holding nearest a raider haven, then by
//!   identity, chosen once while it stays the commander's;
//! - Hard's turrets where raids struck score as a pylon plus a turret.

use std::collections::BTreeSet;

use nomad_types::{
    BuildItem, Command, EconomyCommand, HoldingId, ModuleKind, ShipClass, Slot, Stock,
};

use super::model::{self, Features};
use super::rules::BlueprintMode;
use super::view::{HoldingStatus, StrategicView, StrategistParams};
use super::{Plan, Turn};
use crate::view::BuildCheck;

/// A step one holding could take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) holding: HoldingId,
    pub(crate) kind: ModuleKind,
    pub(crate) slot: Slot,
    pub(crate) price: Stock,
    pub(crate) score: i64,
}

/// What a holding has or has queued in each slot.
fn taken(h: &HoldingStatus, slot: Slot) -> Option<ModuleKind> {
    h.modules
        .iter()
        .find(|m| m.slot == slot)
        .map(|m| m.kind)
        .or_else(|| {
            h.queue.iter().find_map(|q| match q.item {
                BuildItem::Module { kind, slot: s } if s == slot => Some(kind),
                _ => None,
            })
        })
}

/// Whether nothing of the holding's queue waits to be paid, kits and ships aside.
fn free_to_build(h: &HoldingStatus) -> bool {
    !h.queue
        .iter()
        .any(|q| !q.started && matches!(q.item, BuildItem::Module { .. }))
}

/// Power after every present and queued module, and the supply, MW.
fn power(f: &Features, h: &HoldingStatus) -> (u32, u32) {
    let mut kinds: Vec<ModuleKind> = h.modules.iter().map(|m| m.kind).collect();
    kinds.extend(h.queue.iter().filter_map(|q| match q.item {
        BuildItem::Module { kind, .. } => Some(kind),
        _ => None,
    }));
    let mut supply = if h.mother {
        f.numbers.mother_supply_mw
    } else {
        f.numbers.core_supply_mw
    };
    let mut draw = 0;
    for kind in kinds {
        let (d, s) = f.numbers.module_power(kind);
        draw += d;
        supply += s;
    }
    (draw, supply)
}

/// Jumps-to-haven and hostiles make a holding exposed.
fn exposure(f: &Features, h: &HoldingStatus) -> i64 {
    let jumps = f.haven_jumps.get(&h.system).copied().unwrap_or(4);
    let hostiles = f.hostiles.get(&h.system).copied().unwrap_or(0);
    i64::from(4_u32.saturating_sub(jumps)) * 100 + i64::from(hostiles) * 100
}

/// The forward holding for the second dry dock.
fn forward_dock(f: &Features, chosen: &mut Option<HoldingId>) -> Option<HoldingId> {
    if let Some(id) = *chosen
        && f.holding(id).is_some_and(|h| h.complete)
    {
        return Some(id);
    }
    let pick = f
        .complete()
        .filter(|h| !h.mother)
        .map(|h| {
            (
                f.haven_jumps.get(&h.system).copied().unwrap_or(u32::MAX),
                h.id,
            )
        })
        .min()
        .map(|(_, id)| id);
    *chosen = pick;
    pick
}

/// Every holding's next build-out step, best first.
#[expect(
    clippy::too_many_lines,
    reason = "the blueprint walk reads best in one place"
)]
pub(crate) fn steps(
    f: &Features,
    forward: &mut Option<HoldingId>,
    struck: &BTreeSet<HoldingId>,
    params: &StrategistParams<'_>,
    expand_score: i64,
) -> Vec<Step> {
    let rules = params.rules;
    let w = rules.weights;
    let mode = params.blueprints;
    let output = f.output();
    let forward = forward_dock(f, forward);
    let mut out = Vec::new();
    // Extra storage when the next kit exceeds the pool cap (D-189, D-282).
    // The kit waiting in a queue, at its fixed price, else the next one's (D-189).
    let kit = f
        .holdings
        .iter()
        .flat_map(|h| h.queue.iter())
        .filter(|q| q.item == BuildItem::CoreKit && !q.started)
        .map(|q| q.price)
        .max()
        .unwrap_or(f.economy.core_kit_price);
    let storage_building = f.holdings.iter().any(|h| {
        h.modules
            .iter()
            .any(|m| m.kind == ModuleKind::Storage && m.building)
            || h.queue.iter().any(|q| {
                matches!(
                    q.item,
                    BuildItem::Module {
                        kind: ModuleKind::Storage,
                        ..
                    }
                )
            })
    });
    if expand_score > 0 && !f.economy.cap.covers(kit) && !storage_building {
        let mut homes: Vec<&HoldingStatus> = f.complete().filter(|h| free_to_build(h)).collect();
        homes.sort_by_key(|h| (!h.mother, h.id));
        for h in homes {
            if let Some(slot) = storage_slot(f, h, params) {
                out.push(Step {
                    holding: h.id,
                    kind: ModuleKind::Storage,
                    slot,
                    price: f.numbers.module_cost(ModuleKind::Storage),
                    score: expand_score.saturating_add(1),
                });
                break;
            }
        }
    }
    for h in f.complete() {
        if !free_to_build(h) {
            continue;
        }
        let deposits = f.deposits(h.site);
        let blueprint = rules.blueprint(model::site_kind(deposits, h.mother), mode);
        let (draw, supply) = power(f, h);
        let solar_free = taken(h, Slot::Solar).is_none() && h.slots.contains(&Slot::Solar);
        let first = |kind: ModuleKind, slot: Slot| -> (ModuleKind, Slot) {
            let need = f.numbers.module_power(kind).0;
            if kind != ModuleKind::Solar && draw + need > supply && solar_free {
                (ModuleKind::Solar, Slot::Solar)
            } else {
                (kind, slot)
            }
        };
        // Productive: the rest of the blueprint's producers, for their value per cost.
        let productive = remaining_productive(f, h, blueprint);
        if let Some(&(kind, slot)) = productive.first() {
            let building: Vec<(ModuleKind, Slot)> = h
                .modules
                .iter()
                .filter(|m| m.building)
                .map(|m| (m.kind, m.slot))
                .collect();
            let complete: Vec<_> = h.modules.iter().filter(|m| !m.building).copied().collect();
            let now = model::nominal(&f.numbers, deposits, &complete, &building);
            let mut all = building.clone();
            all.extend(productive.iter().copied());
            let full = model::nominal(&f.numbers, deposits, &complete, &all);
            let mut base = output.clone();
            let current = Stock {
                alloy: now.0,
                crystal: now.1,
            };
            if h.mother {
                base.mother = current;
            } else {
                base.remote.insert(h.id, current);
            }
            let value = base.marginal(
                Some(h.id),
                h.mother,
                Stock {
                    alloy: full.0,
                    crystal: full.1,
                },
                f.numbers.diminishing_yield_permille,
                rules.crystal_weight,
            );
            let mut cost: u64 = productive
                .iter()
                .map(|&(k, _)| model::weighed(f.numbers.module_cost(k), rules.crystal_weight))
                .sum();
            let (step_kind, step_slot) = first(kind, slot);
            if step_kind == ModuleKind::Solar {
                cost += model::weighed(
                    f.numbers.module_cost(ModuleKind::Solar),
                    rules.crystal_weight,
                );
            }
            let score = model::return_score(value, cost);
            if score > 0 {
                out.push(Step {
                    holding: h.id,
                    kind: step_kind,
                    slot: step_slot,
                    price: f.numbers.module_cost(step_kind),
                    score,
                });
                continue;
            }
        }
        // Defensive: the blueprint's pylons, turrets and the forward dry dock.
        let raid_turrets = mode == BlueprintMode::PerSiteKindRaidTurrets && struck.contains(&h.id);
        for step in &blueprint.steps {
            if taken(h, step.slot).is_some() || !model::fits(step.kind, step.slot, deposits) {
                continue;
            }
            let weight = match step.kind {
                ModuleKind::Pylon | ModuleKind::Solar => w.pylon,
                ModuleKind::Turret if raid_turrets => w.pylon.saturating_add(w.turret),
                ModuleKind::Turret => w.turret,
                ModuleKind::DryDock if Some(h.id) == forward => w.dry_dock,
                _ => continue,
            };
            let (kind, slot) = first(step.kind, step.slot);
            out.push(Step {
                holding: h.id,
                kind,
                slot,
                price: f.numbers.module_cost(kind),
                score: weight.saturating_add(exposure(f, h)),
            });
            break;
        }
    }
    out.sort_by_key(|s| (std::cmp::Reverse(s.score), s.holding, s.slot));
    out
}

/// The blueprint's producers a holding still lacks, in blueprint order: its deposit modules,
/// then refineries while its ore exceeds its refining.
fn remaining_productive(
    f: &Features,
    h: &HoldingStatus,
    blueprint: &nomad_types::Blueprint,
) -> Vec<(ModuleKind, Slot)> {
    let deposits = f.deposits(h.site);
    let mut present: Vec<(ModuleKind, Slot)> = h.modules.iter().map(|m| (m.kind, m.slot)).collect();
    present.extend(h.queue.iter().filter_map(|q| match q.item {
        BuildItem::Module { kind, slot } => Some((kind, slot)),
        _ => None,
    }));
    let mut add = Vec::new();
    for step in &blueprint.steps {
        if matches!(step.kind, ModuleKind::Harvester | ModuleKind::Extractor)
            && taken(h, step.slot).is_none()
            && model::fits(step.kind, step.slot, deposits)
            && !add.iter().any(|&(_, s)| s == step.slot)
        {
            add.push((step.kind, step.slot));
        }
    }
    let ore = |list: &[(ModuleKind, Slot)]| -> (u64, u64) {
        let mut ore_mph = 0;
        let mut refine_mph = 0;
        for &(kind, slot) in list {
            match (kind, slot) {
                (ModuleKind::Harvester, Slot::Deposit(i)) => {
                    ore_mph += f.numbers.harvester_ore_per_h
                        * deposits
                            .get(usize::from(i))
                            .map_or(0, |d| u64::from(d.richness_permille));
                }
                (ModuleKind::Refinery, _) => refine_mph += f.numbers.refinery_alloy_per_h * 1_000,
                _ => {}
            }
        }
        (ore_mph, refine_mph)
    };
    for step in &blueprint.steps {
        if step.kind != ModuleKind::Refinery || taken(h, step.slot).is_some() {
            continue;
        }
        let all: Vec<_> = present.iter().chain(add.iter()).copied().collect();
        let (ore_mph, refine_mph) = ore(&all);
        if ore_mph > refine_mph {
            add.push((step.kind, step.slot));
        }
    }
    // Blueprint order.
    add.sort_by_key(|&(kind, slot)| {
        blueprint
            .steps
            .iter()
            .position(|s| s.kind == kind && s.slot == slot)
            .unwrap_or(usize::MAX)
    });
    add
}

/// A free inner slot for storage: one no later blueprint step needs, else any.
fn storage_slot(f: &Features, h: &HoldingStatus, params: &StrategistParams<'_>) -> Option<Slot> {
    let deposits = f.deposits(h.site);
    let blueprint = params
        .rules
        .blueprint(model::site_kind(deposits, h.mother), params.blueprints);
    let free: Vec<Slot> = h
        .slots
        .iter()
        .copied()
        .filter(|&s| matches!(s, Slot::Inner(_)) && taken(h, s).is_none())
        .collect();
    let wanted = |slot: Slot| {
        blueprint
            .steps
            .iter()
            .any(|s| s.slot == slot && s.kind != ModuleKind::Storage)
    };
    let planned = blueprint
        .steps
        .iter()
        .find(|s| s.kind == ModuleKind::Storage && free.contains(&s.slot))
        .map(|s| s.slot);
    planned
        .or_else(|| free.iter().rev().copied().find(|&s| !wanted(s)))
        .or_else(|| free.last().copied())
}

/// *Build out*'s score: its best step's.
pub(crate) fn score_build(
    _view: &impl StrategicView,
    f: &Features,
    forward: &mut Option<HoldingId>,
    struck: &BTreeSet<HoldingId>,
    params: &StrategistParams<'_>,
    expand_score: i64,
) -> i64 {
    steps(f, forward, struck, params, expand_score)
        .first()
        .map_or(0, |s| s.score)
}

/// Queues the best step the validator accepts, if the free pool covers it; otherwise
/// reserves it. Returns whether nothing is left to build.
pub(crate) fn advance_build(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    plan: &mut Plan,
    forward: &mut Option<HoldingId>,
    struck: &BTreeSet<HoldingId>,
    params: &StrategistParams<'_>,
) -> bool {
    let expand_score = super::expand::score(view, turn.f, None, params);
    let list = steps(turn.f, forward, struck, params, expand_score);
    if list.is_empty() {
        return true;
    }
    for step in list.iter().take(4) {
        let item = BuildItem::Module {
            kind: step.kind,
            slot: step.slot,
        };
        if view.build_check(step.holding, item) != BuildCheck::Ok {
            continue;
        }
        if turn.spend_or_reserve(step.price) {
            turn.build(step.holding, item);
        }
        plan.observe(
            step.holding.sim_id().get() ^ u64::from(step.slot_index()),
            true,
            turn.f.now,
        );
        return false;
    }
    false
}

impl Step {
    fn slot_index(&self) -> u32 {
        match self.slot {
            Slot::Core => 1,
            Slot::Pylon(i) => 10 + u32::from(i),
            Slot::Inner(i) => 20 + u32::from(i),
            Slot::Dock => 30,
            Slot::Solar => 31,
            Slot::Deposit(i) => 40 + u32::from(i),
        }
    }
}

/// What *Logistics* would do next.
enum LogisticsStep {
    /// Storage at the first holding of a system without a depot.
    Storage { holding: HoldingId, slot: Slot },
    /// A convoy over holdings with no free inner slot.
    Convoy { stops: Vec<HoldingId> },
}

/// The best logistics step and its score.
fn logistics(f: &Features, params: &StrategistParams<'_>) -> Option<(LogisticsStep, i64)> {
    let rules = params.rules;
    let output = f.output();
    let mut best: Option<(LogisticsStep, i64)> = None;
    let mut convoy_stops = Vec::new();
    let mut convoy_value = 0_i64;
    let mut seen_systems = BTreeSet::new();
    let convoyed: BTreeSet<HoldingId> = BTreeSet::new();
    for h in f.complete() {
        if h.delivers_to_pool || f.depot_in(h.system) {
            continue;
        }
        let value = -output.marginal(
            Some(h.id),
            h.mother,
            Stock::ZERO,
            f.numbers.diminishing_yield_permille,
            rules.crystal_weight,
        );
        if value <= 0 {
            continue;
        }
        // One storage per system makes it a depot.
        if seen_systems.contains(&h.system) {
            continue;
        }
        let storage_queued = h.queue.iter().any(|q| {
            matches!(
                q.item,
                BuildItem::Module {
                    kind: ModuleKind::Storage,
                    ..
                }
            )
        }) || h.modules.iter().any(|m| m.kind == ModuleKind::Storage);
        if storage_queued {
            seen_systems.insert(h.system);
            continue;
        }
        if free_to_build(h)
            && let Some(slot) = storage_slot(f, h, params)
        {
            seen_systems.insert(h.system);
            let score = model::return_score(
                value,
                model::weighed(
                    f.numbers.module_cost(ModuleKind::Storage),
                    rules.crystal_weight,
                ),
            );
            if best.as_ref().is_none_or(|b| score > b.1) {
                best = Some((
                    LogisticsStep::Storage {
                        holding: h.id,
                        slot,
                    },
                    score,
                ));
            }
        } else if !convoyed.contains(&h.id) && !h.buffer.is_zero() {
            convoy_stops.push(h.id);
            convoy_value += value;
        }
    }
    if best.is_none() && !convoy_stops.is_empty() {
        let mule = f
            .numbers
            .ship(ShipClass::Mule)
            .map_or(1, |(price, _)| model::weighed(price, rules.crystal_weight));
        convoy_stops.truncate(f.numbers.max_route_stops.saturating_sub(1).max(1));
        let score = model::return_score(convoy_value, mule);
        best = Some((
            LogisticsStep::Convoy {
                stops: convoy_stops,
            },
            score,
        ));
    }
    best
}

/// *Logistics*' score.
pub(crate) fn score_logistics(
    view: &impl StrategicView,
    f: &Features,
    params: &StrategistParams<'_>,
) -> i64 {
    let _ = view;
    let Some((step, score)) = logistics(f, params) else {
        return 0;
    };
    // A convoy already running over these stops needs nothing.
    if let LogisticsStep::Convoy { stops } = &step
        && covered(view, stops)
    {
        return 0;
    }
    score
}

fn covered(view: &impl StrategicView, stops: &[HoldingId]) -> bool {
    let convoys = view.own_convoys();
    stops
        .iter()
        .all(|h| convoys.iter().any(|c| c.route.contains(h)))
}

/// Takes the logistics step. Returns whether nothing is left to do.
pub(crate) fn advance_logistics(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    plan: &mut Plan,
) -> bool {
    let params = view.strategist();
    let f = turn.f;
    let Some((step, _)) = logistics(f, &params) else {
        return true;
    };
    match step {
        LogisticsStep::Storage { holding, slot } => {
            let item = BuildItem::Module {
                kind: ModuleKind::Storage,
                slot,
            };
            if view.build_check(holding, item) == BuildCheck::Ok
                && turn.spend_or_reserve(f.numbers.module_cost(ModuleKind::Storage))
            {
                turn.build(holding, item);
            }
            plan.observe(holding.sim_id().get(), true, f.now);
        }
        LogisticsStep::Convoy { stops } => {
            if covered(view, &stops) {
                return true;
            }
            // The nearest depot ends the loop (D-134): by jumps from the first stop.
            let first = stops.first().and_then(|h| f.holding(*h)).map(|h| h.system);
            let jumps = first
                .map(|s| model::jumps_from(&f.systems, &[s]))
                .unwrap_or_default();
            let depot = f
                .complete()
                .filter(|h| model::is_depot(h))
                .filter_map(|h| jumps.get(&h.system).map(|&j| (j, h.id)))
                .min()
                .map(|(_, id)| id);
            let Some(depot) = depot else {
                return true;
            };
            let mule = f
                .fleets
                .iter()
                .filter(|fl| Features::docked_at(fl).is_some())
                .find(|fl| f.is_free_mule(view, fl, &turn.busy));
            if let Some(mule) = mule {
                let mut route = stops;
                route.push(depot);
                turn.send(Command::Economy(EconomyCommand::Convoy {
                    fleet: mule.id,
                    route,
                }));
                plan.observe(mule.id.sim_id().get(), false, f.now);
            } else if let Some((price, command)) = f.numbers.ship(ShipClass::Mule)
                && f.economy.command_used + turn.command_planned + command <= f.economy.command_cap
            {
                let queued = f.holding(depot).is_some_and(|h| {
                    h.queue.iter().any(|q| {
                        q.item
                            == BuildItem::Ship {
                                class: ShipClass::Mule,
                            }
                    })
                });
                if !queued && turn.spend_or_reserve(price) {
                    turn.build(
                        depot,
                        BuildItem::Ship {
                            class: ShipClass::Mule,
                        },
                    );
                    turn.command_planned += command;
                }
                plan.observe(depot.sim_id().get(), true, f.now);
            }
        }
    }
    false
}
