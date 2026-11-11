//! The raider director: steers each NPC raid group from its haven to its target system,
//! picks what it attacks there, withdraws it and brings it home (doc 13 §8; D-077, D-081,
//! D-181, D-219, D-222; A3 plan §4.5–§4.8).
//!
//! **What it sees.** Only its faction's view (D-077, D-181): its own raid groups and the
//! launch rule's record of each (the target commander and system, the spawned value), public
//! data (holdings and their sites, D-044), and live sensing: the ships and holdings the
//! faction's last sensors pass detected, with a holding's modules (D-175). It has no
//! last-known memory (D-181), and no query tells it anyone's fleet value: the launch rule
//! sized the group (D-219).
//!
//! **How it acts.** Ordinary fleet commands as `Owner::Npc`, through the outbox, the
//! competence gate (Normal's values, D-288) and the single validator (D-077, D-200). The
//! simulation calls [`evaluate`] for each faction with groups on every tick of phase 4; each
//! group is evaluated at 2 Hz, on the tick where its fleet's `SimId` modulo the interval
//! equals the tick's slot (A1 plan §4.5). The inbox is read on every call.
//!
//! **A group's raid (A3 plan §4.5–§4.7):**
//!
//! 1. Spawned at the haven, it jumps to the target system (`Jump`; S3 routes it).
//! 2. On arrival the raid clock (D-313: 15 minutes) starts when the shroud ends: the
//!    inbox's arrival tick plus the shroud (D-055, D-222).
//! 3. It withdraws, with a `Jump` back to the haven, when the clock runs out, when it has
//!    lost half its spawned value (destroyed ships, by value), or when the sensed hostile
//!    value within 5 km exceeds twice its remaining value (D-081, D-222). The launch rule
//!    removes it on arrival (D-221).
//! 4. Until then, after the shroud, it attacks the best sensed target, in priority order:
//!    the target commander's MULEs; its outposts under construction; its ships of a fleet
//!    worth less than the group; its peripheral structures, solar arrays first. Within each,
//!    targets outside every sensed turret's reach come first, then the nearest, then the
//!    lowest `SimId`. With nothing to attack it `AttackMove`s towards the target commander's
//!    holding there, sensing on the way.
//!
//! It never orders an attack on a dome or a module inside one (D-056, D-163): those would
//! take no damage, so the clock is not wasted on them. Withdrawals are reported as
//! [`DirectorEvent`]s for the harness.
//!
//! *Proposed* readings, where the plan leaves a gap:
//!
//! - It attacks only the target commander's assets: D-081's "no commander is raided twice
//!   within 6 h" holds only if a raid on one commander does not raid another who shares the
//!   system. Weapons free still fires at any hostile ship in range (S5a).
//! - "Ships worth less than the group" compares one detected fleet's value with the group's
//!   remaining value; "losing half" includes exactly half.
//! - A target under attack is kept while it stays in the best tier that has a target, so
//!   the group does not switch between equals. A disabled peripheral structure is no target:
//!   raiders only disable (D-223).
//! - The `AttackMove` goes to a point 5 km (twice a turret's reach) short of the holding's
//!   site on the group's side: a site's own point lies inside its station's footprint, where
//!   the validator refuses a move, and a group detects a holding from 18 km (D-175), long
//!   before it gets there. The holding is the target commander's outpost there if it has
//!   one, otherwise its nearest holding, then the lowest identity. With no holding left
//!   there, the group waits for its clock. A refused approach is not repeated, and a target
//!   whose attack was refused is not chosen again.
//! - A command is issued again only when the director wants another one, when the last one
//!   was refused or dropped, or when a `Jump` or `Attack` that applied has left the fleet
//!   idle.

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{
    Command, FleetId, FleetOrder, HoldingId, Location, ModuleKind, Owner, PositionMetres, Queueing,
    SimId, Slot, SystemId, Tick, Ticks,
};
use serde::{Deserialize, Serialize};

use crate::inbox::{CommandOutcome, FleetHappening, InboxEntry, InboxEvent};
use crate::view::{FleetPlace, OwnFleet, RaidOrder, RaidRules, View};

/// A detected ship of another owner, from the view's sensed fleets (D-174).
#[derive(Clone, Copy, Debug)]
struct Contact {
    ship: nomad_types::ShipId,
    fleet: FleetId,
    owner: Owner,
    class: nomad_types::ShipClass,
    position: PositionMetres,
    shrouded: bool,
}

/// Every detected ship of another owner in `system`, by fleet and then ship identity.
fn contacts(view: &impl View, system: SystemId) -> Vec<Contact> {
    view.sensed_fleets_in(system)
        .into_iter()
        .flat_map(|fleet| {
            fleet.ships.into_iter().map(move |ship| Contact {
                ship: ship.id,
                fleet: fleet.id,
                owner: fleet.owner,
                class: ship.class,
                position: ship.position,
                shrouded: ship.shrouded,
            })
        })
        .collect()
}
use crate::{Issue, Layer, Output};

/// The director's memory for one faction (A1 plan §4.2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DirectorMemory {
    /// Inbox entries up to this tick have been read.
    read_until: Option<Tick>,
    /// Per raid group.
    groups: BTreeMap<FleetId, GroupMemory>,
}

impl DirectorMemory {
    /// Whether it remembers nothing.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// When `fleet`'s raid clock started, if it has (D-222).
    pub fn clock_started(&self, fleet: FleetId) -> Option<Tick> {
        self.groups.get(&fleet).and_then(|g| g.clock_from)
    }

    /// Why and when `fleet` withdrew, if it has.
    pub fn withdrawal(&self, fleet: FleetId) -> Option<(Tick, Withdrawal)> {
        self.groups.get(&fleet).and_then(|g| g.withdrew)
    }
}

/// One group's memory.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct GroupMemory {
    /// The tick its raid clock started: the end of its arrival shroud in the target system.
    clock_from: Option<Tick>,
    /// When and why it withdrew.
    withdrew: Option<(Tick, Withdrawal)>,
    /// The last command issued for it, and what became of it.
    ordered: Option<Ordered>,
    /// An approach move was refused: it is not repeated.
    no_approach: bool,
    /// Targets whose attack was refused.
    refused: BTreeSet<SimId>,
}

/// The last command issued for a group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Ordered {
    intent: Intent,
    command: Command,
    state: Sent,
}

/// What became of the last command so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Sent {
    /// Waiting in the outbox.
    Waiting,
    /// The validator accepted it.
    Applied,
    /// Refused or dropped.
    Failed,
}

/// What the director wants a group to do.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Intent {
    /// Jump to a system.
    Jump(SystemId),
    /// Attack a target: a ship, an outpost or a peripheral module.
    Attack(SimId),
    /// Move towards a holding, engaging what it meets.
    Approach(HoldingId),
}

/// Why a group withdrew (D-081, D-222).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Withdrawal {
    /// Its clock ran out (D-313: 15 minutes).
    Clock,
    /// It lost half its spawned value.
    Losses,
    /// The sensed hostile value near it exceeded twice its own.
    StrongerForce,
}

/// What the director reports, for the harness (A3 plan §4.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectorEvent {
    /// A group's raid clock started in its target system (D-222).
    ClockStarted {
        /// The group.
        fleet: FleetId,
    },
    /// A group withdrew towards its haven.
    Withdrew {
        /// The group.
        fleet: FleetId,
        /// Why.
        reason: Withdrawal,
    },
}

/// One call of the director for the observer's faction: reads the inbox, then evaluates the
/// groups due this tick.
pub fn evaluate(view: &impl View, memory: &mut DirectorMemory, out: &mut Output) {
    let Some(rules) = view.raid_rules() else {
        return;
    };
    let now = view.now();
    let orders = view.raid_orders();
    memory
        .groups
        .retain(|fleet, _| orders.iter().any(|o| o.fleet == *fleet));
    for order in &orders {
        memory.groups.entry(order.fleet).or_default();
    }
    read_inbox(view, memory, &orders, rules, out);
    let slot = now.get() % rules.interval_ticks.max(1);
    for order in &orders {
        if order.fleet.sim_id().get() % rules.interval_ticks.max(1) != slot {
            continue;
        }
        let Some(fleet) = view.own_fleet(order.fleet) else {
            continue;
        };
        let Some(group) = memory.groups.get_mut(&order.fleet) else {
            continue;
        };
        evaluate_group(view, rules, order, &fleet, group, out);
    }
}

/// Reads the inbox entries of the ticks completed since the last call: arrivals start raid
/// clocks, and the outcomes of the director's own commands.
fn read_inbox(
    view: &impl View,
    memory: &mut DirectorMemory,
    orders: &[RaidOrder],
    rules: RaidRules,
    out: &mut Output,
) {
    let now = view.now();
    let inbox = view.inbox();
    let read = memory.read_until;
    let from = inbox.partition_point(|e| read.is_some_and(|r| e.tick <= r));
    let to = inbox.partition_point(|e| e.tick < now).max(from);
    let fresh: &[InboxEntry] = &inbox[from..to];
    if let Some(newest) = fresh.last() {
        memory.read_until = Some(newest.tick);
    }
    for entry in fresh {
        match &entry.event {
            InboxEvent::Fleet {
                fleet,
                what: FleetHappening::Arrived { system },
            } => {
                let target = orders
                    .iter()
                    .any(|o| o.fleet == *fleet && o.system == *system);
                if let Some(group) = memory.groups.get_mut(fleet)
                    && target
                    && group.clock_from.is_none()
                {
                    group.clock_from = Some(entry.tick.after(Ticks::new(rules.shroud_ticks)));
                    out.director_events
                        .push(DirectorEvent::ClockStarted { fleet: *fleet });
                }
            }
            InboxEvent::Outcome {
                layer: Layer::Director,
                command,
                outcome,
            } => {
                let Command::Fleet { fleet, order, .. } = command else {
                    continue;
                };
                let Some(group) = memory.groups.get_mut(fleet) else {
                    continue;
                };
                let failed = !matches!(outcome, CommandOutcome::Applied);
                // A refusal is about the target or the point; a drop only about the rate.
                if matches!(outcome, CommandOutcome::Rejected) {
                    match order {
                        FleetOrder::Attack { target } => {
                            group.refused.insert(*target);
                        }
                        FleetOrder::AttackMove { .. } => group.no_approach = true,
                        _ => {}
                    }
                }
                if let Some(ordered) = &mut group.ordered
                    && ordered.command == *command
                {
                    ordered.state = if failed { Sent::Failed } else { Sent::Applied };
                }
            }
            _ => {}
        }
    }
}

/// One evaluation of one group.
fn evaluate_group(
    view: &impl View,
    rules: RaidRules,
    order: &RaidOrder,
    fleet: &OwnFleet,
    group: &mut GroupMemory,
    out: &mut Output,
) {
    let FleetPlace::InSpace(at) = fleet.place else {
        // In transit nothing can be ordered (D-042); a group is never docked.
        return;
    };
    let now = view.now();
    if group.withdrew.is_some() {
        if at.system != order.haven {
            jump(view, order, fleet, group, order.haven, out);
        }
        return;
    }
    if at.system != order.system {
        jump(view, order, fleet, group, order.system, out);
        return;
    }
    // In the target system. The clock starts when the shroud ends (D-222); the inbox gave
    // the arrival, and a group found here without one starts it now.
    let clock_from = *group.clock_from.get_or_insert(now);
    if now < clock_from {
        return;
    }
    let remaining = fleet
        .ships
        .iter()
        .map(|&(_, class)| view.ship_value(class))
        .fold(0_u64, u64::saturating_add);
    if let Some(reason) = withdrawal(view, rules, order, at, remaining, clock_from) {
        group.withdrew = Some((now, reason));
        out.director_events.push(DirectorEvent::Withdrew {
            fleet: order.fleet,
            reason,
        });
        jump(view, order, fleet, group, order.haven, out);
        return;
    }
    let current = match group.ordered {
        Some(Ordered {
            intent: Intent::Attack(target),
            state: Sent::Waiting | Sent::Applied,
            ..
        }) => Some(target),
        _ => None,
    };
    if let Some(target) = choose_target(view, rules, order, at, remaining, current, &group.refused)
    {
        let attack = FleetOrder::Attack { target };
        ensure(
            view,
            order,
            fleet,
            group,
            Intent::Attack(target),
            attack,
            out,
        );
    } else if !group.no_approach
        && let Some((holding, to)) = approach(view, rules, order, at)
    {
        let approach = FleetOrder::AttackMove { to };
        ensure(
            view,
            order,
            fleet,
            group,
            Intent::Approach(holding),
            approach,
            out,
        );
    }
}

/// Orders a `Jump` to `system`, unless one stands.
fn jump(
    view: &impl View,
    order: &RaidOrder,
    fleet: &OwnFleet,
    group: &mut GroupMemory,
    system: SystemId,
    out: &mut Output,
) {
    let jump = FleetOrder::Jump { system };
    ensure(view, order, fleet, group, Intent::Jump(system), jump, out);
}

/// Whether the group withdraws now, and why (D-081, D-222).
fn withdrawal(
    view: &impl View,
    rules: RaidRules,
    order: &RaidOrder,
    at: Location,
    remaining: u64,
    clock_from: Tick,
) -> Option<Withdrawal> {
    if view.now().get() >= clock_from.get().saturating_add(rules.clock_ticks) {
        return Some(Withdrawal::Clock);
    }
    let lost = order.value.saturating_sub(remaining);
    if u128::from(lost) * 1_000 >= u128::from(order.value) * u128::from(rules.loss_limit_permille) {
        return Some(Withdrawal::Losses);
    }
    let radius = f64::from(rules.withdraw_radius_m);
    let near = contacts(view, at.system)
        .iter()
        .filter(|c| distance_sq(c.position, at.position) <= radius * radius)
        .map(|c| view.ship_value(c.class))
        .fold(0_u64, u64::saturating_add);
    if u128::from(near) * 1_000 > u128::from(remaining) * u128::from(rules.withdraw_ratio_permille)
    {
        return Some(Withdrawal::StrongerForce);
    }
    None
}

/// The best target in the target system, as the module documentation orders them; `None`
/// if there is nothing to attack.
fn choose_target(
    view: &impl View,
    rules: RaidRules,
    order: &RaidOrder,
    at: Location,
    remaining: u64,
    current: Option<SimId>,
    refused: &BTreeSet<SimId>,
) -> Option<SimId> {
    let target = Owner::Commander(order.target);
    let contacts = contacts(view, order.system);
    let holdings = view.sensed_holdings_in(order.system);
    // Turrets of every detected holding, which fire at raiders whoever holds them.
    let reach = f64::from(rules.turret_reach_m);
    let turrets: Vec<PositionMetres> = holdings
        .iter()
        .flat_map(|h| &h.modules)
        .filter(|m| m.kind == ModuleKind::Turret && !m.disabled)
        .map(|m| m.position)
        .collect();
    let exposed = |p: PositionMetres| turrets.iter().any(|&t| distance_sq(p, t) <= reach * reach);
    // (exposed, a second key within the tier, distance², id) per candidate.
    let rank = |p: PositionMetres, second: u8, id: SimId| {
        (exposed(p), second, distance_sq(p, at.position), id)
    };
    let fleet_values: BTreeMap<FleetId, u64> =
        contacts
            .iter()
            .filter(|c| c.owner == target)
            .fold(BTreeMap::new(), |mut sum, c| {
                *sum.entry(c.fleet).or_insert(0) += view.ship_value(c.class);
                sum
            });
    let ships = contacts
        .iter()
        .filter(|c| c.owner == target && !c.shrouded && !refused.contains(&c.ship.sim_id()));
    let tiers: [Vec<(bool, u8, f64, SimId)>; 4] = [
        // 1. MULEs: convoys and kit carriers.
        ships
            .clone()
            .filter(|c| c.class == nomad_types::ShipClass::Mule)
            .map(|c| rank(c.position, 0, c.ship.sim_id()))
            .collect(),
        // 2. Outposts under construction (D-054): their core, which grows from 10% (D-224).
        holdings
            .iter()
            .filter(|h| h.owner == order.target && h.outpost)
            .filter(|h| !refused.contains(&h.holding.sim_id()))
            .map(|h| rank(h.position, 0, h.holding.sim_id()))
            .collect(),
        // 3. Ships of a fleet worth less than the group.
        ships
            .filter(|c| fleet_values.get(&c.fleet).is_some_and(|&v| v < remaining))
            .map(|c| rank(c.position, 0, c.ship.sim_id()))
            .collect(),
        // 4. Peripheral structures that still work, solar arrays first (D-138, D-223).
        holdings
            .iter()
            .filter(|h| h.owner == order.target)
            .flat_map(|h| &h.modules)
            .filter(|m| matches!(m.slot, Slot::Solar | Slot::Deposit(_)) && !m.disabled)
            .filter(|m| !refused.contains(&m.id))
            .map(|m| rank(m.position, u8::from(m.kind != ModuleKind::Solar), m.id))
            .collect(),
    ];
    let tier = tiers.iter().find(|t| !t.is_empty())?;
    if let Some(current) = current
        && tier.iter().any(|&(_, _, _, id)| id == current)
    {
        return Some(current);
    }
    tier.iter()
        .min_by(|a, b| {
            (a.0, a.1)
                .cmp(&(b.0, b.1))
                .then(a.2.total_cmp(&b.2))
                .then(a.3.cmp(&b.3))
        })
        .map(|&(_, _, _, id)| id)
}

/// Where the group moves with nothing to attack: short of the target commander's holding in
/// the system, on the group's side (see the module documentation).
fn approach(
    view: &impl View,
    rules: RaidRules,
    order: &RaidOrder,
    at: Location,
) -> Option<(HoldingId, Location)> {
    let holding = view
        .holdings_in(order.system)
        .into_iter()
        .filter(|h| h.owner == order.target)
        .min_by(|a, b| {
            (!a.outpost)
                .cmp(&!b.outpost)
                .then(
                    distance_sq(a.position, at.position)
                        .total_cmp(&distance_sq(b.position, at.position)),
                )
                .then(a.holding.cmp(&b.holding))
        })?;
    let standoff = 2.0 * f64::from(rules.turret_reach_m);
    let (dx, dy) = (
        at.position.x - holding.position.x,
        at.position.y - holding.position.y,
    );
    let distance = libm::sqrt(dx * dx + dy * dy);
    if distance <= standoff {
        // Already that close: the holding is in sensor range; nothing to approach.
        return None;
    }
    let scale = standoff / distance;
    let to = Location {
        system: order.system,
        position: PositionMetres::new(
            holding.position.x + dx * scale,
            holding.position.y + dy * scale,
        ),
    };
    Some((holding.holding, to))
}

/// Issues `fleet_order` for `intent` unless the last command already asks for it and still
/// stands (see the module documentation).
fn ensure(
    view: &impl View,
    order: &RaidOrder,
    fleet: &OwnFleet,
    group: &mut GroupMemory,
    intent: Intent,
    fleet_order: FleetOrder,
    out: &mut Output,
) {
    if let Some(last) = &group.ordered
        && last.intent == intent
    {
        let finished = !fleet.busy && !matches!(intent, Intent::Approach(_));
        match last.state {
            Sent::Waiting => return,
            Sent::Applied if !finished => return,
            Sent::Applied | Sent::Failed => {}
        }
    }
    let command = Command::Fleet {
        fleet: order.fleet,
        order: fleet_order,
        queue: Queueing::Replace,
        retreat: false,
    };
    out.commands.push(Issue {
        trigger: view.now(),
        command: command.clone(),
    });
    group.ordered = Some(Ordered {
        intent,
        command,
        state: Sent::Waiting,
    });
}

/// The squared distance between two points.
fn distance_sq(a: PositionMetres, b: PositionMetres) -> f64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy
}
