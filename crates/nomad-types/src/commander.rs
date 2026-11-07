//! Commanders and exposure windows: shapes only (Design doc 12 §4; D-032, D-077, D-155,
//! D-157; S6 plan §4.1, §4.3).
//!
//! Whether a commander is human or AI, and the quarter hour of the UTC day at which an
//! exposure window starts. How a window follows activity, when a commander may choose it and
//! when it moves are rules for the simulation (S6), not for this crate.

use serde::{Deserialize, Serialize};

/// Whether a human or the AI plays a commander (D-077, doc 13 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CommanderKind {
    /// A human player, online while their client is connected (doc 11 §6).
    Human,
    /// An AI commander, online by its seeded schedule (D-080).
    Ai,
}

/// How competent an AI commander is (D-079): reaction latency, command rate and the
/// Strategist's interval, never resources or information (D-027). Normal is the default.
/// Until A5, `create --difficulty` sets one difficulty for every AI commander (D-231).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Difficulty {
    /// Slow reactions, few commands.
    Easy,
    /// The MVP default.
    #[default]
    Normal,
    /// Close to the human command cap.
    Hard,
}

/// A quarter hour of the UTC day: index 0 is 00:00–00:15 UTC and index 95 is 23:45–24:00
/// UTC.
///
/// Exposure windows start on a quarter hour, the granularity of the activity history
/// (D-155). Decoding accepts any `u8`, because the value can come from the network; the
/// simulation's validator rejects an index that is not a quarter hour of the day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct QuarterHour(u8);

impl QuarterHour {
    /// Quarter hours in a day.
    pub const PER_DAY: u8 = 96;
    /// Minutes in a quarter hour.
    pub const MINUTES: u32 = 15;
    /// Milliseconds in a quarter hour.
    pub const MILLIS: i64 = 900_000;

    /// The quarter hour with this index, if the index is below [`Self::PER_DAY`].
    pub const fn new(index: u8) -> Option<Self> {
        if index < Self::PER_DAY {
            Some(Self(index))
        } else {
            None
        }
    }

    /// The quarter hour that starts at `hours:minutes` UTC, if that is a time of day on a
    /// quarter hour.
    pub const fn at(hours: u8, minutes: u8) -> Option<Self> {
        if hours >= 24 || minutes >= 60 || !minutes.is_multiple_of(15) {
            return None;
        }
        Some(Self(hours * 4 + minutes / 15))
    }

    /// The index as decoded. It is below [`Self::PER_DAY`] unless the value came from
    /// outside unchecked; see [`Self::is_valid`].
    pub const fn index(self) -> u8 {
        self.0
    }

    /// Whether the index is a quarter hour of the day.
    pub const fn is_valid(self) -> bool {
        self.0 < Self::PER_DAY
    }

    /// Minutes after midnight UTC.
    pub const fn minutes_of_day(self) -> u32 {
        self.0 as u32 * Self::MINUTES
    }

    /// Milliseconds after midnight UTC.
    pub const fn time_of_day_ms(self) -> i64 {
        self.0 as i64 * Self::MILLIS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_hours_cover_the_day() {
        assert_eq!(
            QuarterHour::new(0).map(QuarterHour::minutes_of_day),
            Some(0)
        );
        assert_eq!(
            QuarterHour::new(95).map(QuarterHour::minutes_of_day),
            Some(1_425)
        );
        assert_eq!(QuarterHour::new(96), None);
        assert_eq!(QuarterHour::at(19, 0), QuarterHour::new(76));
        assert_eq!(QuarterHour::at(23, 45), QuarterHour::new(95));
        assert_eq!(QuarterHour::at(19, 10), None);
        assert_eq!(QuarterHour::at(24, 0), None);
        assert_eq!(
            QuarterHour::at(19, 0).map(QuarterHour::time_of_day_ms),
            Some(19 * 3_600_000)
        );
        // What decoding can produce from the network.
        assert!(!QuarterHour(200).is_valid());
        assert!(QuarterHour(95).is_valid());
    }
}
