//! The raid launch rule: NPC raider factions, their launch timers, the target draw, the
//! group's strength and composition, its spawn and its removal (Design doc 13 §8, doc 15
//! §10; D-081, D-219 to D-221, D-047; A3 plan §4.1–§4.5).
//!
//! **A rule, not an AI decision (D-219).** When a haven launches, whom it raids and how
//! strong the group is are decided here, inside the step, like a kit's price (D-189). The
//! rule reads the target's real fleet value to size the group, which the faction could not
//! sense (D-077, D-181), and tells nobody what it read. What the group then does is the
//! raider director's (`nomad_ai::director`), through the faction's view and ordinary
//! commands.
//!
//! **Factions.** Each raider haven has one NPC faction, numbered from 1 in haven
//! (`SystemId`) order. *Proposed:* doc 13 §8 has raider factions operate from havens, and
//! [`FactionId`] left their numbering to A3; at stage 1 there is one haven and so
//! `FactionId(1)`, the faction the tests already use.
//!
//! **Launch timing (D-081, D-047; A3 plan §4.2).** Each faction has a timer in the Universe's
//! timer queue, fired in phase 2. The first fires 2–4 h after tick 0; each firing first draws
//! the next interval uniformly from [2 h, 4 h], whole ticks with both ends included, on the
//! haven's `RAIDS` stream (A1 plan §4.6), counted from the firing's deadline. During catch-up
//! a firing launches nothing (D-047) and is not deferred; groups already out keep raiding.
//!
//! - *Proposed:* whole ticks with both ends included; each firing draws in the order next
//!   interval, target, budget share, so the launch times depend on the seed alone; the next
//!   interval counts from the deadline, which is the firing's tick whenever every tick is
//!   stepped.
//!
//! **Target (D-081; A3 plan §4.3).** The candidates are each (commander, system) where the
//! commander has a holding or an outpost in a claimable system reachable from the haven,
//! and was not the target of a launch in the last 6 h. Each weighs, from public data only
//! (ownership D-044, deposits D-066, topology), (the deposit value of its holdings and
//! outposts there + 1 per completed holding + 2 per outpost under construction) ÷ (1 + jumps
//! from the haven), in thousandths, with the division rounded down. The draw is weighted, on
//! the haven's stream, over the candidates in (commander, system) order. With no candidate,
//! no raid.
//!
//! - *Proposed:* the 6 h spacing counts launches of every haven, since D-081 says no
//!   *commander* is raided twice within 6 h; it matters from stage 2's second haven.
//! - *Proposed:* weights are thousandths of a deposit-value unit, the division rounded down;
//!   "outposts under construction" are the holdings whose core is not complete (D-041).
//!
//! **Strength (D-081, D-220; A3 plan §4.4).** The target's fleet value in that system is the
//! value of its ships in space there or docked at a holding there (alloy + 3 × crystal of
//! each class's cost). The budget is a share drawn uniformly from 300–500 per mille, both
//! included, on the haven's stream, times that value, rounded down. One HSTL if the budget
//! covers it plus two strike craft, strike craft for the rest rounded down, and at least two
//! strike craft.
//!
//! - *Proposed:* the share in whole per mille; ships in lane transit or an emergency jump
//!   are in no system, so they do not count.
//!
//! **Spawn and removal (D-221; A3 plan §4.5).** The group is a new fleet of
//! `Owner::Npc(faction)`, its ships scattered in the haven's gate approach as arrivals are
//! (D-055, on the haven's arrival-scatter stream), at full hull, weapons free, with no
//! orders. From then on it is a fleet like any other. When it arrives back at its haven it
//! is removed at once, after movement and before the sensors pass (phase 5); when its last
//! ship is destroyed its record ends with it.
//!
//! - *Proposed:* a spawned group has no jump shroud: it did not jump, and nobody can be at
//!   the haven to see it. A returning group is removed in phase 5, right after movement, so
//!   no sensors pass sees it back at the haven.
//!
//! **Outputs (A3 plan §4.9).** Launches, suppressed and empty firings, returns and destroyed
//! groups are [`RaidEvent`]s in the tick's outputs, for the harness. They are not
//! announcements: nobody learns of a raid except through their sensors (D-074).

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, VecDeque};

use nomad_types::{FactionId, Owner, RulesOfEngagement};
use serde::{Deserialize, Serialize};

use crate::fleet::{Fleet, FleetEvent, FleetId, Motion, Place, Ship, ShipClass, ShipId, reform};
use crate::galaxy::{GateId, SystemId, SystemKind, is_claimable};
use crate::geometry::Vec2;
use crate::id::CommanderId;
use crate::rng::{StreamKey, purpose};
use crate::rules::Rules;
use crate::state::{ScheduledEvent, UniverseState};
use crate::time::{Tick, Ticks};

/// Every raider faction, and when each commander was last a raid's target (A3 plan §8
/// task 1).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Raids {
    /// One faction per haven, by identity.
    pub(crate) factions: BTreeMap<FactionId, Faction>,
    /// The tick of each commander's last launch against it, by any haven (D-081: 6 h).
    pub(crate) last_targeted: BTreeMap<CommanderId, Tick>,
}

/// One haven's NPC faction record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Faction {
    /// Its haven.
    pub(crate) haven: SystemId,
    /// The haven's gate, where groups spawn (D-099 item 6: one gate, to the hub).
    pub(crate) gate: GateId,
    /// When its launch timer fires next.
    pub(crate) next_launch: Tick,
    /// Its raid groups out in the Universe, by fleet.
    pub(crate) groups: BTreeMap<FleetId, RaidGroup>,
}

/// A raid group as the launch rule made it: the faction's own record of its raid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidGroup {
    /// The commander it raids.
    pub target: CommanderId,
    /// The system it raids.
    pub system: SystemId,
    /// The tick it was launched.
    pub launched: Tick,
    /// Its value at the spawn, which losses are counted against (D-222).
    pub value: u64,
}

/// A raider faction as the harness and tests read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaidFactionView {
    /// The faction.
    pub faction: FactionId,
    /// Its haven.
    pub haven: SystemId,
    /// When its launch timer fires next.
    pub next_launch: Tick,
    /// Its groups out in the Universe, in fleet order.
    pub groups: Vec<(FleetId, RaidGroup)>,
}

/// What the raid rule did on a tick (A3 plan §4.9): harness outputs, not announcements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaidEvent {
    /// A haven launched a raid.
    Launched {
        /// The faction.
        faction: FactionId,
        /// The new group.
        fleet: FleetId,
        /// The commander it raids.
        target: CommanderId,
        /// The system it raids.
        system: SystemId,
        /// The target's fleet value in that system when it was launched (D-220).
        fleet_value: u64,
        /// The drawn share of it, per mille (D-081: 300–500).
        share_permille: u32,
        /// The group's budget: the share of the fleet value.
        budget: u64,
        /// Its strike craft.
        strike_craft: u32,
        /// Its heavy cruisers (0 or 1).
        heavy_cruisers: u32,
        /// Its value (D-220).
        value: u64,
    },
    /// A haven's timer fired during catch-up and launched nothing (D-047).
    Suppressed {
        /// The faction.
        faction: FactionId,
    },
    /// A haven's timer fired with no candidate target, and launched nothing.
    NoTarget {
        /// The faction.
        faction: FactionId,
    },
    /// A group arrived back at its haven and was removed (D-221).
    Returned {
        /// The faction.
        faction: FactionId,
        /// The group.
        fleet: FleetId,
        /// Its ships left.
        ships: u32,
    },
    /// A group lost its last ship.
    Destroyed {
        /// The faction.
        faction: FactionId,
        /// The group.
        fleet: FleetId,
    },
}

/// The haven's `RAIDS` stream (A1 plan §4.6).
fn stream(haven: SystemId) -> StreamKey {
    StreamKey {
        scope: haven.sim_id().get(),
        purpose: purpose::RAIDS,
    }
}

/// Creates a faction for every raider haven with a gate and schedules its first launch,
/// 2–4 h after tick 0 (A3 plan §4.2). The stage-1 generator calls it last.
pub(crate) fn initialize(state: &mut UniverseState) {
    let havens: Vec<(SystemId, GateId)> = state
        .galaxy
        .systems()
        .filter(|(_, s)| s.kind == SystemKind::RaiderHaven)
        .filter_map(|(id, s)| Some((id, *s.gates.first()?)))
        .collect();
    for (n, (haven, gate)) in (1_u32..).zip(havens) {
        let faction = FactionId(n);
        let next_launch = schedule_next(state, faction, haven, Tick::ZERO);
        state.raids.factions.insert(
            faction,
            Faction {
                haven,
                gate,
                next_launch,
                groups: BTreeMap::new(),
            },
        );
    }
}

/// Draws the interval after `from` on the haven's stream and schedules the next launch.
fn schedule_next(
    state: &mut UniverseState,
    faction: FactionId,
    haven: SystemId,
    from: Tick,
) -> Tick {
    let raids = &state.rules.ai.raids;
    let lo = Ticks::from_secs(raids.interval_min_s).get();
    let hi = Ticks::from_secs(raids.interval_max_s).get();
    let interval = lo + state.rng.below(stream(haven), hi - lo + 1);
    let at = from.after(Ticks::new(interval));
    state
        .timers
        .schedule(&mut state.ids, at, ScheduledEvent::RaidLaunch { faction });
    at
}

/// Phase 2: `faction`'s timer, due at `deadline`, fires (A3 plan §4.2–§4.5).
pub(crate) fn launch(
    state: &mut UniverseState,
    now: Tick,
    deadline: Tick,
    faction: FactionId,
    events: &mut Vec<RaidEvent>,
) {
    let Some(haven) = state.raids.factions.get(&faction).map(|f| f.haven) else {
        return;
    };
    // The next interval first, so launch times depend on the seed alone.
    let next = schedule_next(state, faction, haven, deadline);
    if let Some(f) = state.raids.factions.get_mut(&faction) {
        f.next_launch = next;
    }
    if state.catching_up {
        // D-047: no new raid starts during catch-up, and none is deferred to its end.
        events.push(RaidEvent::Suppressed { faction });
        return;
    }
    let candidates = target_weights(state, haven, now);
    let total = candidates
        .iter()
        .fold(0_u64, |sum, (_, w)| sum.saturating_add(*w));
    if total == 0 {
        events.push(RaidEvent::NoTarget { faction });
        return;
    }
    let mut pick = state.rng.below(stream(haven), total);
    let mut chosen = None;
    for &(candidate, weight) in &candidates {
        if pick < weight {
            chosen = Some(candidate);
            break;
        }
        pick -= weight;
    }
    let Some((target, system)) = chosen else {
        return;
    };

    let raids = state.rules.ai.raids;
    let fleet_value = fleet_value(state, target, system);
    let span = u64::from(raids.budget_max_permille - raids.budget_min_permille);
    let share_permille = raids.budget_min_permille
        + u32::try_from(state.rng.below(stream(haven), span + 1))
            .expect("invariant: a draw below a u32 span fits in u32");
    let budget = fleet_value.saturating_mul(u64::from(share_permille)) / 1_000;
    let (heavy_cruisers, strike_craft) = composition(&state.rules, budget);
    let mut classes = vec![ShipClass::HeavyCruiser; heavy_cruisers as usize];
    classes.extend(std::iter::repeat_n(
        ShipClass::RaiderStrikeCraft,
        strike_craft as usize,
    ));
    let value = classes
        .iter()
        .map(|&c| class_value(&state.rules, c))
        .fold(0_u64, u64::saturating_add);
    let Some(fleet) = spawn_group(state, faction, &classes) else {
        return;
    };
    state.raids.last_targeted.insert(target, now);
    if let Some(f) = state.raids.factions.get_mut(&faction) {
        f.groups.insert(
            fleet,
            RaidGroup {
                target,
                system,
                launched: now,
                value,
            },
        );
    }
    events.push(RaidEvent::Launched {
        faction,
        fleet,
        target,
        system,
        fleet_value,
        share_permille,
        budget,
        strike_craft,
        heavy_cruisers,
        value,
    });
}

/// Every candidate (commander, system) for a launch from `haven` on `now`, with its weight
/// in thousandths, in (commander, system) order (A3 plan §4.3). Public data only: who holds
/// what (D-044), deposits (D-066) and the topology.
pub(crate) fn target_weights(
    state: &UniverseState,
    haven: SystemId,
    now: Tick,
) -> Vec<((CommanderId, SystemId), u64)> {
    let raids = &state.rules.ai.raids;
    let spacing = Ticks::from_secs(raids.target_spacing_s).get();
    let jumps = state.galaxy.jumps_from(haven);
    // (deposit value, completed holdings, outposts) per (commander, system).
    let mut assets: BTreeMap<(CommanderId, SystemId), (u64, u64, u64)> = BTreeMap::new();
    for (_, holding) in state.holdings() {
        let Some(site) = state.galaxy.site(holding.site) else {
            continue;
        };
        let claimable = state
            .galaxy
            .system(site.system)
            .is_some_and(|s| is_claimable(s.kind));
        if !claimable || !jumps.contains_key(&site.system) {
            continue;
        }
        let recent = state
            .raids
            .last_targeted
            .get(&holding.owner)
            .is_some_and(|&t| now.get().saturating_sub(t.get()) < spacing);
        if recent {
            continue;
        }
        let entry = assets.entry((holding.owner, site.system)).or_default();
        entry.0 = entry.0.saturating_add(site.value().milli());
        if holding.core_complete {
            entry.1 += 1;
        } else {
            entry.2 += 1;
        }
    }
    assets
        .into_iter()
        .map(|((commander, system), (deposits, holdings, outposts))| {
            let numerator = deposits
                .saturating_add(holdings.saturating_mul(raids.holding_weight_milli))
                .saturating_add(outposts.saturating_mul(raids.outpost_weight_milli));
            let distance = 1 + u64::from(jumps.get(&system).copied().unwrap_or(0));
            ((commander, system), numerator / distance)
        })
        .collect()
}

/// A ship class's value (D-220): a commander class's alloy cost plus 3 × its crystal cost;
/// an NPC class's rules value.
pub(crate) fn class_value(rules: &Rules, class: ShipClass) -> u64 {
    let raids = &rules.ai.raids;
    match class {
        ShipClass::RaiderStrikeCraft => raids.strike_craft_value,
        ShipClass::HeavyCruiser => raids.heavy_cruiser_value,
        _ => rules.economy.ships.get(&class).map_or(0, |ship| {
            ship.cost
                .alloy
                .saturating_add(raids.crystal_weight.saturating_mul(ship.cost.crystal))
        }),
    }
}

/// `commander`'s fleet value in `system` (D-081, D-220): its ships in space there or docked
/// at a holding there. Hidden state: only the launch rule reads it (D-219).
pub(crate) fn fleet_value(state: &UniverseState, commander: CommanderId, system: SystemId) -> u64 {
    let owner = Owner::Commander(commander);
    let mut value = 0_u64;
    for id in state.entities.ids_with::<Fleet>() {
        let Some(fleet) = state.entities.get::<Fleet>(id) else {
            continue;
        };
        if fleet.owner != owner {
            continue;
        }
        let here = match fleet.place {
            Place::InSystem(s) => s == system,
            Place::Docked(holding) => state
                .holdings
                .get(&holding)
                .and_then(|h| state.galaxy.site(h.site))
                .is_some_and(|site| site.system == system),
            Place::InTransit { .. } | Place::EmergencyJump { .. } => false,
        };
        if !here {
            continue;
        }
        for &ship in fleet.members.keys() {
            if let Some(s) = state.entities.get::<Ship>(ship.sim_id()) {
                value = value.saturating_add(class_value(&state.rules, s.class));
            }
        }
    }
    value
}

/// The group a budget buys (A3 plan §4.4; D-220): `(heavy cruisers, strike craft)`. One HSTL
/// if the budget covers it plus two strike craft, strike craft for the rest rounded down,
/// and never fewer strike craft than the floor.
pub(crate) fn composition(rules: &Rules, budget: u64) -> (u32, u32) {
    let raids = &rules.ai.raids;
    let strike = raids.strike_craft_value.max(1);
    let heavy = u64::from(budget >= raids.heavy_cruiser_value.saturating_add(2 * strike));
    let rest = budget.saturating_sub(heavy * raids.heavy_cruiser_value);
    let strike_craft = u32::try_from(rest / strike)
        .unwrap_or(u32::MAX)
        .max(raids.min_strike_craft);
    (u32::from(heavy == 1), strike_craft)
}

/// Spawns a group of `classes` for `faction` in its haven's gate approach (D-055, D-221).
fn spawn_group(
    state: &mut UniverseState,
    faction: FactionId,
    classes: &[ShipClass],
) -> Option<FleetId> {
    let f = state.raids.factions.get(&faction)?;
    let (haven, gate) = (f.haven, f.gate);
    let centre = Vec2::from(state.galaxy.gate(gate)?.position);
    let owner = Owner::Npc(faction);
    let key = StreamKey {
        scope: haven.sim_id().get(),
        purpose: purpose::ARRIVAL_SCATTER,
    };
    let scatter = state.rules.gates.arrival_scatter_m;
    let id = FleetId(state.ids.allocate());
    let mut fleet = Fleet {
        owner,
        place: Place::InSystem(haven),
        members: BTreeMap::new(),
        rules_of_engagement: RulesOfEngagement::WeaponsFree,
        engaged_until: None,
        active: None,
        queue: VecDeque::new(),
    };
    let mut ships = Vec::with_capacity(classes.len());
    let mut sum = Vec2::ZERO;
    for &class in classes {
        let ship = ShipId(state.ids.allocate());
        // Uniform over the disc, as arrivals are (D-055).
        let radius = libm::sqrt(state.rng.unit_f64(key)) * scatter;
        let angle = state.rng.unit_f64(key) * std::f64::consts::TAU;
        let position = centre + Vec2::from_angle(angle) * radius;
        sum += position;
        fleet.members.insert(ship, Vec2::ZERO);
        ships.push((ship, class, position));
    }
    reform(&mut fleet, state.rules.movement.formation_spacing_m);
    for (ship, class, position) in ships {
        state.entities.spawn(
            ship.sim_id(),
            (
                Ship {
                    class,
                    owner,
                    fleet: id,
                },
                Motion {
                    position: position.into(),
                    velocity: Vec2::ZERO,
                    heading_rad: 0.0,
                },
            ),
        );
    }
    let n = classes.len().max(1) as f64;
    let anchor = Motion {
        position: (sum * (1.0 / n)).into(),
        velocity: Vec2::ZERO,
        heading_rad: 0.0,
    };
    state.entities.spawn(id.sim_id(), (fleet, anchor));
    Some(id)
}

/// Phase 5, after movement: removes every group that arrived back at its haven (D-221).
pub(crate) fn remove_returned(
    state: &mut UniverseState,
    fleet_events: &[FleetEvent],
    events: &mut Vec<RaidEvent>,
) {
    for event in fleet_events {
        let FleetEvent::Arrived { fleet, system, .. } = *event else {
            continue;
        };
        let Some(faction) = state
            .raids
            .factions
            .iter()
            .find(|(_, f)| f.haven == system && f.groups.contains_key(&fleet))
            .map(|(&id, _)| id)
        else {
            continue;
        };
        let ships = state
            .entities
            .get::<Fleet>(fleet.sim_id())
            .map(|f| f.members.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for &ship in &ships {
            state.entities.despawn(ship.sim_id());
        }
        state.entities.despawn(fleet.sim_id());
        if let Some(f) = state.raids.factions.get_mut(&faction) {
            f.groups.remove(&fleet);
        }
        events.push(RaidEvent::Returned {
            faction,
            fleet,
            ships: u32::try_from(ships.len()).unwrap_or(u32::MAX),
        });
    }
}

/// End of the step: ends the records of groups whose last ship was destroyed.
pub(crate) fn prune(state: &mut UniverseState, events: &mut Vec<RaidEvent>) {
    let entities = &state.entities;
    for (&faction, f) in &mut state.raids.factions {
        f.groups.retain(|&fleet, _| {
            let alive = entities.get::<Fleet>(fleet.sim_id()).is_some();
            if !alive {
                events.push(RaidEvent::Destroyed { faction, fleet });
            }
            alive
        });
    }
}

impl UniverseState {
    /// Every raider faction with its haven, its next launch and its groups out in the
    /// Universe, in faction order (A3 plan §4.9: for the harness).
    pub fn raid_factions(&self) -> Vec<RaidFactionView> {
        self.raids
            .factions
            .iter()
            .map(|(&faction, f)| RaidFactionView {
                faction,
                haven: f.haven,
                next_launch: f.next_launch,
                groups: f.groups.iter().map(|(&id, &g)| (id, g)).collect(),
            })
            .collect()
    }

    /// The raid group `fleet` and its faction, if it is one.
    pub fn raid_group(&self, fleet: FleetId) -> Option<(FactionId, RaidGroup)> {
        self.raids
            .factions
            .iter()
            .find_map(|(&id, f)| f.groups.get(&fleet).map(|&g| (id, g)))
    }

    /// The tick of the last launch against `commander`, if any (D-081).
    pub fn last_raided(&self, commander: CommanderId) -> Option<Tick> {
        self.raids.last_targeted.get(&commander).copied()
    }
}
