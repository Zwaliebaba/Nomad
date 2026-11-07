//! Season standings and the season result (Design doc 10 §6; D-020, D-041, D-116, D-191 to
//! D-198). Standings and control are public (D-075, doc 16 §2).
//!
//! Only the shapes live here, so the protocol, the server's read model and the client can use
//! them (D-086). How a standing is counted and ranked, when the early win triggers and how
//! the result is fixed are game rules and stay in `nomad-sim`.

use serde::{Deserialize, Serialize};

use crate::deposit::DepositValue;
use crate::id::CommanderId;
use crate::time::Tick;

/// One commander's standing (D-041, D-193).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// Whose standing.
    pub commander: CommanderId,
    /// 1 for the first. Commanders equal on systems, holdings, deposit value and `since`
    /// share a rank (D-194), and the next rank counts them all: 1, 1, 3.
    pub rank: u32,
    /// Claimable systems the commander controls (D-041).
    pub systems: u32,
    /// Holdings with a completed core; outposts do not count (D-041).
    pub holdings: u32,
    /// The deposit value at those holdings' sites (D-066).
    pub deposit_value: DepositValue,
    /// The tick from which systems, holdings and deposit value have been unchanged: the
    /// fourth key, earlier first (D-193).
    pub since: Tick,
}

/// Why the season ended (D-020, D-116).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SeasonEndReason {
    /// A commander controlled the early-win share of the claimable systems (D-192).
    EarlyWin,
    /// The season reached its scheduled final tick (D-195).
    FinalTick,
}

/// The season's result, fixed at its final tick (D-116).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonResult {
    /// The final tick: the scheduled one, or the tick of an early win (D-192).
    pub tick: Tick,
    /// Why the season ended.
    pub reason: SeasonEndReason,
    /// The winners, in identity order: the early winner, or every commander ranked first;
    /// a full tie gives joint winners (D-194).
    pub winners: Vec<CommanderId>,
}
