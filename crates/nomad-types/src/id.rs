//! Identities (Design doc 11 §5, D-049; `CODING_STANDARDS.md` §4 and §8).

use serde::{Deserialize, Serialize};

/// Stable identity of a simulated object, assigned in increasing order and never reused.
///
/// A `hecs` entity handle is only a storage detail. `SimId` is the identity used for
/// persistence, the protocol and deterministic ordering.
///
/// The simulation creates identities only through its allocator (`nomad_sim::SimIdAllocator`),
/// which is part of the Universe state. Everywhere else a `SimId` is decoded from a snapshot,
/// a journal or the wire, or built with [`SimId::from_raw`] in tests and fixtures. A `SimId`
/// says nothing about whether its object exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SimId(u64);

impl SimId {
    /// The identity with raw value `raw`.
    ///
    /// For the simulation's allocator, decoders and tests. Simulation code allocates
    /// identities instead of building them, so that none is ever reused (D-049).
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw value, for logging and the protocol.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identity of a commander, human or AI (Design glossary).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CommanderId(pub u32);

/// Declares a typed identity that wraps a [`SimId`] (`CODING_STANDARDS.md` §4: identifiers
/// are newtypes). The simulation builds one only from an identity its allocator handed out;
/// elsewhere they are decoded, like [`SimId`] itself.
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
        pub struct $name(pub $crate::id::SimId);

        impl $name {
            /// The underlying simulation identity.
            pub const fn sim_id(self) -> $crate::id::SimId {
                self.0
            }
        }
    };
}

sim_id_newtype!(
    /// Identity of a sector (doc 10 §2).
    SectorId
);
sim_id_newtype!(
    /// Identity of a star system (doc 10 §2). Also the scope of per-system random streams
    /// (doc 11 §5).
    SystemId
);
sim_id_newtype!(
    /// Identity of a site, a fixed location in a system (doc 10 §5).
    SiteId
);
sim_id_newtype!(
    /// Identity of a gate at a system's edge (doc 10 §4.1).
    GateId
);
sim_id_newtype!(
    /// Identity of a jump or trunk lane between two gates (doc 10 §4.2).
    LaneId
);
sim_id_newtype!(
    /// Identity of a holding. A holding keeps its identity when captured.
    HoldingId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_value_round_trips_and_orders() {
        let a = SimId::from_raw(1);
        let b = SimId::from_raw(2);
        assert_eq!(a.get(), 1);
        assert!(a < b);
        assert_eq!(SystemId(b).sim_id(), b);
        assert!(SiteId(a) < SiteId(b));
    }
}
