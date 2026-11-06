//! The authoritative Universe state (Design doc 11; D-048, D-049).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::id::{CommanderId, SimIdAllocator};
use crate::rng::RngStreams;
use crate::time::{SeasonClock, Tick, Ticks};
use crate::timers::TimerQueue;

/// What is needed to start a season's Universe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonConfig {
    /// The season seed. Every random stream derives from it (D-044, D-049).
    pub seed: u64,
    /// Universe time (UTC, ms since the Unix epoch) of tick 0.
    pub season_start_unix_ms: i64,
}

/// Work scheduled on the timer queue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduledEvent {
    /// Diagnostic workload: reschedules itself after a random delay averaging
    /// `mean_interval`. Not a game rule (D-051).
    Diagnostic {
        /// Average delay between firings.
        mean_interval: Ticks,
    },
}

/// The complete, authoritative state of one season's Universe.
///
/// Everything needed to continue the simulation is in here, so a snapshot of this value plus
/// the journal of later inputs reproduces the Universe exactly (D-048, D-049).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UniverseState {
    pub(crate) season: SeasonConfig,
    pub(crate) clock: SeasonClock,
    /// The last completed tick.
    pub(crate) tick: Tick,
    pub(crate) ids: SimIdAllocator,
    pub(crate) timers: TimerQueue<ScheduledEvent>,
    pub(crate) rng: RngStreams,
    pub(crate) connected: BTreeSet<CommanderId>,
}

impl UniverseState {
    /// A new Universe at tick 0.
    pub fn new(season: SeasonConfig) -> Self {
        Self {
            season,
            clock: SeasonClock::new(season.season_start_unix_ms),
            tick: Tick::ZERO,
            ids: SimIdAllocator::default(),
            timers: TimerQueue::default(),
            rng: RngStreams::new(season.seed),
            connected: BTreeSet::new(),
        }
    }

    /// The season this Universe belongs to.
    pub const fn season(&self) -> SeasonConfig {
        self.season
    }

    /// The last completed tick.
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Maps ticks to Universe time.
    pub const fn clock(&self) -> SeasonClock {
        self.clock
    }

    /// Commanders whose client is currently connected.
    pub const fn connected(&self) -> &BTreeSet<CommanderId> {
        &self.connected
    }

    /// Number of pending timers.
    pub fn pending_timers(&self) -> usize {
        self.timers.len()
    }
}
