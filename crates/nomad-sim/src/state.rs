//! The authoritative Universe state (Design doc 11; D-048, D-049).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::galaxy::{Galaxy, SiteId, SystemId};
use crate::holding::{self, ControlState, Holding, HoldingError, HoldingId};
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
    /// The season galaxy's topology, fixed for the season (doc 10, D-044).
    pub(crate) galaxy: Galaxy,
    /// Every holding and outpost, by identity (doc 10 §5, D-041).
    pub(crate) holdings: BTreeMap<HoldingId, Holding>,
}

impl UniverseState {
    /// A new Universe at tick 0 with an empty galaxy. Use
    /// [`crate::galaxy::generate_stage1`] for a Universe with a generated galaxy.
    pub fn new(season: SeasonConfig) -> Self {
        Self {
            season,
            clock: SeasonClock::new(season.season_start_unix_ms),
            tick: Tick::ZERO,
            ids: SimIdAllocator::default(),
            timers: TimerQueue::default(),
            rng: RngStreams::new(season.seed),
            connected: BTreeSet::new(),
            galaxy: Galaxy::default(),
            holdings: BTreeMap::new(),
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

    /// The season galaxy (doc 10).
    pub const fn galaxy(&self) -> &Galaxy {
        &self.galaxy
    }

    /// Every holding and outpost, in identity order.
    pub fn holdings(&self) -> impl Iterator<Item = (HoldingId, &Holding)> {
        self.holdings.iter().map(|(&id, holding)| (id, holding))
    }

    /// A holding by identity.
    pub fn holding(&self, id: HoldingId) -> Option<&Holding> {
        self.holdings.get(&id)
    }

    /// The holding or outpost on `site`, if any.
    pub fn holding_at(&self, site: SiteId) -> Option<(HoldingId, &Holding)> {
        self.holdings().find(|(_, holding)| holding.site == site)
    }

    /// The control state of `system`, or `None` if it does not exist or cannot be claimed,
    /// such as a raider haven (doc 10 §6, D-041).
    pub fn control_state(&self, system: SystemId) -> Option<ControlState> {
        holding::control_state(&self.galaxy, &self.holdings, system)
    }

    /// The control state of every claimable system, in system order (doc 10 §6, D-041).
    pub fn control_states(&self) -> BTreeMap<SystemId, ControlState> {
        holding::control_states(&self.galaxy, &self.holdings)
    }

    /// Places a holding, or an outpost if `core_complete` is false, on a free, claimable
    /// site. Claim costs and timing are doc 15 rules; callers here are the generator and
    /// tests.
    pub(crate) fn insert_holding(
        &mut self,
        site: SiteId,
        owner: CommanderId,
        core_complete: bool,
    ) -> Result<HoldingId, HoldingError> {
        holding::insert(
            &self.galaxy,
            &mut self.ids,
            &mut self.holdings,
            Holding {
                site,
                owner,
                core_complete,
            },
        )
    }
}
