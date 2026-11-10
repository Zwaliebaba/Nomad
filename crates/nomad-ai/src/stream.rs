//! The AI's random streams (A1 plan §4.6; D-049, doc 13 §2 "seeded per-commander streams").
//!
//! One stream per (commander, layer) and per (haven, raids). The simulation derives each
//! stream's seed from `(season_seed, scope, purpose)` as doc 11 §5 does for its own streams,
//! with the AI purposes, and hands the seed to the AI. The stream then lives in AI memory
//! (A1 plan §4.2), so its draws never depend on another commander's activity, and a
//! snapshot keeps it as its seed and word position.
//!
//! The draws are our own algorithms over the raw 64-bit output, as `nomad-sim`'s are, so
//! results do not change when the dependency changes its sampling.

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

/// A seeded random stream (doc 11 §5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "StreamRepr", into = "StreamRepr")]
pub struct Stream {
    seed: [u8; 32],
    rng: ChaCha8Rng,
}

impl Stream {
    /// A stream from the seed the simulation derived for it.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            seed,
            rng: ChaCha8Rng::from_seed(seed),
        }
    }

    /// The next raw 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        self.rng.next_u64()
    }

    /// A uniform value in `0..bound`, without bias (rejection sampling); 0 when `bound` is 0.
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let x = self.rng.next_u64();
            if x >= threshold {
                return x % bound;
            }
        }
    }

    /// A uniform value in `lo..=hi`; `lo` when `hi < lo`.
    pub fn between(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.below(hi - lo + 1)
    }

    /// A uniform value in `-spread..=spread`.
    pub fn spread(&mut self, spread: u32) -> i64 {
        let width = u64::from(spread) * 2 + 1;
        i64::try_from(self.below(width)).unwrap_or(0) - i64::from(spread)
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct StreamRepr {
    seed: [u8; 32],
    position: u128,
}

impl From<Stream> for StreamRepr {
    fn from(stream: Stream) -> Self {
        Self {
            seed: stream.seed,
            position: stream.rng.get_word_pos(),
        }
    }
}

impl From<StreamRepr> for Stream {
    fn from(repr: StreamRepr) -> Self {
        let mut rng = ChaCha8Rng::from_seed(repr.seed);
        rng.set_word_pos(repr.position);
        Self {
            seed: repr.seed,
            rng,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_stay_in_range_and_a_round_trip_keeps_the_position() {
        let mut s = Stream::from_seed([7; 32]);
        assert!((0..500).all(|_| s.below(7) < 7));
        assert!((0..500).all(|_| (3..=5).contains(&s.between(3, 5))));
        assert!((0..500).all(|_| (-2..=2).contains(&s.spread(2))));
        assert_eq!(s.between(4, 4), 4);
        let bytes = serde_json::to_string(&s).unwrap();
        let mut restored: Stream = serde_json::from_str(&bytes).unwrap();
        assert_eq!(restored, s);
        assert_eq!(restored.next_u64(), s.next_u64());
    }
}
