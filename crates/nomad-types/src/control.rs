//! System control states (Design doc 10 §6, D-041). Control is public (D-075, doc 16 §2).

use std::collections::BTreeMap;

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

/// The number of completed holdings needed to control a system with `site_count` sites: a
/// strict majority, ⌊n/2⌋+1 (D-041). 4 of 7, 4 of 6, 5 of 8.
pub const fn majority_threshold(site_count: usize) -> usize {
    site_count / 2 + 1
}

impl ControlState {
    /// The control state of a system with `site_count` sites, given how many **completed**
    /// holdings each commander owns there (D-041).
    pub fn from_counts(site_count: usize, completed: &BTreeMap<CommanderId, usize>) -> Self {
        let threshold = majority_threshold(site_count);
        // At most one commander can hold a strict majority of a system's sites; iterating in
        // commander order keeps the answer defined even for inconsistent counts.
        if let Some((&owner, _)) = completed.iter().find(|&(_, &n)| n >= threshold) {
            return Self::Controlled(owner);
        }
        if completed.values().filter(|&&n| n > 0).count() >= 2 {
            Self::Contested
        } else {
            Self::Open
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YOU: CommanderId = CommanderId(1);
    const HOSTILE: CommanderId = CommanderId(2);

    fn counts(pairs: &[(CommanderId, usize)]) -> BTreeMap<CommanderId, usize> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn majority_is_strict() {
        assert_eq!(majority_threshold(7), 4);
        assert_eq!(majority_threshold(6), 4);
        assert_eq!(majority_threshold(8), 5);
        assert_eq!(majority_threshold(9), 5);
    }

    #[test]
    fn from_counts_covers_every_state() {
        // Phase-1 Kestrel-7: "YOU 3 · HSTL 2 · FREE 2" is CONTESTED (doc 10 §6).
        assert_eq!(
            ControlState::from_counts(7, &counts(&[(YOU, 3), (HOSTILE, 2)])),
            ControlState::Contested
        );
        assert_eq!(
            ControlState::from_counts(7, &counts(&[(YOU, 4), (HOSTILE, 2)])),
            ControlState::Controlled(YOU)
        );
        // Half is not a majority.
        assert_eq!(
            ControlState::from_counts(6, &counts(&[(YOU, 3), (HOSTILE, 3)])),
            ControlState::Contested
        );
        assert_eq!(
            ControlState::from_counts(6, &counts(&[(YOU, 3)])),
            ControlState::Open
        );
        assert_eq!(
            ControlState::from_counts(7, &counts(&[])),
            ControlState::Open
        );
        assert_eq!(
            ControlState::from_counts(7, &counts(&[(YOU, 0), (HOSTILE, 1)])),
            ControlState::Open
        );
    }
}
