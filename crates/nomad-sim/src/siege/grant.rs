//! The comeback grant (D-034, D-167; S6 plan §4.6; S8 plan §4.7).
//!
//! **The grant** (D-167): the cost of one core kit for each holding the commander is behind
//! the standings leader after the fall; at least 1 kit and at most 3; paid into the pool,
//! ignoring the storage cap. *Derived:* "the cost of one core kit" is what a kit queued by
//! the commander would cost right after the fall, with D-189's growth for their size.
//!
//! **The standings leader** is S8's: [`crate::UniverseState::standings_now`] counts the
//! standings from the holdings at this point of the tick, after the station has left its site
//! and before phase 9 (S8 plan §4.2), and [`crate::Standings::leader_holdings`] gives the
//! leader's holdings, the most any joint leader holds (S8 plan §4.7). Holdings are completed
//! holdings only; the fallen station in its jump is none (S8 plan §4.1).

use crate::economy::Stock;
use crate::id::CommanderId;
use crate::state::UniverseState;

/// The comeback grant of `commander`, whose mother station has just left its site (D-167).
pub(super) fn comeback_grant(state: &UniverseState, commander: CommanderId) -> Stock {
    let rules = &state.rules.sieges;
    let standings = state.standings_now();
    let own = standings.of(commander).map_or(0, |s| s.holdings);
    let behind = standings.leader_holdings().saturating_sub(own);
    let kits = u64::from(behind.clamp(rules.comeback_min_kits, rules.comeback_max_kits));
    let price = state.core_kit_price(commander);
    Stock {
        alloy: price.alloy.saturating_mul(kits),
        crystal: price.crystal.saturating_mul(kits),
    }
}
