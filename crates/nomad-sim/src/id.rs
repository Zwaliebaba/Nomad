//! Stable identities (Design doc 11 §5, D-049).

use serde::{Deserialize, Serialize};

/// Stable identity of a simulated object, assigned in increasing order and never reused.
///
/// A `hecs` entity handle is only a storage detail. `SimId` is the identity used for
/// persistence, the protocol and deterministic ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SimId(u64);

impl SimId {
    /// The raw value, for logging and the protocol.
    pub const fn get(self) -> u64 {
        self.0
    }
}

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
        let id = SimId(self.next);
        self.next = self
            .next
            .checked_add(1)
            .expect("invariant: the u64 SimId space is never exhausted");
        id
    }
}

/// Identity of a commander, human or AI (Design glossary).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CommanderId(pub u32);

/// Declares a typed identity that wraps a [`SimId`] (`CODING_STANDARDS.md` §4: identifiers
/// are newtypes). Only the module that declares it, and its descendants, can create one, so
/// every value comes from the Universe's [`SimIdAllocator`].
macro_rules! sim_id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone,
            Copy,
            Debug,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        pub struct $name($crate::id::SimId);

        impl $name {
            /// The underlying simulation identity.
            pub const fn sim_id(self) -> $crate::id::SimId {
                self.0
            }
        }
    };
}
pub(crate) use sim_id_newtype;

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
