//! The economy phase (doc 11 §4.1 phase 8; S4 plan §4.1, §4.3, §4.4, §4.7).
//!
//! Each holding runs once a second, in the tick of the second given by its identity
//! ([`super::economy_slot`]): construction progress, power allocation, production and
//! delivery, starting queued items, and repair. Convoys run once a second in their fleet's
//! slot.

use nomad_types::{DepositKind, ModuleKind, Slot};

use super::power::{self, Consumer};
use super::rules::MILLI;
use super::{
    Construction, EconomyEvent, Hull, Stock, economy_slot, logistics, queue, richness_permille,
};
use crate::fleet::FleetEvent;
use crate::holding::HoldingId;
use crate::id::{CommanderId, SimId};
use crate::state::UniverseState;
use crate::time::{TICKS_PER_SECOND, Tick};

/// Runs the economy phase for this tick.
pub(crate) fn run(
    state: &mut UniverseState,
    now: Tick,
    events: &mut Vec<EconomyEvent>,
    fleet_events: &mut Vec<FleetEvent>,
) {
    prune_gone(state, events);
    let slot = now.get() % TICKS_PER_SECOND;
    let due: Vec<HoldingId> = state
        .holdings
        .keys()
        .copied()
        .filter(|&h| economy_slot(h) == slot)
        .collect();
    for holding in due {
        holding_second(state, holding, now, events, fleet_events);
    }
    logistics::run(state, now, slot, events, fleet_events);
}

/// Drops what refers to ships and fleets that are gone, most often destroyed in combat
/// earlier in the tick (phase 7): their cargo is lost, their convoy routes end, and rally
/// points that name them are cleared.
fn prune_gone(state: &mut UniverseState, events: &mut Vec<EconomyEvent>) {
    let lost: Vec<_> = state
        .economy
        .cargo
        .keys()
        .copied()
        .filter(|ship| !state.entities.has::<crate::fleet::Ship>(ship.sim_id()))
        .collect();
    for ship in lost {
        if let Some(cargo) = state.economy.cargo.remove(&ship) {
            events.push(EconomyEvent::CargoLost {
                ship,
                amount: cargo.stock,
                core_kit: cargo.core_kit,
            });
        }
    }
    let gone: Vec<_> = state
        .economy
        .convoys
        .keys()
        .copied()
        .filter(|fleet| !state.entities.has::<crate::fleet::Fleet>(fleet.sim_id()))
        .collect();
    for fleet in gone {
        state.economy.convoys.remove(&fleet);
        events.push(EconomyEvent::ConvoyEnded { fleet });
    }
    for economy in state.economy.holdings.values_mut() {
        if economy
            .rally
            .is_some_and(|f| !state.entities.has::<crate::fleet::Fleet>(f.sim_id()))
        {
            economy.rally = None;
        }
    }
}

fn holding_second(
    state: &mut UniverseState,
    holding: HoldingId,
    now: Tick,
    events: &mut Vec<EconomyEvent>,
    fleet_events: &mut Vec<FleetEvent>,
) {
    let Some(owner) = state.holdings.get(&holding).map(|h| h.owner) else {
        return;
    };
    state.economy.holdings.entry(holding).or_default();
    queue::progress(state, holding, now, events, fleet_events);
    power::allocate(state, holding, now);
    produce(state, holding, owner, now, events);
    queue::start_items(state, holding, events);
    repair(state, holding, owner, now, events);
}

/// A producer's nominal rate in milli-units per hour, before power.
fn nominal_rate(state: &UniverseState, holding: HoldingId, slot: Slot, kind: ModuleKind) -> u64 {
    let p = &state.rules.economy.production;
    let deposit = deposit_at(state, holding, slot);
    let richness = deposit.map_or(0, |d| richness_permille(d.richness));
    match (kind, deposit.map(|d| d.kind)) {
        (ModuleKind::Harvester, Some(DepositKind::OreField)) => {
            p.harvester_ore_per_h.saturating_mul(richness)
        }
        (ModuleKind::Extractor, Some(DepositKind::CrystalVein)) => {
            p.extractor_crystal_per_h.saturating_mul(richness)
        }
        (ModuleKind::Refinery, _) => p.refinery_alloy_per_h.saturating_mul(MILLI),
        _ => 0,
    }
}

/// The deposit under a deposit slot.
fn deposit_at(
    state: &UniverseState,
    holding: HoldingId,
    slot: Slot,
) -> Option<nomad_types::Deposit> {
    let Slot::Deposit(i) = slot else {
        return None;
    };
    let site = state.holdings.get(&holding)?.site;
    state
        .galaxy
        .site(site)?
        .deposits
        .get(usize::from(i))
        .copied()
}

/// A holding's nominal output per hour, (alloy, crystal) milli-units, from its completed
/// producers: refined ore is limited by both harvesting and refining (doc 14 §6).
pub(crate) fn nominal_output_per_h(state: &UniverseState, holding: HoldingId) -> (u64, u64) {
    let Some(e) = state.economy.holdings.get(&holding) else {
        return (0, 0);
    };
    let (mut ore, mut refine, mut crystal) = (0_u64, 0_u64, 0_u64);
    for (&slot, &id) in &e.slots {
        if state.entities.has::<Construction>(id) {
            continue;
        }
        let Some(kind) = state.module_kind(id) else {
            continue;
        };
        let rate = nominal_rate(state, holding, slot, kind);
        match kind {
            ModuleKind::Harvester => ore = ore.saturating_add(rate),
            ModuleKind::Refinery => refine = refine.saturating_add(rate),
            ModuleKind::Extractor => crystal = crystal.saturating_add(rate),
            _ => {}
        }
    }
    (ore.min(refine), crystal)
}

/// One second of harvesting, refining and extraction at `holding`, delivered to the pool or
/// the local buffer (doc 14 §4, §6; D-126).
fn produce(
    state: &mut UniverseState,
    holding: HoldingId,
    owner: CommanderId,
    now: Tick,
    events: &mut Vec<EconomyEvent>,
) {
    if !state
        .holdings
        .get(&holding)
        .is_some_and(|h| h.core_complete)
    {
        return;
    }
    let Some(e) = state.economy.holdings.get(&holding) else {
        return;
    };
    let (mut harvest, mut refine, mut extract, mut ore_cap) = (0_u64, 0_u64, 0_u64, 0_u64);
    for (&slot, &id) in &e.slots {
        if !state.is_working(id) {
            continue;
        }
        let Some(kind) = state.module_kind(id) else {
            continue;
        };
        let nominal = nominal_rate(state, holding, slot, kind);
        let rate = match e.allocation.share(Consumer::Module(id)) {
            Some((allocated, demand)) if demand > 0 => u64::try_from(
                u128::from(nominal) * u128::from(allocated.min(demand)) / u128::from(demand),
            )
            .unwrap_or(0),
            _ => 0,
        };
        match kind {
            ModuleKind::Harvester => {
                harvest = harvest.saturating_add(rate);
                ore_cap = ore_cap.saturating_add(nominal);
            }
            ModuleKind::Refinery => refine = refine.saturating_add(rate),
            ModuleKind::Extractor => extract = extract.saturating_add(rate),
            _ => {}
        }
    }
    let ore_cap = ore_cap.saturating_mul(state.rules.economy.production.ore_buffer_h);
    let Some(e) = state.economy.holdings.get_mut(&holding) else {
        return;
    };
    let harvested = e.harvest.second(harvest);
    e.ore = e.ore.saturating_add(harvested).min(ore_cap.max(e.ore));
    let quota = e.refine.second(refine);
    let refined = quota.min(e.ore);
    e.ore -= refined;
    let extracted = e.extract.second(extract);
    let output = Stock {
        alloy: refined,
        crystal: extracted,
    };
    if output.is_zero() {
        return;
    }
    let stats = state.economy.stats.entry(owner).or_default();
    stats.produced = stats.produced.plus(output);
    let minute = now.get() / (60 * TICKS_PER_SECOND);
    match stats.minutes.back_mut() {
        Some((m, amount)) if *m == minute => *amount = amount.plus(output),
        _ => stats.minutes.push_back((minute, output)),
    }
    while stats
        .minutes
        .front()
        .is_some_and(|&(m, _)| m.saturating_add(60) <= minute)
    {
        stats.minutes.pop_front();
    }
    deliver(state, holding, owner, output, events);
}

/// Puts production into the pool if `holding` is in a depot's system, otherwise into its
/// buffer; what does not fit is lost and counted (D-025, D-064).
fn deliver(
    state: &mut UniverseState,
    holding: HoldingId,
    owner: CommanderId,
    output: Stock,
    events: &mut Vec<EconomyEvent>,
) {
    if state.delivers_to_pool(holding) {
        queue::credit_pool(state, owner, output, events);
        return;
    }
    let cap = state.buffer_cap(holding);
    let Some(e) = state.economy.holdings.get_mut(&holding) else {
        return;
    };
    let (kept, lost) = output.clip(cap.minus(e.buffer));
    e.buffer = e.buffer.plus(kept);
    if !lost.is_zero() {
        let stats = state.economy.stats.entry(owner).or_default();
        stats.lost = stats.lost.plus(lost);
        events.push(EconomyEvent::BufferOverflow { holding, lost });
    }
}

/// Free repair once a minute, and the REPAIR ability every second (doc 14 §10; D-071).
fn repair(
    state: &mut UniverseState,
    holding: HoldingId,
    owner: CommanderId,
    now: Tick,
    events: &mut Vec<EconomyEvent>,
) {
    let rules = state.rules.economy.repair;
    let grace = crate::time::Ticks::from_secs(rules.damage_grace_s);
    let modules: Vec<SimId> = state
        .economy
        .holdings
        .get(&holding)
        .map_or_else(Vec::new, |e| e.slots.values().copied().collect());
    let damaged = |state: &UniverseState, id: SimId| {
        !state.entities.has::<Construction>(id)
            && state.entities.get::<Hull>(id).is_some_and(|h| h.hp < h.max)
    };
    let second = now.get() / TICKS_PER_SECOND;
    if second.is_multiple_of(60) {
        for &id in &modules {
            if !damaged(state, id) {
                continue;
            }
            if let Some(hull) = state.entities.get_mut::<Hull>(id)
                && hull
                    .last_damage
                    .is_none_or(|t| now.since(t).get() >= grace.get())
            {
                let step = u32::try_from(
                    u64::from(hull.max) * u64::from(rules.free_percent_per_min) / 100,
                )
                .unwrap_or(u32::MAX)
                .max(1);
                hull.hp = hull.hp.saturating_add(step).min(hull.max);
            }
        }
    }
    let repairing = state
        .economy
        .holdings
        .get(&holding)
        .is_some_and(|e| e.repairing);
    if !repairing {
        return;
    }
    let mut targets: Vec<(u64, SimId)> = modules
        .iter()
        .filter(|&&id| damaged(state, id))
        .filter_map(|&id| {
            let hull = state.entities.get::<Hull>(id)?;
            Some((u64::from(hull.hp) * 1_000 / u64::from(hull.max.max(1)), id))
        })
        .collect();
    if targets.is_empty() {
        if let Some(e) = state.economy.holdings.get_mut(&holding) {
            e.repairing = false;
        }
        events.push(EconomyEvent::RepairDone { holding });
        return;
    }
    let price = rules.ability_alloy_per_s.saturating_mul(MILLI);
    let pool = state.economy.pools.entry(owner).or_default();
    if pool.alloy < price {
        return;
    }
    pool.alloy -= price;
    targets.sort_unstable();
    let mut left = rules.ability_hull_per_s;
    for (_, id) in targets {
        if left == 0 {
            break;
        }
        if let Some(hull) = state.entities.get_mut::<Hull>(id) {
            let give = (hull.max - hull.hp).min(left);
            hull.hp += give;
            left -= give;
        }
    }
}
