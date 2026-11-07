//! The comeback grant and the standings leader it is measured against (D-034, D-041, D-167;
//! S6 plan §4.6; S8 plan §4.1, §4.7).
//!
//! **The grant** (D-167): the cost of one core kit for each holding the commander is behind
//! the standings leader after the fall; at least 1 kit and at most 3; paid into the pool,
//! ignoring the storage cap. *Derived:* "the cost of one core kit" is what a kit queued by
//! the commander would cost right after the fall, with D-189's growth for their size.
//!
//! **The standings leader** — the interface for the S8 merge. S8 (built in parallel) puts the
//! standings in the simulation state (D-191) and its plan §4.7 defines the leader for this
//! grant: the commander ranked first after the fall; with joint leaders, the one with the
//! most holdings. Until the merge, [`leader_holdings`] computes the same number straight
//! from the holdings: the leader ranks first by systems controlled, then by holdings owned
//! (D-041). The later tie-breaks (deposit value, *since*) only choose among commanders with
//! equal holdings, so they cannot change the count. Holdings owned are completed holdings
//! only, the fallen station in its jump being none (S8 plan §4.1).
//!
//! At the merge, [`leader_holdings`] and [`holdings_owned`] read S8's standings view (its
//! pure function, which S8 plan §4.2 provides for exactly this "after the fall" moment in
//! phase 7) instead of recounting.

use std::collections::BTreeMap;

use crate::economy::Stock;
use crate::holding::ControlState;
use crate::id::CommanderId;
use crate::state::UniverseState;

/// The completed holdings `commander` owns: S8's "holdings owned" (D-041; S8 plan §4.1).
pub(crate) fn holdings_owned(state: &UniverseState, commander: CommanderId) -> u32 {
    let n = state
        .holdings
        .values()
        .filter(|h| h.owner == commander && h.core_complete)
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The holdings owned by the standings leader: the commander first by systems controlled,
/// then by holdings owned (D-041; S8 plan §4.7). 0 if nobody holds anything. **The S8
/// interface:** S8's standings view replaces this count at the merge.
pub(crate) fn leader_holdings(state: &UniverseState) -> u32 {
    let mut systems: BTreeMap<CommanderId, u32> = BTreeMap::new();
    for control in state.control_states().into_values() {
        if let ControlState::Controlled(c) = control {
            *systems.entry(c).or_default() += 1;
        }
    }
    let mut commanders: Vec<CommanderId> = state.commanders.records.keys().copied().collect();
    commanders.extend(state.holdings.values().map(|h| h.owner));
    commanders.sort_unstable();
    commanders.dedup();
    commanders
        .into_iter()
        .map(|c| {
            (
                systems.get(&c).copied().unwrap_or(0),
                holdings_owned(state, c),
            )
        })
        .max()
        .map_or(0, |(_, holdings)| holdings)
}

/// The comeback grant of `commander`, whose mother station has just left its site (D-167).
pub(super) fn comeback_grant(state: &UniverseState, commander: CommanderId) -> Stock {
    let rules = &state.rules.sieges;
    let behind = leader_holdings(state).saturating_sub(holdings_owned(state, commander));
    let kits = u64::from(behind.clamp(rules.comeback_min_kits, rules.comeback_max_kits));
    let price = state.core_kit_price(commander);
    Stock {
        alloy: price.alloy.saturating_mul(kits),
        crystal: price.crystal.saturating_mul(kits),
    }
}
