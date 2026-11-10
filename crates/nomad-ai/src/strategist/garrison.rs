//! The *Garrison* stand-in until A4 (D-212; A2 plan §4.7).
//!
//! When nothing else is worth buying (it scores lowest, so it gets only what the higher
//! plans have not reserved), and the command cap is not full, the Strategist queues ships
//! in the preferred composition: neutral at A2, by value 30% VX, 40% LANCER and 30% WARDEN,
//! WARDENs only where there is a dry dock (D-068). New ships join the building holding's
//! garrison (D-207): the holding's rally is set to a warship fleet docked there. Holdings
//! are chosen by exposure, nearest a raider haven and most hostiles first. The sign-off sets
//! the replacement list to the garrisons ([`super::signoff`]).
//!
//! *Proposed* readings:
//! - the next class is the one furthest below its share of the warships' value;
//! - the next holding is the one with the least garrison value for its exposure weight
//!   (1 + jumps closer to a haven than 4 + hostiles there), among those with docking room
//!   that can build the class, then by identity;
//! - one ship is queued per evaluation, and none while one waits to be paid.

use nomad_types::{BuildItem, EconomyCommand, FleetId, HoldingId, ShipClass, Stock};

use super::model::{self, Features};
use super::view::{StrategicView, StrategistParams};
use super::{Plan, Turn};
use crate::view::{BuildCheck, FleetPlace};

/// The stand-in's score: its weight while the command cap has room.
pub(crate) fn score(f: &Features, params: &StrategistParams<'_>) -> i64 {
    let room = f
        .economy
        .command_cap
        .saturating_sub(f.economy.command_used + f.command_queued());
    let cheapest = params
        .rules
        .composition_permille
        .keys()
        .filter_map(|&c| f.numbers.ship(c).map(|s| s.1))
        .min()
        .unwrap_or(u32::MAX);
    if room >= cheapest {
        params.rules.weights.garrison
    } else {
        0
    }
}

/// Values by key.
type Values<K> = Vec<(K, u64)>;

/// Warship value per class, and per holding (docked there).
fn values(f: &Features, crystal_weight: u64) -> (Values<ShipClass>, Values<HoldingId>) {
    let mut by_class: Vec<(ShipClass, u64)> = Vec::new();
    let mut by_holding: Vec<(HoldingId, u64)> = Vec::new();
    for fleet in &f.fleets {
        for &(_, class) in &fleet.ships {
            if class == ShipClass::Mule {
                continue;
            }
            let value = f
                .numbers
                .ship(class)
                .map_or(0, |(price, _)| model::weighed(price, crystal_weight));
            match by_class.iter_mut().find(|(c, _)| *c == class) {
                Some(entry) => entry.1 += value,
                None => by_class.push((class, value)),
            }
            if let FleetPlace::Docked(h) = fleet.place {
                match by_holding.iter_mut().find(|(x, _)| *x == h) {
                    Some(entry) => entry.1 += value,
                    None => by_holding.push((h, value)),
                }
            }
        }
    }
    (by_class, by_holding)
}

/// Queues the next garrison ship. Returns whether the cap is full.
pub(crate) fn advance(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    plan: &mut Plan,
    params: &StrategistParams<'_>,
) -> bool {
    let f = turn.f;
    let rules = params.rules;
    if score(f, params) <= 0 {
        return true;
    }
    let waiting = f.holdings.iter().any(|h| {
        h.queue.iter().any(|q| {
            !q.started && matches!(q.item, BuildItem::Ship { class } if class != ShipClass::Mule)
        })
    });
    if waiting {
        plan.observe(1, true, f.now);
        return false;
    }
    let (by_class, by_holding) = values(f, rules.crystal_weight);
    let total: u64 = by_class.iter().map(|c| c.1).sum();
    let dock = f.complete().any(|h| {
        h.modules
            .iter()
            .any(|m| m.kind == nomad_types::ModuleKind::DryDock && !m.building)
    });
    // The class furthest below its share.
    let mut classes: Vec<(i128, ShipClass)> = rules
        .composition_permille
        .iter()
        .filter(|&(&class, _)| dock || class != ShipClass::Warden)
        .map(|(&class, &share)| {
            let have = by_class.iter().find(|c| c.0 == class).map_or(0, |c| c.1);
            let want = u128::from(total) * u128::from(share) / 1_000;
            let gap = i128::try_from(want).unwrap_or(i128::MAX) - i128::from(have);
            (gap, class)
        })
        .collect();
    classes.sort_by_key(|&(gap, class)| (std::cmp::Reverse(gap), class));
    let room = f
        .economy
        .command_cap
        .saturating_sub(f.economy.command_used + turn.command_planned);
    let Some((class, price, command)) = classes.iter().find_map(|&(_, class)| {
        let (price, command) = f.numbers.ship(class)?;
        (command <= room).then_some((class, price, command))
    }) else {
        return true;
    };
    // The holding with the least garrison for its exposure.
    let mut best: Option<(u128, HoldingId)> = None;
    for h in f.complete() {
        if h.docked >= h.docking_capacity {
            continue;
        }
        if view.build_check(h.id, BuildItem::Ship { class }) != BuildCheck::Ok {
            continue;
        }
        let jumps = f.haven_jumps.get(&h.system).copied().unwrap_or(4);
        let weight = 1
            + u128::from(4_u32.saturating_sub(jumps))
            + u128::from(f.hostiles.get(&h.system).copied().unwrap_or(0));
        let value = by_holding.iter().find(|x| x.0 == h.id).map_or(0, |x| x.1);
        let key = u128::from(value) * 1_000 / weight;
        if best.is_none_or(|(k, id)| (key, h.id) < (k, id)) {
            best = Some((key, h.id));
        }
    }
    let Some((_, holding)) = best else {
        plan.observe(2, false, f.now);
        return false;
    };
    // New ships join a warship fleet docked there (D-207).
    let garrison = garrison_fleet(f, holding);
    if let Some(fleet) = garrison
        && f.holding(holding).and_then(|h| h.rally) != Some(fleet)
    {
        turn.send(nomad_types::Command::Economy(EconomyCommand::SetRally {
            holding,
            fleet: Some(fleet),
        }));
    }
    if turn.spend_or_reserve(price) {
        turn.build(holding, BuildItem::Ship { class });
        turn.command_planned += command;
    }
    plan.observe(holding.sim_id().get() ^ total, true, f.now);
    let _ = Stock::ZERO;
    false
}

/// A fleet of warships docked at `holding`, the lowest identity first.
pub(crate) fn garrison_fleet(f: &Features, holding: HoldingId) -> Option<FleetId> {
    f.fleets
        .iter()
        .filter(|fl| fl.place == FleetPlace::Docked(holding))
        .filter(|fl| !f.convoys.contains(&fl.id))
        .find(|fl| fl.ships.iter().any(|&(_, c)| c != ShipClass::Mule))
        .map(|fl| fl.id)
}
