//! Rebuilding losses from the replacement list within the daily budget (doc 12 §5.1; D-016,
//! D-208; A1 plan §5.7).
//!
//! - **Ships:** the replacement list holds entries "keep `fleet` at `count` × `class`". When
//!   an own ship of a listed class is lost from that fleet (inbox), one replacement is queued
//!   at the nearest own holding by route time that can build the class and has queue room,
//!   with the rally set to the fleet. If the fleet no longer exists, the replacement forms a
//!   new fleet, which the entry then keeps up.
//! - **Modules:** when an own module is destroyed and the holding doctrine's "rebuild
//!   modules" is on (the default), the same module is queued in the same slot.
//! - **The budget:** the preset's share (50%, 30% or 15%) of each resource's income over the
//!   last 24 h, against replacement spending over the same rolling 24 h. The Deputy samples
//!   the hourly income into a 24-slot ledger, so the economy needs no change.
//! - **Deferred:** a replacement that does not fit the budget, any builder or any queue is
//!   not queued; the Deputy reports [`DeputyEvent::Deferred`] once per reason and retries
//!   when the budget changes.
//! - It queues nothing else: no new projects (D-016).
//!
//! *Proposed* readings:
//! - "keep at `count`": a loss is replaced only while the fleet's ships of the class, with
//!   the replacements already on their way, number fewer than `count`;
//! - the ledger takes one sample in each hour of the season, at the first evaluation in the
//!   hour, so the first hour's budget is the share of the income at the season start (zero);
//! - spending counts when the Deputy queues an item, at its queued price, and is given back
//!   if the command is refused or dropped; a later cancel by the player does not refund it;
//! - with the fleet gone, "nearest" is measured from the entry role's holding, and without
//!   one by holding identity;
//! - the rally is set when it differs and left set afterwards; with the fleet gone it is
//!   cleared, so the ship forms a new fleet (S4 plan §4.7);
//! - a retry after a budget change counts its latency from the evaluation that retries it.

use std::collections::{BTreeMap, VecDeque};

use nomad_types::{
    BuildItem, Command, EconomyCommand, FleetId, FleetRole, HoldingId, Location, ModuleKind,
    ShipClass, Stock, Tick, Ticks,
};
use serde::{Deserialize, Serialize};

use super::{DeferReason, DeputyEvent, Rebuild, Stagger};
use crate::inbox::{CommandOutcome, InboxEntry, InboxEvent};
use crate::view::{BuildCheck, FleetPlace, RouteQuery, RouteSpeed, RouteTo, View};
use crate::{Issue, Layer, Output};

/// The ledger keeps this many hourly samples (D-208: 24 h).
const LEDGER_SLOTS: usize = 24;

/// Replacements in progress, the ledger and the spending (A1 plan §4.2 "budget ledgers").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct RebuildMemory {
    /// Hourly samples of the income over the last hour, oldest first.
    ledger: VecDeque<Stock>,
    /// When the next sample is due.
    next_sample: Option<Tick>,
    /// Replacement spending, oldest first: when, and how much.
    spent: VecDeque<(Tick, Stock)>,
    /// What waits to be queued.
    pending: Vec<Pending>,
    /// Build commands issued and not yet answered.
    in_flight: Vec<InFlight>,
    /// Ships queued for an entry whose fleet was gone: the new fleet they form becomes the
    /// entry's.
    awaiting: Vec<Awaiting>,
    /// An entry's fleet, by the entry's original fleet, once a replacement formed a new one.
    successors: BTreeMap<FleetId, FleetId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Pending {
    what: Rebuild,
    /// The event that called for it, or the evaluation that retries it.
    trigger: Tick,
    /// Why it was last deferred, and the budget left then.
    deferred: Option<(DeferReason, Stock)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct InFlight {
    what: Rebuild,
    command: Command,
    price: Stock,
    spent_at: Tick,
    /// The holding builds a ship for an entry whose fleet is gone.
    new_fleet: Option<HoldingId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Awaiting {
    entry: FleetId,
    holding: HoldingId,
    class: ShipClass,
    since: Tick,
}

/// Reads the fresh inbox entries: losses to replace, ships built, and answers to the
/// Deputy's build commands.
pub(super) fn read(view: &impl View, memory: &mut RebuildMemory, fresh: &[&InboxEntry]) {
    for entry in fresh {
        match &entry.event {
            InboxEvent::ShipLost { class, fleet, .. } => {
                let Some(listed) = memory.entry_for(view, *fleet, *class) else {
                    continue;
                };
                let what = Rebuild::Ship {
                    fleet: listed.fleet,
                    class: *class,
                };
                let waiting = memory.pending.iter().filter(|p| p.what == what).count();
                if waiting < usize::try_from(listed.count).unwrap_or(usize::MAX) {
                    memory.pending.push(Pending {
                        what,
                        trigger: entry.tick,
                        deferred: None,
                    });
                }
            }
            InboxEvent::ModuleDestroyed {
                holding,
                kind,
                slot,
                ..
            } => {
                // A core cannot be built; its holding has fallen (doc 15 §6).
                let rebuild = *kind != ModuleKind::Core
                    && view
                        .holding_doctrine(*holding)
                        .is_some_and(|d| d.rebuild_modules);
                let what = Rebuild::Module {
                    holding: *holding,
                    kind: *kind,
                    slot: *slot,
                };
                if rebuild && memory.pending.iter().all(|p| p.what != what) {
                    memory.pending.push(Pending {
                        what,
                        trigger: entry.tick,
                        deferred: None,
                    });
                }
            }
            InboxEvent::ShipBuilt {
                holding,
                class,
                fleet,
                ..
            } => {
                if let Some(at) = memory
                    .awaiting
                    .iter()
                    .position(|a| a.holding == *holding && a.class == *class)
                {
                    let awaited = memory.awaiting.remove(at);
                    memory.successors.insert(awaited.entry, *fleet);
                }
            }
            InboxEvent::Outcome {
                layer: Layer::Deputy,
                command,
                outcome,
            } => memory.answered(command, *outcome, entry.tick),
            _ => {}
        }
    }
    // A ship that was never built (its item cancelled) is no longer awaited after a day.
    let now = view.now();
    memory
        .awaiting
        .retain(|a| now.get().saturating_sub(a.since.get()) < Ticks::from_days(1).get());
}

/// One sample of the income over the last hour into the ledger in each hour of the season,
/// at the first evaluation in it.
pub(super) fn sample(view: &impl View, memory: &mut RebuildMemory) {
    let now = view.now();
    if memory.next_sample.is_some_and(|due| now < due) {
        return;
    }
    let Some(income) = view.income_last_hour() else {
        return;
    };
    memory.ledger.push_back(income);
    while memory.ledger.len() > LEDGER_SLOTS {
        memory.ledger.pop_front();
    }
    let hour = Ticks::from_hours(1).get();
    memory.next_sample = Some(Tick::new((now.get() / hour + 1) * hour));
}

/// Queues the due pending replacements that fit the budget.
pub(super) fn act(
    view: &impl View,
    memory: &mut RebuildMemory,
    stagger: Stagger,
    out: &mut Output,
) {
    if memory.pending.is_empty() {
        return;
    }
    let now = view.now();
    let mut available = memory.available(view, now);
    let mut kept = Vec::with_capacity(memory.pending.len());
    for mut pending in std::mem::take(&mut memory.pending) {
        if !stagger.due(pending.what.unit()) {
            kept.push(pending);
            continue;
        }
        // A deferred replacement waits for the next budget change.
        if pending.deferred.is_some_and(|(_, then)| then == available) {
            kept.push(pending);
            continue;
        }
        match memory.plan(view, &pending) {
            Plan::Drop => {}
            Plan::Wait => kept.push(pending),
            Plan::Defer(reason) => {
                if pending.deferred.map(|(r, _)| r) != Some(reason) {
                    out.events.push(DeputyEvent::Deferred {
                        what: pending.what,
                        reason,
                    });
                }
                pending.deferred = Some((reason, available));
                kept.push(pending);
            }
            Plan::Queue(order) => {
                if !available.covers(order.price) {
                    let reason = DeferReason::BudgetExhausted;
                    if pending.deferred.map(|(r, _)| r) != Some(reason) {
                        out.events.push(DeputyEvent::Deferred {
                            what: pending.what,
                            reason,
                        });
                    }
                    pending.deferred = Some((reason, available));
                    kept.push(pending);
                    continue;
                }
                let trigger = if pending.deferred.is_some() {
                    now
                } else {
                    pending.trigger
                };
                if order.set_rally {
                    out.commands.push(Issue {
                        trigger,
                        command: Command::Economy(EconomyCommand::SetRally {
                            holding: order.holding,
                            fleet: order.rally_to,
                        }),
                    });
                }
                let command = Command::Economy(EconomyCommand::Build {
                    holding: order.holding,
                    item: order.item,
                });
                out.commands.push(Issue {
                    trigger,
                    command: command.clone(),
                });
                available = available.minus(order.price);
                memory.spent.push_back((now, order.price));
                memory.in_flight.push(InFlight {
                    what: pending.what,
                    command,
                    price: order.price,
                    spent_at: now,
                    new_fleet: order.new_fleet.then_some(order.holding),
                });
            }
        }
    }
    memory.pending = kept;
}

/// What to do about one pending replacement now.
enum Plan {
    /// Nothing is left to do.
    Drop,
    /// Not now, and not a deferral: its fleet is in transit.
    Wait,
    /// It cannot be queued, for this reason.
    Defer(DeferReason),
    /// Queue it so.
    Queue(Order),
}

struct Order {
    holding: HoldingId,
    item: BuildItem,
    price: Stock,
    /// Set the holding's rally to `rally_to` first: it differs.
    set_rally: bool,
    /// The fleet the ship should join; `None` forms a new fleet.
    rally_to: Option<FleetId>,
    new_fleet: bool,
}

impl Rebuild {
    /// The unit whose slot it runs on (D-203).
    fn unit(self) -> u64 {
        match self {
            Self::Ship { fleet, .. } => fleet.sim_id().get(),
            Self::Module { holding, .. } => holding.sim_id().get(),
        }
    }
}

impl RebuildMemory {
    /// The replacement-list entry that covers a ship of `class` lost from `fleet`.
    fn entry_for(
        &self,
        view: &impl View,
        fleet: FleetId,
        class: ShipClass,
    ) -> Option<nomad_types::ReplacementEntry> {
        view.replacement_list().into_iter().find(|e| {
            e.class == class && (e.fleet == fleet || self.successors.get(&e.fleet) == Some(&fleet))
        })
    }

    /// The budget left: the preset's share of the ledger's income, less the spending of the
    /// last 24 h (D-208).
    fn available(&mut self, view: &impl View, now: Tick) -> Stock {
        let day = Ticks::from_days(1).get();
        while self
            .spent
            .front()
            .is_some_and(|&(t, _)| now.get().saturating_sub(t.get()) >= day)
        {
            self.spent.pop_front();
        }
        let Some(preset) = view.doctrine_preset() else {
            return Stock::ZERO;
        };
        let share = view.preset_values(preset).replacement_budget_permille;
        let income = self.ledger.iter().fold(Stock::ZERO, |sum, &s| sum.plus(s));
        let spent = self
            .spent
            .iter()
            .fold(Stock::ZERO, |sum, &(_, s)| sum.plus(s));
        income.permille(share).minus(spent)
    }

    /// An answer to one of the Deputy's commands.
    fn answered(&mut self, command: &Command, outcome: CommandOutcome, tick: Tick) {
        let Some(at) = self.in_flight.iter().position(|f| &f.command == command) else {
            return;
        };
        let flight = self.in_flight.remove(at);
        match outcome {
            CommandOutcome::Applied => {
                if let (Some(holding), Rebuild::Ship { fleet, class }) =
                    (flight.new_fleet, flight.what)
                {
                    self.awaiting.push(Awaiting {
                        entry: fleet,
                        holding,
                        class,
                        since: tick,
                    });
                }
            }
            CommandOutcome::Rejected | CommandOutcome::Dropped(_) => {
                if let Some(at) = self
                    .spent
                    .iter()
                    .position(|&(t, s)| t == flight.spent_at && s == flight.price)
                {
                    self.spent.remove(at);
                }
                self.pending.push(Pending {
                    what: flight.what,
                    trigger: tick,
                    deferred: None,
                });
            }
        }
    }

    fn plan(&self, view: &impl View, pending: &Pending) -> Plan {
        match pending.what {
            Rebuild::Ship { fleet, class } => self.plan_ship(view, fleet, class),
            Rebuild::Module {
                holding,
                kind,
                slot,
            } => {
                if !view
                    .holding_doctrine(holding)
                    .is_some_and(|d| d.rebuild_modules)
                {
                    return Plan::Drop;
                }
                let item = BuildItem::Module { kind, slot };
                match view.build_check(holding, item) {
                    BuildCheck::Ok => match view.build_cost(holding, item) {
                        Some(price) => Plan::Queue(Order {
                            holding,
                            item,
                            price,
                            set_rally: false,
                            rally_to: None,
                            new_fleet: false,
                        }),
                        None => Plan::Defer(DeferReason::NoBuilder),
                    },
                    // Someone queued it, or built something there, already.
                    BuildCheck::SlotTaken => Plan::Drop,
                    BuildCheck::QueueFull => Plan::Defer(DeferReason::QueueFull),
                    BuildCheck::CannotBuild => Plan::Defer(DeferReason::NoBuilder),
                }
            }
        }
    }

    fn plan_ship(&self, view: &impl View, entry_fleet: FleetId, class: ShipClass) -> Plan {
        let Some(entry) = view
            .replacement_list()
            .into_iter()
            .find(|e| e.fleet == entry_fleet && e.class == class)
        else {
            return Plan::Drop;
        };
        let what = Rebuild::Ship {
            fleet: entry_fleet,
            class,
        };
        // The fleet the entry keeps up: its own, or the one a replacement formed.
        let target = [
            self.successors.get(&entry_fleet).copied(),
            Some(entry_fleet),
        ]
        .into_iter()
        .flatten()
        .find_map(|f| view.own_fleet(f));
        let have = target
            .as_ref()
            .map_or(0, |f| f.ships.iter().filter(|(_, c)| *c == class).count());
        let coming = self.in_flight.iter().filter(|f| f.what == what).count()
            + self
                .awaiting
                .iter()
                .filter(|a| a.entry == entry_fleet && a.class == class)
                .count();
        if have + coming >= usize::try_from(entry.count).unwrap_or(usize::MAX) {
            return Plan::Drop;
        }
        let origin: Option<Location> = match &target {
            Some(fleet) => match fleet.place {
                FleetPlace::InSpace(at) => Some(at),
                FleetPlace::Docked(holding) => view.holding_location(holding),
                FleetPlace::Travelling => return Plan::Wait,
            },
            None => match &entry.role {
                Some(FleetRole::Garrison { holding } | FleetRole::Reserve { holding }) => {
                    view.holding_location(*holding)
                }
                _ => None,
            },
        };
        let item = BuildItem::Ship { class };
        let mut best: Option<(u64, HoldingId)> = None;
        let mut queue_full = false;
        for holding in view.own_holdings() {
            if !holding.complete {
                continue;
            }
            match view.build_check(holding.id, item) {
                BuildCheck::Ok => {}
                BuildCheck::QueueFull => {
                    queue_full = true;
                    continue;
                }
                BuildCheck::SlotTaken | BuildCheck::CannotBuild => continue,
            }
            let distance = match origin {
                Some(from) => {
                    let query = RouteQuery {
                        from,
                        to: RouteTo::Holding(holding.id),
                        speed: RouteSpeed::Class(class),
                        avoid: Vec::new(),
                    };
                    match view.route(&query) {
                        Some(route) => route.duration_ms,
                        None => continue,
                    }
                }
                None => 0,
            };
            if best.is_none_or(|b| (distance, holding.id) < b) {
                best = Some((distance, holding.id));
            }
        }
        let Some((_, holding)) = best else {
            return Plan::Defer(if queue_full {
                DeferReason::QueueFull
            } else {
                DeferReason::NoBuilder
            });
        };
        let Some(price) = view.build_cost(holding, item) else {
            return Plan::Defer(DeferReason::NoBuilder);
        };
        let rally_to = target.as_ref().map(|f| f.id);
        Plan::Queue(Order {
            holding,
            item,
            price,
            set_rally: view.rally(holding) != rally_to,
            rally_to,
            new_fleet: target.is_none(),
        })
    }
}
