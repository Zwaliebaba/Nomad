//! The authoritative Universe state (Design doc 11; D-048, D-049).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::combat::Projectiles;
use crate::commander::Commanders;
use crate::defence::Defence;
use crate::economy::Economy;
use crate::entities::Entities;
use crate::fleet::FleetId;
use crate::galaxy::{Galaxy, SiteId, SystemId};
use crate::holding::{self, ControlState, Holding, HoldingError, HoldingId};
use crate::id::{CommanderId, SimIdAllocator};
use crate::rng::RngStreams;
use crate::rules::{Rules, RulesError};
use crate::season::SeasonRecord;
use crate::sensors::Sensors;
use crate::siege::Sieges;
use crate::time::{SeasonClock, Tick, Ticks};
use crate::timers::TimerQueue;

/// What is needed to start a season's Universe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonConfig {
    /// The season seed. Every random stream derives from it (D-044, D-049).
    pub seed: u64,
    /// Universe time (UTC, ms since the Unix epoch) of tick 0.
    pub season_start_unix_ms: i64,
    /// The season's scheduled final tick, set at creation from the season length (D-195,
    /// D-106). The step that computes it fixes the result, unless an early win came first
    /// (D-192), and no tick follows it (D-116).
    pub final_tick: Tick,
}

impl SeasonConfig {
    /// The default season length in weeks (D-106); `create` allows 8 to 12 (D-020).
    pub const DEFAULT_WEEKS: u32 = 8;

    /// A season of the default length, [`SeasonConfig::DEFAULT_WEEKS`].
    pub const fn new(seed: u64, season_start_unix_ms: i64) -> Self {
        Self {
            seed,
            season_start_unix_ms,
            final_tick: final_tick_after_weeks(Self::DEFAULT_WEEKS),
        }
    }
}

/// The final tick of a season `weeks` long: the last tick of its last day (D-020, D-116).
pub const fn final_tick_after_weeks(weeks: u32) -> Tick {
    Tick::ZERO.after(Ticks::from_days(7 * weeks as u64))
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
    /// The season's rules, copied in at creation (D-104).
    pub(crate) rules: Rules,
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
    /// Whether the server is catching up after downtime (doc 11 §7, D-047). Set and
    /// cleared only by the journaled [`crate::OperatorInput::CatchUpStart`] and
    /// [`crate::OperatorInput::CatchUpEnd`] inputs, so replay reproduces it.
    pub(crate) catching_up: bool,
    /// Ships and fleets (doc 15 §3; S3 plan §4.2).
    pub(crate) entities: Entities,
    /// The jump-transit queue: fleets in lane transit, by arrival tick (doc 11 §4.1 phase 5,
    /// D-042).
    pub(crate) transits: TimerQueue<FleetId>,
    /// Projectiles in flight (doc 15 §4; S5 plan §4.3).
    pub(crate) projectiles: Projectiles,
    /// Pools, holding economies, convoys and cargo (doc 14; S4). Modules are entities.
    pub(crate) economy: Economy,
    /// Domes, turret state, abilities and the damage ledger (doc 15 §5; S5b).
    pub(crate) defence: Defence,
    /// Commander records: presence, the disconnect grace, activity and exposure windows
    /// (doc 12 §4, §5.3; S6a).
    pub(crate) commanders: Commanders,

    // S8: season standings and the result.
    /// The cached control map, the standings and the result (D-191; S8 plan §4.2).
    pub(crate) season_record: SeasonRecord,
    /// Defence states, protections and emergency jumps (doc 12 §4, doc 15 §6–§7; S6b).
    pub(crate) sieges: Sieges,
    // S7: sensors and fog of war.
    /// Each observer's picture from the last sensors pass, last-known memory and lane
    /// traffic (doc 16 §3–§5; S7).
    pub(crate) sensors: Sensors,
    // A1: the AI.
    /// Every commander's doctrine, and the fleets under direct control (A1 plan §5.1, §5.2).
    pub(crate) doctrine: crate::doctrine::Doctrines,
    /// The memory of every AI layer (A1 plan §4.2; D-199).
    pub(crate) ai: nomad_ai::Memory,
    /// Outboxes, inboxes and per-layer counters (D-200 to D-202).
    pub(crate) ai_runtime: crate::ai::Runtime,
}

impl UniverseState {
    /// A new Universe at tick 0 with an empty galaxy and the embedded rules. Use
    /// [`crate::galaxy::generate_stage1`] for a Universe with a generated galaxy.
    pub fn new(season: SeasonConfig) -> Self {
        Self::create(season, Rules::embedded())
    }

    /// Like [`UniverseState::new`], with other rules, for example a tuning run's (D-104).
    pub fn with_rules(season: SeasonConfig, rules: Rules) -> Result<Self, RulesError> {
        rules.check()?;
        Ok(Self::create(season, rules))
    }

    fn create(season: SeasonConfig, rules: Rules) -> Self {
        Self {
            season,
            rules,
            clock: SeasonClock::new(season.season_start_unix_ms),
            tick: Tick::ZERO,
            ids: SimIdAllocator::default(),
            timers: TimerQueue::default(),
            rng: RngStreams::new(season.seed),
            connected: BTreeSet::new(),
            galaxy: Galaxy::default(),
            holdings: BTreeMap::new(),
            catching_up: false,
            entities: Entities::default(),
            transits: TimerQueue::default(),
            projectiles: Projectiles::default(),
            economy: Economy::default(),
            defence: Defence::default(),
            commanders: Commanders::default(),
            season_record: SeasonRecord::default(),
            sieges: Sieges::default(),
            sensors: Sensors::default(),
            doctrine: crate::doctrine::Doctrines::default(),
            ai: nomad_ai::Memory::default(),
            ai_runtime: crate::ai::Runtime::default(),
        }
    }

    /// The season this Universe belongs to.
    pub const fn season(&self) -> SeasonConfig {
        self.season
    }

    /// The season's rules (D-104).
    pub const fn rules(&self) -> &Rules {
        &self.rules
    }

    /// The last completed tick.
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Maps ticks to Universe time.
    pub const fn clock(&self) -> SeasonClock {
        self.clock
    }

    /// Commanders whose client is currently connected: the journaled `Connected` and
    /// `Disconnected` inputs (doc 11 §6). A human is online exactly while connected; see
    /// [`UniverseState::commander`] for presence.
    pub const fn connected(&self) -> &BTreeSet<CommanderId> {
        &self.connected
    }

    /// Whether this tick runs in catch-up mode: the server is fast-forwarding through
    /// downtime and every commander is offline (doc 11 §7, D-047). Rules that must not act
    /// during catch-up, such as NPC raiders starting raids, read this.
    pub const fn catching_up(&self) -> bool {
        self.catching_up
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
    /// such as a raider haven (doc 10 §6, D-041). Read from the cache phase 9 keeps (D-191);
    /// after a change to completed holdings or their owners earlier in the tick, recounted.
    pub fn control_state(&self, system: SystemId) -> Option<ControlState> {
        self.cached_control_state(system)
    }

    /// The control state of every claimable system, in system order (doc 10 §6, D-041), as
    /// [`UniverseState::control_state`] gives it.
    pub fn control_states(&self) -> BTreeMap<SystemId, ControlState> {
        self.cached_control_states()
    }

    /// Places a holding, or an outpost if `core_complete` is false, on a free, claimable
    /// site. Callers are the generator, claims ([`crate::economy`], D-135) and tests; the
    /// economy adds the core module.
    pub(crate) fn insert_holding(
        &mut self,
        site: SiteId,
        owner: CommanderId,
        core_complete: bool,
    ) -> Result<HoldingId, HoldingError> {
        let id = holding::insert(
            &self.galaxy,
            &mut self.ids,
            &mut self.holdings,
            Holding {
                site,
                owner,
                core_complete,
                mother_station: false,
            },
        )?;
        if core_complete {
            crate::season::mark_dirty(self);
        }
        Ok(id)
    }
}
