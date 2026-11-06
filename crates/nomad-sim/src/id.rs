//! Stable identities (Design doc 11 §5, D-049).
//!
//! [`SimId`] and [`CommanderId`] live in `nomad-types` (D-086, doc 20 §3) and are re-exported
//! here. The allocator stays in the simulation: it is part of the Universe state, and only
//! the simulation hands out identities.

pub use nomad_types::{CommanderId, SimId};

use serde::{Deserialize, Serialize};

/// Hands out [`SimId`]s. It is part of the Universe state, so it survives snapshots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimIdAllocator {
    next: u64,
}

impl Default for SimIdAllocator {
    fn default() -> Self {
        // 0 is reserved so that a zeroed value is never a valid identity.
        Self { next: 1 }
    }
}

impl SimIdAllocator {
    /// Allocates the next identity.
    pub fn allocate(&mut self) -> SimId {
        let id = SimId::from_raw(self.next);
        self.next = self
            .next
            .checked_add(1)
            .expect("invariant: the u64 SimId space is never exhausted");
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_increase_and_start_at_one() {
        let mut ids = SimIdAllocator::default();
        let a = ids.allocate();
        let b = ids.allocate();
        assert_eq!(a.get(), 1);
        assert!(b > a);
    }
}
