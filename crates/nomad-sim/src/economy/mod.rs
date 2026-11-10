//! The economy (Design doc 14; S4 plan; D-064 to D-071, D-112, D-126 to D-138).
//!
//! Holdings produce, store, spend and build. Every commander has one **pool** of alloy and
//! crystal (D-064), fed by harvesting, refining and extraction at holdings in a depot's
//! system, and by convoys from the local buffers of other holdings. Each holding has power
//! (D-127, D-128), build slots (D-130, D-131) and a build queue (D-068). A commander's
//! holdings other than the mother station share a diminishing yield (D-217), counted per
//! resource (D-281), which the views show ([`CommanderEconomy::yield_permille`]).
//!
//! **Modules are entities** ([`crate::entities`]) with these components:
//!
//! | Component | Fields |
//! |---|---|
//! | [`Module`] | holding, slot, kind |
//! | [`Hull`] | hit points, maximum, the last tick it took damage |
//! | [`Construction`] | build time left, while it is being built |
//! | [`Disabled`] | since when, while it is knocked offline (doc 12 §3) |
//!
//! Everything else is in [`Economy`], part of the Universe state: pools, per-holding
//! economy (slots, flows, buffers, the queue, power), convoys and MULE cargo.
//!
//! **Time:** the economy runs in phase 8 at 1 Hz per holding, staggered over the ten ticks
//! of a second by holding identity (doc 11 §4.1). Amounts are milli-units and power is kW,
//! so every step is integer and exact (D-049); see [`Flow`].

mod command;
pub(crate) mod logistics;
mod power;
mod queue;
pub mod rules;
mod tick;

#[cfg(test)]
mod tests;

pub(crate) use command::{end_convoy, execute, validate};
pub(crate) use power::{ability_power_available, request_ability_power};
pub(crate) use queue::price as item_price;
pub(crate) use tick::run;

use std::collections::{BTreeMap, VecDeque};

pub use nomad_types::Stock;
use nomad_types::{BuildItem, DepositKind, ModuleKind, PowerProfile, Richness, Slot};
use serde::{Deserialize, Serialize};

use crate::fleet::{FleetId, Ship, ShipId};
use crate::galaxy::SiteId;
use crate::holding::HoldingId;
use crate::id::{CommanderId, SimId};
use crate::state::UniverseState;
use crate::time::{TICKS_PER_SECOND, Tick};

use self::rules::{MILLI, SlotTemplate};

pub use self::power::{Consumer, ConsumerAllocation, PowerAllocation};

/// A station module (component).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Module {
    pub(crate) holding: HoldingId,
    pub(crate) slot: Slot,
    pub(crate) kind: ModuleKind,
}

/// Hit points (component). Modules have it; S5 may give it to ships too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Hull {
    pub(crate) hp: u32,
    pub(crate) max: u32,
    /// The last tick it took damage, which holds back free repair (D-071).
    pub(crate) last_damage: Option<Tick>,
}

/// A module being built (component): it exists with full hull, draws no power and produces
/// nothing until done (S4 plan §4.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Construction {
    /// Build time left, in milliseconds.
    pub(crate) remaining_ms: u64,
}

/// A module knocked offline (component): it produces, supplies and draws nothing
/// (doc 12 §3; S5b restores it under D-149).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Disabled {
    pub(crate) since: Tick,
}

/// An exact production flow (S4 plan §4.1): over `s` whole seconds at `rate` milli-units per
/// hour it has credited exactly `⌊rate × s / 3600⌋`, with no drift and no floating point. A
/// new rate restarts the count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Flow {
    rate: u64,
    seconds: u64,
}

impl Flow {
    /// The milli-units of one more second at `rate` per hour.
    pub(crate) fn second(&mut self, rate: u64) -> u64 {
        if rate != self.rate {
            *self = Self { rate, seconds: 0 };
        }
        let at = |s: u64| u128::from(rate) * u128::from(s) / 3_600;
        let credit = at(self.seconds + 1) - at(self.seconds);
        self.seconds = self.seconds.saturating_add(1);
        u64::try_from(credit).unwrap_or(u64::MAX)
    }
}

/// One holding's economy (S4 plan §4.2–§4.7).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HoldingEconomy {
    /// Modules by slot, including those being built.
    pub(crate) slots: BTreeMap<Slot, SimId>,
    pub(crate) profile: PowerProfile,
    /// Harvested ore waiting for a refinery (doc 14 §6).
    pub(crate) ore: u64,
    /// The local buffer of a holding outside a depot's system (D-064).
    pub(crate) buffer: Stock,
    pub(crate) harvest: Flow,
    pub(crate) refine: Flow,
    pub(crate) extract: Flow,
    pub(crate) queue: Vec<QueueEntry>,
    /// The fleet new ships join (RALLY).
    pub(crate) rally: Option<FleetId>,
    /// The last power allocation, which S5 reads every tick.
    pub(crate) allocation: PowerAllocation,
    /// Fixed draws of active abilities (S5), served in tier 1 until they end.
    pub(crate) abilities: Vec<AbilityDraw>,
    /// Regeneration demand per sector, set by S5b ([`crate::defence`]) every tick; zero
    /// while a sector is full or open.
    pub(crate) regen_demand_kw: [u64; 4],
    /// The REPAIR ability is on (doc 15 §5.3).
    pub(crate) repairing: bool,
    /// Finished core kits waiting for a MULE.
    pub(crate) kits: u32,
    /// When its core completed, which its owner knows; `None` for a holding placed complete,
    /// such as a mother station. The Strategist defends a holding before the next kit once
    /// its core completed 48 h ago (D-309).
    pub(crate) completed_at: Option<Tick>,
}

/// An ability's fixed power draw (S4/S5 interface).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AbilityDraw {
    pub(crate) draw_kw: u64,
    /// The first tick it no longer draws.
    pub(crate) until: Tick,
}

/// One build queue item (D-068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct QueueEntry {
    pub(crate) id: SimId,
    pub(crate) item: BuildItem,
    /// What it costs, fixed when it was queued and paid when it starts. A core kit's price
    /// grows with the commander's size (D-189); every other price is the season's rules
    /// cost, which does not change (D-104).
    pub(crate) price: Stock,
    pub(crate) state: EntryState,
    /// Why it last could not start, so a waiting event is sent once per reason.
    pub(crate) short: Option<Shortage>,
}

/// Whether a queue item waits or builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum EntryState {
    Waiting,
    Building {
        lane: BuildLane,
        remaining_ms: u64,
        /// What was paid, for the half refund on cancel.
        paid: Stock,
        /// The module being built, for module items.
        module: Option<SimId>,
    },
}

/// Where a queue item builds: the holding's main lane, or a dry dock's ship lane (D-068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BuildLane {
    /// The main lane: modules, core kits and small craft.
    Main,
    /// The ship lane of this dry dock.
    Dock(SimId),
}

/// Why a queue item cannot start yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Shortage {
    /// The pool has too little alloy.
    Alloy,
    /// The pool has too little crystal.
    Crystal,
    /// The ship would exceed the command cap (D-129).
    CommandCap,
}

/// A MULE fleet's convoy route (D-134).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Convoy {
    pub(crate) route: Vec<HoldingId>,
    /// The stop it is heading for.
    pub(crate) next: usize,
    /// The contested gate it holds for, if it is holding at its last stop (D-055).
    pub(crate) held: Option<crate::galaxy::GateId>,
}

/// A value per pooled resource: D-281 counts and scales alloy and crystal separately.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PerResource<T> {
    /// The alloy value.
    pub alloy: T,
    /// The crystal value.
    pub crystal: T,
}

/// What a MULE carries (D-133).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cargo {
    /// Alloy and crystal.
    pub stock: Stock,
    /// A core kit, which fills the hold.
    pub core_kit: bool,
}

/// What a commander produced and lost, for views and the harness.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stats {
    /// Everything produced since the season start (alloy refined, crystal extracted).
    pub(crate) produced: Stock,
    /// Lost to a full pool or a full buffer (D-025, D-064).
    pub(crate) lost: Stock,
    /// Production per minute over the last hour, oldest first: (minute, amount).
    pub(crate) minutes: VecDeque<(u64, Stock)>,
}

/// The economy part of the Universe state (S4 plan §4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Economy {
    pub(crate) pools: BTreeMap<CommanderId, Stock>,
    pub(crate) holdings: BTreeMap<HoldingId, HoldingEconomy>,
    pub(crate) convoys: BTreeMap<FleetId, Convoy>,
    /// Claim trips that continue offline, with their site (D-291).
    pub(crate) claims: BTreeMap<FleetId, SiteId>,
    pub(crate) cargo: BTreeMap<ShipId, Cargo>,
    pub(crate) stats: BTreeMap<CommanderId, Stats>,
}

/// What the economy did on one tick (S4 plan §4.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyEvent {
    /// An item joined a queue.
    ItemQueued {
        /// The holding.
        holding: HoldingId,
        /// The queue item.
        item: SimId,
    },
    /// An item was paid for and started.
    ItemStarted {
        /// The holding.
        holding: HoldingId,
        /// The queue item.
        item: SimId,
    },
    /// An item could not start; sent once per reason.
    ItemWaiting {
        /// The holding.
        holding: HoldingId,
        /// The queue item.
        item: SimId,
        /// What is short.
        short: Shortage,
    },
    /// An item finished.
    ItemCompleted {
        /// The holding.
        holding: HoldingId,
        /// The queue item.
        item: SimId,
    },
    /// An item was cancelled.
    ItemCancelled {
        /// The holding.
        holding: HoldingId,
        /// The queue item.
        item: SimId,
        /// What went back to the pool.
        refund: Stock,
    },
    /// A ship was built.
    ShipBuilt {
        /// Where.
        holding: HoldingId,
        /// The ship.
        ship: ShipId,
        /// The fleet it joined.
        fleet: FleetId,
    },
    /// Production did not fit into the pool and was lost (D-025).
    PoolOverflow {
        /// Whose pool.
        commander: CommanderId,
        /// What was lost.
        lost: Stock,
    },
    /// Production did not fit into a holding's buffer and was lost (D-064).
    BufferOverflow {
        /// The holding.
        holding: HoldingId,
        /// What was lost.
        lost: Stock,
    },
    /// A module was knocked offline.
    ModuleDisabled {
        /// The module.
        module: SimId,
    },
    /// A disabled module came back online.
    ModuleRestored {
        /// The module.
        module: SimId,
    },
    /// A module was destroyed and its slot freed.
    ModuleDestroyed {
        /// The module.
        module: SimId,
        /// Where it was.
        holding: HoldingId,
        /// What it was.
        kind: ModuleKind,
        /// The slot it was in, for the Deputy's rebuild in the same slot (D-208).
        slot: Slot,
    },
    /// A core kit was deployed: the site is now an outpost (D-054).
    OutpostDeployed {
        /// The new holding.
        holding: HoldingId,
        /// Its site.
        site: SiteId,
    },
    /// An outpost's core is complete: it is a holding and counts for control (D-041).
    HoldingCompleted {
        /// The holding.
        holding: HoldingId,
    },
    /// A MULE loaded a core kit.
    KitLoaded {
        /// The fleet.
        fleet: FleetId,
        /// Where.
        holding: HoldingId,
    },
    /// A convoy loaded from a holding's buffer.
    ConvoyLoaded {
        /// The fleet.
        fleet: FleetId,
        /// Where.
        holding: HoldingId,
        /// What it took.
        amount: Stock,
    },
    /// A convoy unloaded into the pool at a depot.
    ConvoyUnloaded {
        /// The fleet.
        fleet: FleetId,
        /// Where.
        holding: HoldingId,
        /// What it delivered.
        amount: Stock,
        /// What did not fit into the pool and was lost.
        lost: Stock,
    },
    /// A convoy holds at its last stop: the next leg's route passes a gate armed hostiles
    /// contest (D-055; S4 plan §4.4). Sent when the hold starts.
    ConvoyHeld {
        /// The fleet.
        fleet: FleetId,
        /// The contested gate.
        gate: crate::galaxy::GateId,
    },
    /// A convoy route ended: replaced by an order, its fleet gone, or a stop lost.
    ConvoyEnded {
        /// The fleet.
        fleet: FleetId,
    },
    /// The REPAIR ability turned itself off: nothing left to repair.
    RepairDone {
        /// The holding.
        holding: HoldingId,
    },
    /// A MULE was destroyed with cargo aboard; the cargo, and a core kit if it carried one,
    /// are lost (D-064).
    CargoLost {
        /// The ship that is gone.
        ship: ShipId,
        /// What it carried.
        amount: Stock,
        /// Whether it carried a core kit.
        core_kit: bool,
    },
}

/// The richness multiplier in per mille (D-066).
pub(crate) const fn richness_permille(richness: Richness) -> u64 {
    match richness {
        Richness::Poor => 500,
        Richness::Standard => 1_000,
        Richness::Rich => 1_500,
    }
}

/// The holding's economy slot within a second: 0–9 (doc 11 §4.1).
pub(crate) fn economy_slot(holding: HoldingId) -> u64 {
    holding.sim_id().get() % TICKS_PER_SECOND
}

/// A module as seen from outside (S4 plan §4.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleView {
    /// Its identity.
    pub id: SimId,
    /// Its holding.
    pub holding: HoldingId,
    /// Its slot.
    pub slot: Slot,
    /// What it is.
    pub kind: ModuleKind,
    /// Hit points.
    pub hp: u32,
    /// Maximum hit points.
    pub max_hp: u32,
    /// Build time left in milliseconds, while it is being built.
    pub building_ms: Option<u64>,
    /// Whether it is knocked offline.
    pub disabled: bool,
}

/// A queue item as seen from outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueItemView {
    /// Its identity, for [`nomad_types::EconomyCommand::Cancel`].
    pub id: SimId,
    /// What it makes.
    pub item: BuildItem,
    /// What it costs, fixed when it was queued (D-189 for core kits).
    pub price: Stock,
    /// Its lane and build time left, once started.
    pub building: Option<(BuildLane, u64)>,
    /// Why it last could not start.
    pub short: Option<Shortage>,
}

/// One holding's economy as seen from outside (S4 plan §4.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldingEconomyView {
    /// The holding.
    pub holding: HoldingId,
    /// Every slot of the template, with its module if any.
    pub slots: Vec<(Slot, Option<ModuleView>)>,
    /// The queue, in order.
    pub queue: Vec<QueueItemView>,
    /// The power report.
    pub power: PowerAllocation,
    /// The power profile.
    pub profile: PowerProfile,
    /// The local buffer (zero at holdings that deliver straight to the pool).
    pub buffer: Stock,
    /// The local buffer's capacity.
    pub buffer_cap: Stock,
    /// Ore waiting for a refinery, milli-units.
    pub ore: u64,
    /// Whether it is a depot (D-064).
    pub depot: bool,
    /// Whether its production goes straight to the pool.
    pub delivers_to_pool: bool,
    /// Finished core kits stored here.
    pub kits: u32,
    /// Whether REPAIR is on.
    pub repairing: bool,
    /// Which resources it produces for D-217 and D-281: those its completed producers give it
    /// a nominal output of.
    pub producing: PerResource<bool>,
    /// D-281's factors on its alloy and crystal output, per mille: 1,000 at the mother
    /// station, otherwise its owner's [`CommanderEconomy::yield_permille`].
    pub yield_permille: PerResource<u32>,
}

/// A commander's economy as seen from outside (S4 plan §4.8). S7 wraps it into the
/// commander's view; the Deputy and the Strategist read only that (D-077).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommanderEconomy {
    /// The pool.
    pub pool: Stock,
    /// The pool cap (D-025).
    pub cap: Stock,
    /// Production over the last hour.
    pub income_last_hour: Stock,
    /// Everything produced since the season start.
    pub produced: Stock,
    /// Everything lost to full storage.
    pub lost: Stock,
    /// Command cost of ships and ships being built.
    pub command_used: u32,
    /// The command cap (D-129).
    pub command_cap: u32,
    /// What a core kit queued now would cost (D-189).
    pub core_kit_price: Stock,
    /// Holdings other than the mother station that produce alloy and crystal: D-281's n per
    /// resource.
    pub producing_holdings: PerResource<u32>,
    /// D-281's diminishing yield on those holdings' alloy and crystal output, per mille,
    /// rounded down: min(1, F ÷ n) per resource. The simulation applies the exact fraction.
    pub yield_permille: PerResource<u32>,
    /// Every holding and outpost, in identity order.
    pub holdings: Vec<HoldingEconomyView>,
}

impl UniverseState {
    /// A commander's economy (S4 plan §4.8).
    pub fn economy_of(&self, commander: CommanderId) -> CommanderEconomy {
        let stats = self.economy.stats.get(&commander);
        let holdings = self
            .holdings
            .iter()
            .filter(|(_, h)| h.owner == commander)
            .filter_map(|(&id, _)| self.holding_economy(id))
            .collect();
        CommanderEconomy {
            pool: self
                .economy
                .pools
                .get(&commander)
                .copied()
                .unwrap_or_default(),
            cap: self.pool_cap(commander),
            income_last_hour: stats.map_or(Stock::ZERO, |s| {
                s.minutes
                    .iter()
                    .fold(Stock::ZERO, |sum, &(_, amount)| sum.plus(amount))
            }),
            produced: stats.map_or(Stock::ZERO, |s| s.produced),
            lost: stats.map_or(Stock::ZERO, |s| s.lost),
            command_used: self.command_used(commander),
            command_cap: self.command_cap(commander),
            core_kit_price: self.core_kit_price(commander),
            producing_holdings: self.producing_holdings(commander),
            yield_permille: self.yield_permille(commander),
            holdings,
        }
    }

    /// A commander's holdings other than the mother station that produce alloy and crystal:
    /// D-281's n per resource.
    pub fn producing_holdings(&self, commander: CommanderId) -> PerResource<u32> {
        let mut count = PerResource::<u32>::default();
        for (&id, h) in &self.holdings {
            if h.owner != commander || h.mother_station {
                continue;
            }
            let produces = tick::produces(self, id);
            count.alloy += u32::from(produces.alloy);
            count.crystal += u32::from(produces.crystal);
        }
        count
    }

    /// D-281's factors on the alloy and crystal output of a commander's holdings other than
    /// the mother station, per mille, rounded down: min(1, F ÷ n) per resource.
    pub fn yield_permille(&self, commander: CommanderId) -> PerResource<u32> {
        let f = self.rules.economy.diminishing_yield_permille;
        let n = self.producing_holdings(commander);
        PerResource {
            alloy: rules::yield_permille(f, u64::from(n.alloy)),
            crystal: rules::yield_permille(f, u64::from(n.crystal)),
        }
    }

    /// One holding's economy, or `None` if it does not exist.
    pub fn holding_economy(&self, holding: HoldingId) -> Option<HoldingEconomyView> {
        let h = self.holdings.get(&holding)?;
        let economy = self.economy.holdings.get(&holding);
        let slots = self
            .slot_list(holding)
            .into_iter()
            .map(|slot| {
                let module = economy
                    .and_then(|e| e.slots.get(&slot))
                    .and_then(|&id| self.module(id));
                (slot, module)
            })
            .collect();
        let queue = economy.map_or_else(Vec::new, |e| {
            e.queue
                .iter()
                .map(|entry| QueueItemView {
                    id: entry.id,
                    item: entry.item,
                    price: entry.price,
                    building: match entry.state {
                        EntryState::Waiting => None,
                        EntryState::Building {
                            lane, remaining_ms, ..
                        } => Some((lane, remaining_ms)),
                    },
                    short: entry.short,
                })
                .collect()
        });
        Some(HoldingEconomyView {
            holding,
            slots,
            queue,
            power: economy.map(|e| e.allocation.clone()).unwrap_or_default(),
            profile: economy.map(|e| e.profile).unwrap_or_default(),
            buffer: economy.map_or(Stock::ZERO, |e| e.buffer),
            buffer_cap: self.buffer_cap(holding),
            ore: economy.map_or(0, |e| e.ore),
            depot: self.is_depot(holding),
            delivers_to_pool: self.delivers_to_pool(holding),
            kits: economy.map_or(0, |e| e.kits),
            repairing: economy.is_some_and(|e| e.repairing) && h.core_complete,
            producing: tick::produces(self, holding),
            yield_permille: if h.mother_station {
                let full = u32::try_from(MILLI).unwrap_or(u32::MAX);
                PerResource {
                    alloy: full,
                    crystal: full,
                }
            } else {
                self.yield_permille(h.owner)
            },
        })
    }

    /// A module, or `None`.
    pub fn module(&self, id: SimId) -> Option<ModuleView> {
        let module = self.entities.get::<Module>(id)?;
        let hull = self.entities.get::<Hull>(id)?;
        Some(ModuleView {
            id,
            holding: module.holding,
            slot: module.slot,
            kind: module.kind,
            hp: hull.hp,
            max_hp: hull.max,
            building_ms: self
                .entities
                .get::<Construction>(id)
                .map(|c| c.remaining_ms),
            disabled: self.entities.has::<Disabled>(id),
        })
    }

    /// Every module of `holding`, in slot order.
    pub fn modules_at(&self, holding: HoldingId) -> Vec<ModuleView> {
        self.economy
            .holdings
            .get(&holding)
            .map_or_else(Vec::new, |e| {
                e.slots.values().filter_map(|&id| self.module(id)).collect()
            })
    }

    /// The last power allocation of `holding` (S4/S5 interface).
    pub fn power(&self, holding: HoldingId) -> Option<&PowerAllocation> {
        self.economy.holdings.get(&holding).map(|e| &e.allocation)
    }

    /// Where module `id` is, for hit tests (S4/S5 interface): its system, its centre in the
    /// system frame and its hit radius in metres. Slots are template positions around the
    /// holding's site (D-130), turned into the system frame like the phase-1 station.
    pub fn module_position(
        &self,
        id: SimId,
    ) -> Option<(crate::galaxy::SystemId, crate::PositionMetres, f64)> {
        let module = self.entities.get::<Module>(id)?;
        let site = self.galaxy.site(self.holdings.get(&module.holding)?.site)?;
        let template = &self.rules.economy.slots;
        let grid = slot_position(template, module.slot)?;
        let layout = &self.rules.mother_station;
        let offset =
            crate::fleet::station::grid_to_system(layout, [f64::from(grid[0]), f64::from(grid[1])]);
        let r = &template.radius_voxels;
        let radius = match module.slot {
            Slot::Core => r.core,
            Slot::Pylon(_) => r.pylon,
            Slot::Inner(_) => r.inner,
            Slot::Dock => r.dock,
            Slot::Solar => r.solar,
            Slot::Deposit(_) => r.deposit,
        };
        let position = crate::geometry::Vec2::from(site.position) + offset;
        Some((
            site.system,
            position.into(),
            f64::from(radius) * layout.voxel_m,
        ))
    }

    /// Which shield sectors have a completed, working pylon, in sector order (S4/S5
    /// interface; D-140: a missing pylon leaves its sector at 25%).
    pub fn pylons_per_sector(&self, holding: HoldingId) -> [bool; 4] {
        let mut sectors = [false; 4];
        for (i, sector) in sectors.iter_mut().enumerate() {
            let slot = Slot::Pylon(u8::try_from(i).unwrap_or(u8::MAX));
            *sector = self.working_module_in(holding, slot, ModuleKind::Pylon);
        }
        sectors
    }

    /// A commander's pool, in milli-units.
    pub fn pool(&self, commander: CommanderId) -> Stock {
        self.economy
            .pools
            .get(&commander)
            .copied()
            .unwrap_or_default()
    }

    /// What a MULE carries.
    pub fn cargo(&self, ship: ShipId) -> Cargo {
        self.economy.cargo.get(&ship).copied().unwrap_or_default()
    }

    /// The pool cap: the mother station's base plus each completed storage module (D-025).
    /// A fallen mother station in its emergency jump keeps the base (S6b).
    pub fn pool_cap(&self, commander: CommanderId) -> Stock {
        let storage = &self.rules.economy.storage;
        let has_mother = self
            .holdings
            .values()
            .any(|h| h.owner == commander && h.mother_station && h.core_complete)
            || crate::siege::mother_in_transit(self, commander);
        let base = if has_mother {
            Stock::units(storage.base_alloy, storage.base_crystal)
        } else {
            Stock::ZERO
        };
        let racks = self
            .holdings
            .iter()
            .filter(|(_, h)| h.owner == commander)
            .map(|(&id, _)| self.completed_count(id, ModuleKind::Storage))
            .sum::<u64>();
        let rack = Stock::units(storage.rack_alloy, storage.rack_crystal);
        base.plus(Stock {
            alloy: rack.alloy.saturating_mul(racks),
            crystal: rack.crystal.saturating_mul(racks),
        })
    }

    /// The command cap: the base, plus each other completed holding, up to the maximum
    /// (D-129). A commander without a mother station has no cap. A fallen mother station in
    /// its emergency jump stays the commander's mother station (S6 plan §4.6).
    pub fn command_cap(&self, commander: CommanderId) -> u32 {
        let cap = &self.rules.economy.command_cap;
        let mut has_mother = crate::siege::mother_in_transit(self, commander);
        let mut others = 0_u32;
        for h in self
            .holdings
            .values()
            .filter(|h| h.owner == commander && h.core_complete)
        {
            if h.mother_station {
                has_mother = true;
            } else {
                others = others.saturating_add(1);
            }
        }
        if !has_mother {
            return 0;
        }
        cap.base
            .saturating_add(cap.per_holding.saturating_mul(others))
            .min(cap.max)
    }

    /// Command cost of a commander's ships and of the ships they are building (doc 14 §8).
    pub fn command_used(&self, commander: CommanderId) -> u32 {
        let owner = nomad_types::Owner::Commander(commander);
        let ships = self
            .entities
            .ids_with::<Ship>()
            .into_iter()
            .filter_map(|id| self.entities.get::<Ship>(id))
            .filter(|s| s.owner == owner)
            .map(|s| self.ship_command(s.class))
            .fold(0_u32, u32::saturating_add);
        let building = self
            .holdings
            .iter()
            .filter(|(_, h)| h.owner == commander)
            .filter_map(|(id, _)| self.economy.holdings.get(id))
            .flat_map(|e| e.queue.iter())
            .filter_map(|entry| match (entry.item, entry.state) {
                (BuildItem::Ship { class }, EntryState::Building { .. }) => {
                    Some(self.ship_command(class))
                }
                _ => None,
            })
            .fold(0_u32, u32::saturating_add);
        ships.saturating_add(building)
    }

    /// n of D-189: the commander's holdings beyond the mother station, outposts under
    /// construction included, plus the core kits not yet deployed: in a build queue (waiting
    /// or being built), stored at a holding or carried by a MULE. Counting queued kits keeps
    /// several kits queued at once from sharing the low price. A kit that is cancelled, or
    /// lost with its MULE, no longer counts.
    pub fn expansion_count(&self, commander: CommanderId) -> u64 {
        let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        let mut n = 0_u64;
        for (id, h) in self.holdings.iter().filter(|(_, h)| h.owner == commander) {
            if !h.mother_station {
                n = n.saturating_add(1);
            }
            if let Some(e) = self.economy.holdings.get(id) {
                let queued = e
                    .queue
                    .iter()
                    .filter(|entry| entry.item == BuildItem::CoreKit)
                    .count();
                n = n
                    .saturating_add(count(queued))
                    .saturating_add(u64::from(e.kits));
            }
        }
        let owner = nomad_types::Owner::Commander(commander);
        let carried = self
            .economy
            .cargo
            .iter()
            .filter(|(ship, cargo)| {
                cargo.core_kit
                    && self
                        .entities
                        .with::<Ship, _>(ship.sim_id(), |s| s.owner == owner)
                        .unwrap_or(false)
            })
            .count();
        n.saturating_add(count(carried))
    }

    /// What a core kit `commander` queues now would cost (D-189). The queue keeps this price
    /// until the kit is paid for.
    pub fn core_kit_price(&self, commander: CommanderId) -> Stock {
        let economy = &self.rules.economy;
        rules::core_kit_price(
            economy.core_kit,
            economy.core_kit_growth_permille,
            self.expansion_count(commander),
        )
    }

    /// Completed dry docks at `holding`, for docking room and frigates (D-102).
    pub fn dry_docks(&self, holding: HoldingId) -> u32 {
        u32::try_from(self.completed_count(holding, ModuleKind::DryDock)).unwrap_or(u32::MAX)
    }

    /// Whether `holding` is a depot: the mother station, or a holding with a completed
    /// storage module (D-064).
    pub fn is_depot(&self, holding: HoldingId) -> bool {
        let Some(h) = self.holdings.get(&holding) else {
            return false;
        };
        h.core_complete
            && (h.mother_station || self.completed_count(holding, ModuleKind::Storage) > 0)
    }

    /// Whether `holding`'s production goes straight to the pool: it is in the same system as
    /// one of its owner's depots (D-064).
    pub fn delivers_to_pool(&self, holding: HoldingId) -> bool {
        let Some(h) = self.holdings.get(&holding) else {
            return false;
        };
        let Some(system) = self.galaxy.site(h.site).map(|s| s.system) else {
            return false;
        };
        self.holdings.iter().any(|(&id, other)| {
            other.owner == h.owner
                && self
                    .galaxy
                    .site(other.site)
                    .is_some_and(|s| s.system == system)
                && self.is_depot(id)
        })
    }

    /// The local buffer's capacity: 2 h of the holding's nominal production (D-064), after
    /// D-281's per-resource diminishing yield (*Derived:* the buffer holds 2 h of that
    /// holding's production, doc 14 §4, and D-217 scales the production).
    pub fn buffer_cap(&self, holding: HoldingId) -> Stock {
        self.buffer_cap_at(holding, self.holding_yield_factor(holding))
    }

    /// [`UniverseState::buffer_cap`] with D-281's factors already known.
    pub(crate) fn buffer_cap_at(&self, holding: HoldingId, factor: rules::YieldFactors) -> Stock {
        let hours = self.rules.economy.storage.buffer_h;
        let (alloy, crystal) = tick::nominal_output_per_h(self, holding);
        Stock {
            alloy: rules::scaled(alloy, factor.alloy).saturating_mul(hours),
            crystal: rules::scaled(crystal, factor.crystal).saturating_mul(hours),
        }
    }

    /// D-281's factors on `holding`'s output: none at the mother station.
    fn holding_yield_factor(&self, holding: HoldingId) -> rules::YieldFactors {
        match self.holdings.get(&holding) {
            Some(h) if !h.mother_station => {
                let n = self.producing_holdings(h.owner);
                rules::YieldFactors::new(
                    self.rules.economy.diminishing_yield_permille,
                    PerResource {
                        alloy: u64::from(n.alloy),
                        crystal: u64::from(n.crystal),
                    },
                )
            }
            _ => rules::YieldFactors::FULL,
        }
    }

    /// The slots `holding` has under the template, in order (D-130, D-131).
    pub fn slot_list(&self, holding: HoldingId) -> Vec<Slot> {
        let Some(h) = self.holdings.get(&holding) else {
            return Vec::new();
        };
        let template = &self.rules.economy.slots;
        let inner = if h.mother_station {
            template.mother_inner
        } else {
            template.other_inner
        };
        let deposits = self
            .galaxy
            .site(h.site)
            .map_or(0, |site| site.deposits.len().min(template.deposits.len()));
        let mut slots = vec![Slot::Core];
        slots.extend((0..4).map(Slot::Pylon));
        slots.extend((0..inner).filter_map(|i| u8::try_from(i).ok().map(Slot::Inner)));
        slots.push(Slot::Dock);
        slots.push(Slot::Solar);
        slots.extend((0..deposits).filter_map(|i| u8::try_from(i).ok().map(Slot::Deposit)));
        slots
    }

    /// Commander `n`'s ships of a class's command cost, 0 for classes that cannot be built.
    fn ship_command(&self, class: crate::fleet::ShipClass) -> u32 {
        self.rules.economy.ship(class).map_or(0, |s| s.command)
    }

    /// Completed modules of `kind` at `holding`, disabled ones included.
    pub(crate) fn completed_count(&self, holding: HoldingId, kind: ModuleKind) -> u64 {
        self.economy
            .holdings
            .get(&holding)
            .map_or(0, |e| {
                e.slots
                    .values()
                    .filter(|&&id| {
                        self.entities
                            .with::<Module, _>(id, |m| m.kind == kind)
                            .unwrap_or(false)
                            && !self.entities.has::<Construction>(id)
                    })
                    .count()
            })
            .try_into()
            .unwrap_or(u64::MAX)
    }

    /// Whether `slot` of `holding` has a completed, enabled module of `kind`.
    pub(crate) fn working_module_in(
        &self,
        holding: HoldingId,
        slot: Slot,
        kind: ModuleKind,
    ) -> bool {
        self.economy
            .holdings
            .get(&holding)
            .and_then(|e| e.slots.get(&slot))
            .is_some_and(|&id| self.is_working(id) && self.module_kind(id) == Some(kind))
    }

    /// Whether module `id` is complete and not disabled.
    pub(crate) fn is_working(&self, id: SimId) -> bool {
        self.entities.has::<Module>(id)
            && !self.entities.has::<Construction>(id)
            && !self.entities.has::<Disabled>(id)
    }

    pub(crate) fn module_kind(&self, id: SimId) -> Option<ModuleKind> {
        self.entities.with::<Module, _>(id, |m| m.kind)
    }

    /// Checks the economy for broken invariants and returns what is wrong, if anything:
    /// slot maps against module entities, and cargo of ships that are gone.
    pub fn economy_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (&holding, e) in &self.economy.holdings {
            for (&slot, &id) in &e.slots {
                match self.entities.get::<Module>(id) {
                    Some(m) if m.holding == holding && m.slot == slot => {}
                    other => problems.push(format!(
                        "holding {holding:?} slot {slot:?} maps to {id:?}, which is {other:?}"
                    )),
                }
            }
        }
        for id in self.entities.ids_with::<Module>() {
            if let Some(m) = self.entities.get::<Module>(id)
                && self
                    .economy
                    .holdings
                    .get(&m.holding)
                    .and_then(|e| e.slots.get(&m.slot))
                    != Some(&id)
            {
                problems.push(format!("module {id:?} is not in its holding's slot map"));
            }
        }
        for ship in self.economy.cargo.keys() {
            if !self.entities.has::<Ship>(ship.sim_id()) {
                problems.push(format!("cargo of {ship:?}, which is gone"));
            }
        }
        problems
    }
}

/// The slot's position in grid voxels, origin at the core (D-130).
pub(crate) fn slot_position(template: &SlotTemplate, slot: Slot) -> Option<[i32; 2]> {
    match slot {
        Slot::Core => Some([0, 0]),
        Slot::Pylon(i) => template.pylons.get(usize::from(i)).copied(),
        Slot::Inner(i) => template.inner.get(usize::from(i)).copied(),
        Slot::Dock => Some(template.dock),
        Slot::Solar => Some(template.solar),
        Slot::Deposit(i) => template.deposits.get(usize::from(i)).copied(),
    }
}

/// Which module kinds may go into `slot`, given the site's deposit there (D-130, D-131).
pub(crate) fn fits(kind: ModuleKind, slot: Slot, deposit: Option<DepositKind>) -> bool {
    match (slot, kind) {
        (Slot::Pylon(_), ModuleKind::Pylon)
        | (Slot::Dock, ModuleKind::DryDock)
        | (Slot::Solar, ModuleKind::Solar)
        | (Slot::Inner(_), ModuleKind::Refinery | ModuleKind::Storage | ModuleKind::Turret) => true,
        (Slot::Deposit(_), ModuleKind::Harvester) => deposit == Some(DepositKind::OreField),
        (Slot::Deposit(_), ModuleKind::Extractor) => deposit == Some(DepositKind::CrystalVein),
        _ => false,
    }
}

/// Spawns a module entity in `slot` of `holding` and records it in the slot map. Under
/// construction if `build_ms` is some.
pub(crate) fn spawn_module(
    state: &mut UniverseState,
    holding: HoldingId,
    slot: Slot,
    kind: ModuleKind,
    build_ms: Option<u64>,
) -> SimId {
    let max = module_hull(state, holding, kind);
    let id = state.ids.allocate();
    let hull = Hull {
        hp: max,
        max,
        last_damage: None,
    };
    let module = Module {
        holding,
        slot,
        kind,
    };
    match build_ms {
        Some(remaining_ms) => state
            .entities
            .spawn(id, (module, hull, Construction { remaining_ms })),
        None => state.entities.spawn(id, (module, hull)),
    }
    state
        .economy
        .holdings
        .entry(holding)
        .or_default()
        .slots
        .insert(slot, id);
    id
}

/// Spawns the core of the outpost `holding`, under construction for the deployment time and
/// at the start share of its hull, which grows linearly to the full hull as the deployment
/// runs (D-054, D-224). For a core kit's claim (S4) and for the outpost a fallen mother
/// station's site becomes (D-057, S6b).
pub(crate) fn spawn_outpost_core(state: &mut UniverseState, holding: HoldingId) -> SimId {
    let core = state.rules.economy.core;
    let deploy_ms = core.deploy_s.saturating_mul(MILLI);
    let id = spawn_module(
        state,
        holding,
        Slot::Core,
        ModuleKind::Core,
        Some(deploy_ms),
    );
    if let Some(hull) = state.entities.get_mut::<Hull>(id) {
        hull.hp = deploy_hull(hull.max, core.deploy_start_hull_percent, deploy_ms, 0);
    }
    id
}

/// The hull an outpost's core has grown to `elapsed_ms` into a deployment of `total_ms`:
/// linearly from `start_percent` of `max` to `max` (D-224). Integer and exact, so a second's
/// growth is the difference of two calls (D-049).
pub(crate) fn deploy_hull(max: u32, start_percent: u32, total_ms: u64, elapsed_ms: u64) -> u32 {
    let total = u128::from(total_ms.max(1));
    let elapsed = u128::from(elapsed_ms).min(total);
    let start = u128::from(start_percent.min(100));
    let share = start * total + (100 - start) * elapsed;
    u32::try_from(u128::from(max) * share / (100 * total)).unwrap_or(max)
}

/// A module's maximum hull (doc 14 §9.3).
pub(crate) fn module_hull(state: &UniverseState, holding: HoldingId, kind: ModuleKind) -> u32 {
    let economy = &state.rules.economy;
    match kind {
        ModuleKind::Core => {
            if state
                .holdings
                .get(&holding)
                .is_some_and(|h| h.mother_station)
            {
                economy.core.mother_hull
            } else {
                economy.core.hull
            }
        }
        other => economy.module(other).hull,
    }
}

/// Builds every mother station's start kit: the core and the modules of D-112, complete,
/// and the starting stock (doc 14 §12). Identities follow holding order, then slot order.
pub(crate) fn spawn_start_stations(state: &mut UniverseState) {
    let mothers: Vec<(HoldingId, CommanderId)> = state
        .holdings
        .iter()
        .filter(|(_, h)| h.mother_station)
        .map(|(&id, h)| (id, h.owner))
        .collect();
    let start = state.rules.economy.start.clone();
    for (holding, owner) in mothers {
        spawn_module(state, holding, Slot::Core, ModuleKind::Core, None);
        for module in &start.modules {
            spawn_module(state, holding, module.slot, module.kind, None);
        }
        state
            .economy
            .pools
            .insert(owner, Stock::units(start.alloy, start.crystal));
    }
}

/// Records that module `id` took damage now, which holds back its free repair (D-071), and
/// lowers its hull by `amount`. Returns the hit points left; at zero, S5b decides between
/// disabled and destroyed (doc 12 §3, [`crate::defence::hooks`]) and calls
/// [`disable_module`] or [`destroy_module`].
pub(crate) fn damage_module(
    state: &mut UniverseState,
    id: SimId,
    amount: u32,
    now: Tick,
) -> Option<u32> {
    let hull = state.entities.get_mut::<Hull>(id)?;
    hull.hp = hull.hp.saturating_sub(amount);
    hull.last_damage = Some(now);
    Some(hull.hp)
}

/// Knocks module `id` offline (doc 12 §3).
pub(crate) fn disable_module(
    state: &mut UniverseState,
    id: SimId,
    now: Tick,
    events: &mut Vec<EconomyEvent>,
) {
    if state.entities.has::<Module>(id) && !state.entities.has::<Disabled>(id) {
        state.entities.insert(id, Disabled { since: now });
        events.push(EconomyEvent::ModuleDisabled { module: id });
    }
}

/// Brings a disabled module back online (D-149 decides when; S5b calls this).
pub(crate) fn restore_module(state: &mut UniverseState, id: SimId, events: &mut Vec<EconomyEvent>) {
    if state.entities.remove::<Disabled>(id).is_some() {
        events.push(EconomyEvent::ModuleRestored { module: id });
    }
}

/// Destroys module `id` and frees its slot. A module being built is removed from the queue
/// without a refund. Destroying a core is S6's capture rule and is refused here.
pub(crate) fn destroy_module(
    state: &mut UniverseState,
    id: SimId,
    events: &mut Vec<EconomyEvent>,
) -> bool {
    let Some(module) = state.entities.get::<Module>(id) else {
        return false;
    };
    if module.kind == ModuleKind::Core {
        return false;
    }
    if let Some(e) = state.economy.holdings.get_mut(&module.holding) {
        e.slots.remove(&module.slot);
        e.queue.retain(|entry| {
            !matches!(
                entry.state,
                EntryState::Building {
                    module: Some(m), ..
                } if m == id
            ) && !matches!(entry.state, EntryState::Building { lane: BuildLane::Dock(d), .. } if d == id)
        });
    }
    state.entities.despawn(id);
    events.push(EconomyEvent::ModuleDestroyed {
        module: id,
        holding: module.holding,
        kind: module.kind,
        slot: module.slot,
    });
    true
}
