//! Build queues and construction (D-068; S4 plan §4.7).
//!
//! - **One queue per holding,** plus one parallel ship lane per working dry dock. Modules
//!   and core kits build in the main lane; small craft in the main lane or a free dock lane;
//!   frigates only in a dock lane (D-068: frigates need a dry dock). *Derived.*
//! - **Payment in full when an item starts,** at the price fixed when it was queued. Only a
//!   core kit's price depends on when: it grows with the commander's size (D-189). An item
//!   the pool cannot pay for, or a ship that would exceed the command cap (D-129), waits and
//!   does not block later items.
//! - **Cancelling** refunds everything before the start (nothing was paid) and half after.
//! - **Progress** is counted in the holding's economy second: the main lane at full speed,
//!   a dock lane at the fraction of its 150 MW it is allocated (doc 14 §7). *Derived:* the
//!   plan's deadline timer cannot follow a power-limited dock, so all lanes use counters.

use std::collections::{BTreeMap, VecDeque};

use nomad_types::{BuildItem, ModuleKind, Owner, ShipClass, Slot};

use super::power::Consumer;
use super::rules::{Cost, MILLI};
use super::{
    BuildLane, Construction, EconomyEvent, EntryState, QueueEntry, Shortage, Stock, spawn_module,
};
use crate::fleet::{
    DEFAULT_RULES_OF_ENGAGEMENT, Fleet, FleetId, Motion, Place, Ship, ShipId, reform,
    slot_position, station,
};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::{CommanderId, SimId};
use crate::state::UniverseState;
use crate::time::Tick;

/// The rules entry of `item`: its base cost and build time (doc 14 §9.3–§9.4).
fn rules_cost(state: &UniverseState, item: BuildItem) -> Option<Cost> {
    let economy = &state.rules.economy;
    match item {
        BuildItem::Module { kind, .. } if kind != ModuleKind::Core => {
            Some(economy.module(kind).cost)
        }
        BuildItem::Module { .. } => None,
        BuildItem::Ship { class } => economy.ship(class).map(|s| s.cost),
        BuildItem::CoreKit => Some(economy.core_kit),
    }
}

/// How long `item` takes to build, in milliseconds.
fn build_ms(state: &UniverseState, item: BuildItem) -> Option<u64> {
    rules_cost(state, item).map(|c| c.build_s.saturating_mul(MILLI))
}

/// What `item` costs if it is queued at `holding` now, in milli-units: the rules cost, or
/// for a core kit the price that grows with the owner's size (D-189).
pub(crate) fn price(state: &UniverseState, holding: HoldingId, item: BuildItem) -> Option<Stock> {
    match item {
        BuildItem::CoreKit => {
            let owner = state.holdings.get(&holding)?.owner;
            Some(state.core_kit_price(owner))
        }
        other => rules_cost(state, other).map(|c| Stock::units(c.alloy, c.crystal)),
    }
}

/// Adds an item to `holding`'s queue (validated), with its price fixed now (D-189): the kit
/// is priced before it joins the queue, so it does not count towards its own price.
pub(crate) fn enqueue(
    state: &mut UniverseState,
    holding: HoldingId,
    item: BuildItem,
    events: &mut Vec<EconomyEvent>,
) {
    let Some(price) = price(state, holding, item) else {
        return;
    };
    let id = state.ids.allocate();
    state
        .economy
        .holdings
        .entry(holding)
        .or_default()
        .queue
        .push(QueueEntry {
            id,
            item,
            price,
            state: EntryState::Waiting,
            short: None,
        });
    events.push(EconomyEvent::ItemQueued { holding, item: id });
}

/// Cancels queue item `item` at `holding` (validated): no refund is owed before it starts,
/// half of what was paid after (D-068). A module being built is removed.
pub(crate) fn cancel(
    state: &mut UniverseState,
    holding: HoldingId,
    item: SimId,
    events: &mut Vec<EconomyEvent>,
) {
    let Some(owner) = state.holdings.get(&holding).map(|h| h.owner) else {
        return;
    };
    let Some(e) = state.economy.holdings.get_mut(&holding) else {
        return;
    };
    let Some(index) = e.queue.iter().position(|entry| entry.id == item) else {
        return;
    };
    let entry = e.queue.remove(index);
    let mut refund = Stock::ZERO;
    if let EntryState::Building { paid, module, .. } = entry.state {
        refund = paid.half();
        if let Some(module) = module {
            if let Some(m) = state.entities.get::<super::Module>(module)
                && let Some(e) = state.economy.holdings.get_mut(&holding)
            {
                e.slots.remove(&m.slot);
            }
            state.entities.despawn(module);
        }
    }
    let kept = credit_pool(state, owner, refund, events);
    events.push(EconomyEvent::ItemCancelled {
        holding,
        item,
        refund: kept,
    });
}

/// Adds `amount` to `owner`'s pool up to its cap; the rest is lost and counted (D-025).
/// Returns what was kept.
pub(crate) fn credit_pool(
    state: &mut UniverseState,
    owner: CommanderId,
    amount: Stock,
    events: &mut Vec<EconomyEvent>,
) -> Stock {
    if amount.is_zero() {
        return Stock::ZERO;
    }
    let cap = state.pool_cap(owner);
    let pool = state.economy.pools.entry(owner).or_default();
    let room = cap.minus(*pool);
    let (kept, lost) = amount.clip(room);
    *pool = pool.plus(kept);
    if !lost.is_zero() {
        let stats = state.economy.stats.entry(owner).or_default();
        stats.lost = stats.lost.plus(lost);
        events.push(EconomyEvent::PoolOverflow {
            commander: owner,
            lost,
        });
    }
    kept
}

/// Advances every item that is building at `holding` by one second, and an outpost's core
/// deployment, and completes those that are done.
pub(crate) fn progress(
    state: &mut UniverseState,
    holding: HoldingId,
    now: Tick,
    events: &mut Vec<EconomyEvent>,
    fleet_events: &mut Vec<crate::fleet::FleetEvent>,
) {
    progress_core(state, holding, events);
    let Some(e) = state.economy.holdings.get(&holding) else {
        return;
    };
    let allocation = e.allocation.clone();
    let working_docks: Vec<SimId> = e
        .slots
        .values()
        .copied()
        .filter(|&id| state.is_working(id) && state.module_kind(id) == Some(ModuleKind::DryDock))
        .collect();
    let mut done = Vec::new();
    let Some(e) = state.economy.holdings.get_mut(&holding) else {
        return;
    };
    for entry in &mut e.queue {
        let EntryState::Building {
            lane,
            ref mut remaining_ms,
            ..
        } = entry.state
        else {
            continue;
        };
        let step = match lane {
            BuildLane::Main => MILLI,
            BuildLane::Dock(dock) if working_docks.contains(&dock) => {
                match allocation.share(Consumer::Module(dock)) {
                    Some((allocated, demand)) if demand > 0 => {
                        MILLI.saturating_mul(allocated.min(demand)) / demand
                    }
                    // The dock started this second: the next allocation counts it.
                    _ => MILLI,
                }
            }
            BuildLane::Dock(_) => 0,
        };
        *remaining_ms = remaining_ms.saturating_sub(step);
        if *remaining_ms == 0 {
            done.push(entry.id);
        }
    }
    for id in done {
        complete(state, holding, id, now, events, fleet_events);
    }
}

/// Counts down an outpost's core deployment, grows its hull by the second's share (D-224:
/// from 10% to 100% over the deployment; damage taken stays taken), and completes the
/// holding (D-054, D-135).
fn progress_core(state: &mut UniverseState, holding: HoldingId, events: &mut Vec<EconomyEvent>) {
    let Some(core) = state
        .economy
        .holdings
        .get(&holding)
        .and_then(|e| e.slots.get(&Slot::Core).copied())
    else {
        return;
    };
    let rules = state.rules.economy.core;
    let total_ms = rules.deploy_s.saturating_mul(MILLI);
    let Some(c) = state.entities.get_mut::<Construction>(core) else {
        return;
    };
    let before = c.remaining_ms;
    c.remaining_ms = c.remaining_ms.saturating_sub(MILLI);
    let remaining = c.remaining_ms;
    if let Some(hull) = state.entities.get_mut::<super::Hull>(core) {
        let grown = |left: u64| {
            super::deploy_hull(
                hull.max,
                rules.deploy_start_hull_percent,
                total_ms,
                total_ms.saturating_sub(left),
            )
        };
        let growth = grown(remaining).saturating_sub(grown(before));
        hull.hp = hull.hp.saturating_add(growth).min(hull.max);
    }
    if remaining == 0 {
        state.entities.remove::<Construction>(core);
        if let Some(h) = state.holdings.get_mut(&holding) {
            h.core_complete = true;
        }
        // The new holding counts for control and the standings (D-041, D-191).
        crate::season::mark_dirty(state);
        events.push(EconomyEvent::HoldingCompleted { holding });
    }
}

fn complete(
    state: &mut UniverseState,
    holding: HoldingId,
    item: SimId,
    now: Tick,
    events: &mut Vec<EconomyEvent>,
    fleet_events: &mut Vec<crate::fleet::FleetEvent>,
) {
    let Some(e) = state.economy.holdings.get_mut(&holding) else {
        return;
    };
    let Some(index) = e.queue.iter().position(|entry| entry.id == item) else {
        return;
    };
    let entry = e.queue.remove(index);
    match (entry.item, entry.state) {
        (
            BuildItem::Module { .. },
            EntryState::Building {
                module: Some(module),
                ..
            },
        ) => {
            state.entities.remove::<Construction>(module);
        }
        (BuildItem::CoreKit, _) => e.kits = e.kits.saturating_add(1),
        (BuildItem::Ship { class }, _) => {
            if let Some(owner) = state.holdings.get(&holding).map(|h| h.owner) {
                let (ship, fleet) = spawn_ship(state, holding, owner, class, now, fleet_events);
                events.push(EconomyEvent::ShipBuilt {
                    holding,
                    ship,
                    fleet,
                });
            }
        }
        (BuildItem::Module { .. }, _) => {}
    }
    events.push(EconomyEvent::ItemCompleted { holding, item });
}

/// Starts every waiting item at `holding` that has a free lane, is affordable and fits the
/// command cap, in queue order (D-068).
pub(crate) fn start_items(
    state: &mut UniverseState,
    holding: HoldingId,
    events: &mut Vec<EconomyEvent>,
) {
    let Some(h) = state.holdings.get(&holding) else {
        return;
    };
    if !h.core_complete {
        return;
    }
    let owner = h.owner;
    let Some(e) = state.economy.holdings.get(&holding) else {
        return;
    };
    let mut main_free = !e.queue.iter().any(|entry| {
        matches!(
            entry.state,
            EntryState::Building {
                lane: BuildLane::Main,
                ..
            }
        )
    });
    let mut free_docks: VecDeque<SimId> = e
        .slots
        .values()
        .copied()
        .filter(|&id| state.is_working(id) && state.module_kind(id) == Some(ModuleKind::DryDock))
        .filter(|&id| {
            !e.queue.iter().any(|entry| {
                matches!(entry.state, EntryState::Building { lane: BuildLane::Dock(d), .. } if d == id)
            })
        })
        .collect();
    let waiting: Vec<(usize, QueueEntry)> = e
        .queue
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, entry)| entry.state == EntryState::Waiting)
        .collect();
    let mut used = state.command_used(owner);
    let cap = state.command_cap(owner);
    for (index, entry) in waiting {
        if !main_free && free_docks.is_empty() {
            break;
        }
        let Some(lane) = choose_lane(state, entry.item, main_free, &free_docks) else {
            continue;
        };
        // The price fixed when the item was queued (D-189).
        let price = entry.price;
        let Some(build_ms) = build_ms(state, entry.item) else {
            continue;
        };
        let command = match entry.item {
            BuildItem::Ship { class } => state.rules.economy.ship(class).map_or(0, |s| s.command),
            _ => 0,
        };
        if let Some(short) = shortage(state.pool(owner), price, used, command, cap) {
            mark_waiting(state, holding, index, entry.id, short, events);
            continue;
        }
        // Pay and start.
        let pool = state.economy.pools.entry(owner).or_default();
        *pool = pool.minus(price);
        used = used.saturating_add(command);
        let module = match entry.item {
            BuildItem::Module { kind, slot } => {
                Some(spawn_module(state, holding, slot, kind, Some(build_ms)))
            }
            _ => None,
        };
        match lane {
            BuildLane::Main => main_free = false,
            BuildLane::Dock(d) => free_docks.retain(|&x| x != d),
        }
        if let Some(slot) = state
            .economy
            .holdings
            .get_mut(&holding)
            .and_then(|e| e.queue.get_mut(index))
        {
            slot.state = EntryState::Building {
                lane,
                remaining_ms: build_ms,
                paid: price,
                module,
            };
            slot.short = None;
        }
        events.push(EconomyEvent::ItemStarted {
            holding,
            item: entry.id,
        });
    }
}

fn is_frigate(state: &UniverseState, class: ShipClass) -> bool {
    state.rules.class(class).frigate
}

/// The lane an item would start in now, if any (D-068): modules and kits in the main lane;
/// frigates in a dock lane; small craft in the main lane, else a dock lane.
fn choose_lane(
    state: &UniverseState,
    item: BuildItem,
    main_free: bool,
    free_docks: &VecDeque<SimId>,
) -> Option<BuildLane> {
    let dock = || free_docks.front().copied().map(BuildLane::Dock);
    match item {
        BuildItem::Module { .. } | BuildItem::CoreKit => main_free.then_some(BuildLane::Main),
        BuildItem::Ship { class } if is_frigate(state, class) => dock(),
        BuildItem::Ship { .. } if main_free => Some(BuildLane::Main),
        BuildItem::Ship { .. } => dock(),
    }
}

/// Records why queue entry `index` waits, with one event when the reason changes.
fn mark_waiting(
    state: &mut UniverseState,
    holding: HoldingId,
    index: usize,
    item: SimId,
    short: Shortage,
    events: &mut Vec<EconomyEvent>,
) {
    if let Some(slot) = state
        .economy
        .holdings
        .get_mut(&holding)
        .and_then(|e| e.queue.get_mut(index))
        && slot.short != Some(short)
    {
        slot.short = Some(short);
        events.push(EconomyEvent::ItemWaiting {
            holding,
            item,
            short,
        });
    }
}

/// What keeps an item from starting: the pool, then the command cap (D-068, D-129).
fn shortage(pool: Stock, price: Stock, used: u32, command: u32, cap: u32) -> Option<Shortage> {
    if pool.alloy < price.alloy {
        Some(Shortage::Alloy)
    } else if pool.crystal < price.crystal {
        Some(Shortage::Crystal)
    } else if command > 0 && used.saturating_add(command) > cap {
        Some(Shortage::CommandCap)
    } else {
        None
    }
}

/// Spawns a finished ship at `holding`: into the rally fleet if it is docked there with room
/// or in space in the holding's system; otherwise docked as a new fleet, or, if docking is
/// full, as a new fleet in space at the launch point (S4 plan §4.7). *Derived.*
pub(crate) fn spawn_ship(
    state: &mut UniverseState,
    holding: HoldingId,
    owner: CommanderId,
    class: ShipClass,
    now: Tick,
    fleet_events: &mut Vec<crate::fleet::FleetEvent>,
) -> (ShipId, FleetId) {
    let _ = (now, fleet_events);
    let owner = Owner::Commander(owner);
    let room = state.docked_ships(holding) < state.docking_capacity(holding);
    let system = state
        .holdings
        .get(&holding)
        .and_then(|h| state.galaxy.site(h.site))
        .map(|s| s.system);
    let launch = station::launch_point(state, holding).unwrap_or_default();
    let rally = state
        .economy
        .holdings
        .get(&holding)
        .and_then(|e| e.rally)
        .and_then(|fleet| {
            state
                .entities
                .get::<Fleet>(fleet.sim_id())
                .filter(|f| f.owner == owner)
                .map(|f| (fleet, f.place))
        })
        .filter(|&(_, place)| match place {
            Place::Docked(at) => at == holding && room,
            Place::InSystem(s) => Some(s) == system,
            Place::InTransit { .. } | Place::EmergencyJump { .. } => false,
        });
    let ship = ShipId(state.ids.allocate());
    let spacing = state.rules.movement.formation_spacing_m;
    let at_rest = Motion {
        position: launch,
        velocity: Vec2::ZERO,
        heading_rad: 0.0,
    };
    if let Some((fleet_id, _)) = rally {
        let anchor = state
            .entities
            .get::<Motion>(fleet_id.sim_id())
            .unwrap_or(at_rest);
        if let Some(fleet) = state.entities.get_mut::<Fleet>(fleet_id.sim_id()) {
            fleet.members.insert(ship, Vec2::ZERO);
            reform(fleet, spacing);
        }
        let docked = matches!(
            state
                .entities
                .with::<Fleet, _>(fleet_id.sim_id(), |f| f.place),
            Some(Place::Docked(_))
        );
        let position = if docked { anchor.position } else { launch };
        state.entities.spawn(
            ship.sim_id(),
            (
                Ship {
                    class,
                    owner,
                    fleet: fleet_id,
                },
                Motion {
                    position,
                    ..at_rest
                },
            ),
        );
        return (ship, fleet_id);
    }
    let fleet_id = FleetId(state.ids.allocate());
    let place = match (room, system) {
        (true, _) | (false, None) => Place::Docked(holding),
        (false, Some(system)) => Place::InSystem(system),
    };
    let mut fleet = Fleet {
        owner,
        place,
        members: BTreeMap::from([(ship, Vec2::ZERO)]),
        rules_of_engagement: DEFAULT_RULES_OF_ENGAGEMENT,
        engaged_until: None,
        active: None,
        queue: VecDeque::new(),
    };
    reform(&mut fleet, spacing);
    let offset = fleet.members.get(&ship).copied().unwrap_or_default();
    state.entities.spawn(
        ship.sim_id(),
        (
            Ship {
                class,
                owner,
                fleet: fleet_id,
            },
            Motion {
                position: slot_position(&at_rest, offset).into(),
                ..at_rest
            },
        ),
    );
    state.entities.spawn(fleet_id.sim_id(), (fleet, at_rest));
    (ship, fleet_id)
}
