//! Power supply, demand and allocation per holding (doc 14 §7, doc 15 §5.2; D-127, D-128).
//!
//! **Supply** is the core plus working solar arrays. **Demand** comes in two kinds:
//!
//! 1. **Fixed draws, all-or-nothing, served first:** pylon base draw, turrets and active
//!    abilities, in that order (pylons and turrets by slot). One that does not fit gets
//!    nothing: a turret without its full draw holds fire.
//! 2. **Scalable draws, by profile tiers:** sector regeneration, producers (harvester,
//!    refinery, extractor) and dry docks that are building. Within a tier every consumer
//!    gets the same fraction of its demand, computed in integer kW.
//!
//! | Profile | Tier 2 | Tier 3 | Tier 4 |
//! |---|---|---|---|
//! | Balanced | Regeneration and production | Building dry dock | — |
//! | Shields first | Regeneration | Production | Building dry dock |
//! | Production first | Production and building dry dock | Regeneration | — |
//!
//! Power never travels between holdings (D-127).

use nomad_types::{ModuleKind, PowerProfile, Slot};
use serde::{Deserialize, Serialize};

use super::rules::KW_PER_MW;
use super::{BuildLane, EntryState};
use crate::holding::HoldingId;
use crate::id::SimId;
use crate::state::UniverseState;
use crate::time::Tick;

/// What draws power.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Consumer {
    /// A module: a pylon's base draw, a turret, a producer, or a building dry dock.
    Module(SimId),
    /// Regeneration of the shield sector with this index.
    Regeneration(u8),
    /// An active ability, in the order it started.
    Ability(u8),
}

/// One consumer's share.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerAllocation {
    /// Who.
    pub consumer: Consumer,
    /// What it asks for, kW.
    pub demand_kw: u64,
    /// What it gets, kW. Fixed draws get all or nothing.
    pub allocated_kw: u64,
}

/// A holding's power report and allocation (S4/S5 interface; the phase-1
/// "1,140 / 1,400 MW · 81% LOAD").
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerAllocation {
    /// Supply, kW.
    pub supply_kw: u64,
    /// Total demand, kW.
    pub demand_kw: u64,
    /// Total allocated, kW.
    pub allocated_kw: u64,
    /// Supply left after the fixed draws, kW: what an ability may claim.
    pub headroom_kw: u64,
    /// Every consumer, fixed draws first.
    pub consumers: Vec<ConsumerAllocation>,
}

impl PowerAllocation {
    /// The share of `consumer` as (allocated, demand) kW, or `None` if it draws nothing.
    pub fn share(&self, consumer: Consumer) -> Option<(u64, u64)> {
        self.consumers
            .iter()
            .find(|c| c.consumer == consumer)
            .map(|c| (c.allocated_kw, c.demand_kw))
    }

    /// Whether a fixed-draw module (pylon, turret) has its full draw.
    pub fn powered(&self, module: SimId) -> bool {
        self.share(Consumer::Module(module))
            .is_some_and(|(allocated, demand)| allocated >= demand)
    }

    /// Regeneration power of a sector, kW (S5b turns it into a rate, D-142).
    pub fn regeneration_kw(&self, sector: u8) -> u64 {
        self.share(Consumer::Regeneration(sector))
            .map_or(0, |(allocated, _)| allocated)
    }

    /// Load in per mille of supply.
    pub fn load_permille(&self) -> u64 {
        self.allocated_kw
            .saturating_mul(1_000)
            .checked_div(self.supply_kw)
            .unwrap_or(0)
    }
}

/// Which tier a scalable consumer is in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Regeneration,
    Production,
    Dock,
}

fn tiers(profile: PowerProfile) -> &'static [&'static [Kind]] {
    match profile {
        PowerProfile::Balanced => &[&[Kind::Regeneration, Kind::Production], &[Kind::Dock]],
        PowerProfile::ShieldsFirst => &[&[Kind::Regeneration], &[Kind::Production], &[Kind::Dock]],
        PowerProfile::ProductionFirst => &[&[Kind::Production, Kind::Dock], &[Kind::Regeneration]],
    }
}

/// Recomputes `holding`'s allocation now and stores it.
pub(crate) fn allocate(state: &mut UniverseState, holding: HoldingId, now: Tick) {
    let allocation = compute(state, holding, now);
    let economy = state.economy.holdings.entry(holding).or_default();
    economy.abilities.retain(|a| a.until > now);
    economy.allocation = allocation;
}

/// Supply, fixed draws in serving order, and scalable draws with their tier kind.
type Demands = (u64, Vec<(Consumer, u64)>, Vec<(Kind, Consumer, u64)>);

/// What `holding` supplies and asks for now.
fn demands(state: &UniverseState, holding: HoldingId, now: Tick) -> Option<Demands> {
    let h = state.holdings.get(&holding)?;
    let economy = state.economy.holdings.get(&holding)?;
    let rules = &state.rules.economy;
    let mw = |v: u64| v.saturating_mul(KW_PER_MW);

    let mut supply = 0_u64;
    if h.core_complete
        && economy
            .slots
            .get(&Slot::Core)
            .is_some_and(|&c| state.is_working(c))
    {
        supply = mw(if h.mother_station {
            rules.core.mother_supply_mw
        } else {
            rules.core.supply_mw
        });
    }
    let mut fixed: Vec<(Consumer, u64)> = Vec::new();
    let mut scalable: Vec<(Kind, Consumer, u64)> = Vec::new();
    let mut turrets = Vec::new();
    for &id in economy.slots.values() {
        if !state.is_working(id) {
            continue;
        }
        let Some(kind) = state.module_kind(id) else {
            continue;
        };
        match kind {
            ModuleKind::Solar => supply = supply.saturating_add(mw(rules.module(kind).supply_mw)),
            ModuleKind::Pylon => fixed.push((Consumer::Module(id), mw(rules.module(kind).draw_mw))),
            ModuleKind::Turret => {
                turrets.push((Consumer::Module(id), mw(rules.module(kind).draw_mw)));
            }
            ModuleKind::Harvester | ModuleKind::Refinery | ModuleKind::Extractor => {
                scalable.push((
                    Kind::Production,
                    Consumer::Module(id),
                    mw(rules.module(kind).draw_mw),
                ));
            }
            ModuleKind::DryDock => {
                let building = economy.queue.iter().any(|entry| {
                    matches!(entry.state, EntryState::Building { lane: BuildLane::Dock(d), .. } if d == id)
                });
                if building {
                    scalable.push((
                        Kind::Dock,
                        Consumer::Module(id),
                        mw(rules.module(kind).draw_mw),
                    ));
                }
            }
            ModuleKind::Core | ModuleKind::Storage => {}
        }
    }
    fixed.extend(turrets);
    for (i, ability) in economy
        .abilities
        .iter()
        .filter(|a| a.until > now)
        .enumerate()
    {
        fixed.push((
            Consumer::Ability(u8::try_from(i).unwrap_or(u8::MAX)),
            ability.draw_kw,
        ));
    }
    // Regeneration: S5b sets the demand, capped by the sector's maximum draw (D-142).
    let pylons = state.pylons_per_sector(holding);
    for (sector, &demand) in economy.regen_demand_kw.iter().enumerate() {
        let full = mw(rules.power.regen_mw_per_sector);
        let cap = if pylons[sector] { full } else { full / 4 };
        let demand = demand.min(cap);
        if demand > 0 {
            scalable.push((
                Kind::Regeneration,
                Consumer::Regeneration(u8::try_from(sector).unwrap_or(u8::MAX)),
                demand,
            ));
        }
    }
    Some((supply, fixed, scalable))
}

/// The allocation `holding` would get now.
fn compute(state: &UniverseState, holding: HoldingId, now: Tick) -> PowerAllocation {
    let Some((supply, fixed, scalable)) = demands(state, holding, now) else {
        return PowerAllocation::default();
    };
    let profile = state
        .economy
        .holdings
        .get(&holding)
        .map(|e| e.profile)
        .unwrap_or_default();
    let mut remaining = supply;
    let mut consumers = Vec::with_capacity(fixed.len() + scalable.len());
    for (consumer, demand) in fixed {
        let allocated = if demand <= remaining { demand } else { 0 };
        remaining -= allocated;
        consumers.push(ConsumerAllocation {
            consumer,
            demand_kw: demand,
            allocated_kw: allocated,
        });
    }
    let headroom = remaining;
    for tier in tiers(profile) {
        let members: Vec<&(Kind, Consumer, u64)> = scalable
            .iter()
            .filter(|(k, _, _)| tier.contains(k))
            .collect();
        let total: u64 = members.iter().map(|(_, _, d)| *d).sum();
        let given_total = remaining.min(total);
        for &&(_, consumer, demand) in &members {
            let allocated = if total == 0 {
                0
            } else {
                u64::try_from(u128::from(demand) * u128::from(given_total) / u128::from(total))
                    .unwrap_or(0)
            };
            consumers.push(ConsumerAllocation {
                consumer,
                demand_kw: demand,
                allocated_kw: allocated,
            });
        }
        let used: u64 = consumers
            .iter()
            .rev()
            .take(members.len())
            .map(|c| c.allocated_kw)
            .sum();
        remaining = remaining.saturating_sub(used);
    }
    let demand_kw = consumers.iter().map(|c| c.demand_kw).sum();
    let allocated_kw = consumers.iter().map(|c| c.allocated_kw).sum();
    PowerAllocation {
        supply_kw: supply,
        demand_kw,
        allocated_kw,
        headroom_kw: headroom,
        consumers,
    }
}

/// Whether `draw_kw` of fixed power for an ability would be granted at `holding` now (S4/S5
/// interface; D-128): the validator's check, which [`request_ability_power`] repeats.
pub(crate) fn ability_power_available(
    state: &UniverseState,
    holding: HoldingId,
    draw_kw: u64,
    now: Tick,
) -> bool {
    draw_kw <= compute(state, holding, now).headroom_kw
}

/// Claims `draw_kw` of fixed power at `holding` for an ability until `until` (S4/S5
/// interface). Granted only if the full draw fits after the fixed draws; the ability then
/// joins them until it ends, and the allocation is recomputed now.
pub(crate) fn request_ability_power(
    state: &mut UniverseState,
    holding: HoldingId,
    draw_kw: u64,
    now: Tick,
    until: Tick,
) -> bool {
    if !ability_power_available(state, holding, draw_kw, now) || until <= now {
        return false;
    }
    let Some(economy) = state.economy.holdings.get_mut(&holding) else {
        return false;
    };
    economy
        .abilities
        .push(super::AbilityDraw { draw_kw, until });
    allocate(state, holding, now);
    true
}
