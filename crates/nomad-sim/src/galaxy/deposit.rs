//! Deposits on sites (Design doc 14 §5, D-066).
//!
//! A site has 0–2 deposits. Each is an ore field or a crystal vein of poor, standard or rich
//! richness, and never runs out: deposits limit the extraction rate, not a quantity. Deposit
//! value feeds doc 10 invariant 5 (balanced home neighbourhoods) and the D-041 season
//! tie-break. Harvesting and extraction rates are doc 14 §6 and are not modelled here.
//!
//! The deposit types are public data (doc 16 §2), so they live in `nomad-types` (D-086,
//! doc 20 §3) and are re-exported here. How many deposits a site may have is a generator
//! rule and stays in the simulation.

pub use nomad_types::{Deposit, DepositKind, DepositValue, Richness};

/// At most this many deposits on one site (D-066).
pub const MAX_DEPOSITS_PER_SITE: usize = 2;
