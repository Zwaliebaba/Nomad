//! System control states (Design doc 10 §6, D-041). Control is public (D-075, doc 16 §2).
//!
//! Only the shape lives here. The majority rule that derives a state from holdings is a game
//! rule and stays in `nomad-sim` (D-086: the client links no rules).

use serde::{Deserialize, Serialize};

use crate::id::CommanderId;

/// Who controls a claimable system (doc 10 §6, D-041). Derived from holdings whenever it is
/// needed, never stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ControlState {
    /// This commander owns a strict majority of the system's sites as completed holdings.
    Controlled(CommanderId),
    /// Nobody has a majority, and at least two commanders own completed holdings there.
    Contested,
    /// Nobody has a majority, and at most one commander owns completed holdings there.
    Open,
}
