//! Seeded random streams (Design doc 11 §5, D-049).
//!
//! Each stream is identified by a [`StreamKey`] (a scope such as a star system, and a purpose)
//! and seeded from `(season_seed, scope, purpose)`. Results therefore do not depend on the
//! order in which scopes are processed. Streams are stored as `(key, word position)`, so
//! snapshots stay small and canonical.

use std::collections::BTreeMap;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

/// Well-known stream purposes.
pub mod purpose {
    /// The diagnostic workload used by pacing and determinism tests.
    pub const DIAGNOSTICS: u32 = 1;
    /// Season galaxy layout at season start: site and gate positions (per-system scope) and
    /// lane transit times (global scope). Design doc 10 §4, D-044.
    pub const GALAXY_LAYOUT: u32 = 2;
    /// Season galaxy deposits at season start, per system (doc 14 §5, D-066).
    pub const GALAXY_DEPOSITS: u32 = 3;
    /// Where ships arriving through a gate appear, per destination system (D-055).
    pub const ARRIVAL_SCATTER: u32 = 4;
    /// Shot spread, per system, drawn in (ship `SimId`, weapon index) order (doc 15 §4).
    pub const COMBAT: u32 = 5;

    // S6b: sieges. 60–69 are kept for S6, apart from the purposes other packages add.
    /// A capture tie after core and total damage, per holding (D-229).
    pub const SIEGE_TIES: u32 = 60;
}

/// Identifies one random stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StreamKey {
    /// The scope, for example a star system's raw id. [`StreamKey::GLOBAL_SCOPE`] is
    /// Universe-wide.
    pub scope: u64,
    /// What the stream is used for (see [`purpose`]).
    pub purpose: u32,
}

impl StreamKey {
    /// The Universe-wide scope.
    pub const GLOBAL_SCOPE: u64 = 0;

    /// A Universe-wide stream for `purpose`.
    pub const fn global(purpose: u32) -> Self {
        Self {
            scope: Self::GLOBAL_SCOPE,
            purpose,
        }
    }
}

/// All random streams of a season, created on first use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "RngStreamsRepr", into = "RngStreamsRepr")]
pub struct RngStreams {
    season_seed: u64,
    streams: BTreeMap<StreamKey, ChaCha8Rng>,
}

impl RngStreams {
    /// Streams for a season seeded with `season_seed`.
    pub fn new(season_seed: u64) -> Self {
        Self {
            season_seed,
            streams: BTreeMap::new(),
        }
    }

    /// The next raw 64-bit value of a stream.
    pub fn next_u64(&mut self, key: StreamKey) -> u64 {
        self.stream(key).next_u64()
    }

    /// A uniform value in `0..bound`, without bias. Returns 0 when `bound` is 0.
    ///
    /// This is our own algorithm (rejection sampling), so results do not change when a
    /// dependency changes its sampling method.
    pub fn below(&mut self, key: StreamKey, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        // Reject the lowest (2^64 mod bound) values so every residue is equally likely.
        let threshold = bound.wrapping_neg() % bound;
        let rng = self.stream(key);
        loop {
            let x = rng.next_u64();
            if x >= threshold {
                return x % bound;
            }
        }
    }

    /// A uniform index in `0..len`. Returns 0 when `len` is 0.
    pub fn index(&mut self, key: StreamKey, len: usize) -> usize {
        let bound = u64::try_from(len).unwrap_or(u64::MAX);
        usize::try_from(self.below(key, bound))
            .expect("invariant: a value below a usize bound fits in usize")
    }

    /// A uniform value in `[0, 1)` with 53 random bits.
    ///
    /// Built from the raw stream with exact operations only (a shift and a division by a
    /// power of two), so the result is the same on every platform (doc 11 §5).
    pub fn unit_f64(&mut self, key: StreamKey) -> f64 {
        const SCALE: f64 = 9_007_199_254_740_992.0; // 2^53
        (self.next_u64(key) >> 11) as f64 / SCALE
    }

    fn stream(&mut self, key: StreamKey) -> &mut ChaCha8Rng {
        let season_seed = self.season_seed;
        self.streams
            .entry(key)
            .or_insert_with(|| ChaCha8Rng::from_seed(derive_seed(season_seed, key)))
    }
}

/// `season_seed (8) | scope (8) | purpose (4) | domain tag (12)`, little-endian.
fn derive_seed(season_seed: u64, key: StreamKey) -> [u8; 32] {
    const DOMAIN: [u8; 12] = *b"nomad-rng-v1";
    let mut seed = [0_u8; 32];
    seed[..8].copy_from_slice(&season_seed.to_le_bytes());
    seed[8..16].copy_from_slice(&key.scope.to_le_bytes());
    seed[16..20].copy_from_slice(&key.purpose.to_le_bytes());
    seed[20..].copy_from_slice(&DOMAIN);
    seed
}

#[derive(Clone, Serialize, Deserialize)]
struct RngStreamsRepr {
    season_seed: u64,
    positions: Vec<(StreamKey, u128)>,
}

impl From<RngStreams> for RngStreamsRepr {
    fn from(streams: RngStreams) -> Self {
        Self {
            season_seed: streams.season_seed,
            positions: streams
                .streams
                .iter()
                .map(|(key, rng)| (*key, rng.get_word_pos()))
                .collect(),
        }
    }
}

impl From<RngStreamsRepr> for RngStreams {
    fn from(repr: RngStreamsRepr) -> Self {
        let streams = repr
            .positions
            .into_iter()
            .map(|(key, pos)| {
                let mut rng = ChaCha8Rng::from_seed(derive_seed(repr.season_seed, key));
                rng.set_word_pos(pos);
                (key, rng)
            })
            .collect();
        Self {
            season_seed: repr.season_seed,
            streams,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: StreamKey = StreamKey {
        scope: 7,
        purpose: 1,
    };
    const B: StreamKey = StreamKey {
        scope: 8,
        purpose: 1,
    };

    #[test]
    fn streams_are_independent_of_use_order() {
        let mut one = RngStreams::new(42);
        let a1 = one.next_u64(A);
        let b1 = one.next_u64(B);
        let mut two = RngStreams::new(42);
        let b2 = two.next_u64(B);
        let a2 = two.next_u64(A);
        assert_eq!((a1, b1), (a2, b2));
        assert_ne!(a1, b1);
    }

    #[test]
    fn below_stays_in_range() {
        let mut s = RngStreams::new(1);
        assert_eq!(s.below(A, 0), 0);
        assert!((0..1_000).all(|_| s.below(A, 7) < 7));
    }

    #[test]
    fn index_and_unit_stay_in_range() {
        let mut s = RngStreams::new(3);
        assert_eq!(s.index(A, 0), 0);
        assert!((0..1_000).all(|_| s.index(A, 5) < 5));
        assert!((0..1_000).all(|_| (0.0..1.0).contains(&s.unit_f64(A))));
    }

    #[test]
    fn serde_round_trip_preserves_position() {
        let mut s = RngStreams::new(9);
        for _ in 0..5 {
            s.next_u64(A);
        }
        let bytes = postcard::to_stdvec(&s).unwrap();
        let mut restored: RngStreams = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(restored, s);
        assert_eq!(restored.next_u64(A), s.next_u64(A));
    }
}
