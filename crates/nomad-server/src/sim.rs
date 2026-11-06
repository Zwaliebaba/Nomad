//! The simulation thread: catch-up, live pacing and tick statistics (N2 plan §4.4–4.5;
//! Design doc 11 §7; doc 20 §4, §7, §10).
//!
//! The thread owns the `UniverseState`. It never reads a disk or the network: journal rows
//! and snapshots go to the persistence thread over a channel.
//!
//! - **Catch-up** *(Derived from doc 11 §7, D-047):* when the Universe is behind the wall
//!   clock, the thread fast-forwards with empty inputs to the tick the wall clock maps to.
//!   The first catch-up tick carries `CatchUpStart` and the target tick carries
//!   `CatchUpEnd`, both journaled, so the simulation sees the mode and replay is identical.
//!   The target tick itself therefore runs live, and the Universe ends catch-up at exactly
//!   the state an uninterrupted run would have. If the wall clock moved on by more than a
//!   second meanwhile, catch-up continues to the new target first.
//! - **Live:** one tick per 100 ms against the monotonic clock. Overruns run back to back,
//!   with a warning when the thread is more than 10 ticks behind. *Derived* from doc 11 §2
//!   (Universe time is UTC): if the Universe falls more than a minute behind the wall
//!   clock, as after the computer slept, the thread catches up again.
//! - **Snapshots** every 3,000 ticks, at ticks that are multiples of the interval so that
//!   retention's hours and days line up, and a final one on a clean stop.
//! - **Journal commits** at least once per 10 ticks (1 s) while rows are pending (D-048).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::time::Duration;

use nomad_sim::snapshot::{self, encode, state_hash};
use nomad_sim::{
    OperatorInput, SeasonClock, TICK_MILLIS, Tick, TickInputs, TickOutputs, Ticks, UniverseState,
    step,
};
use tracing::{debug, info, warn};

use crate::clock::{Alarm, Clock};
use crate::error::ServerError;
use crate::journal;
use crate::persist::PersistMessage;
use crate::server::{CatchUpProgress, Input, Phase, ServerEvent, Shared};
use crate::store::SnapshotRecord;
use crate::timefmt::{duration_text, ticks_text};

/// Nanoseconds per tick.
const TICK_NANOS: i128 = TICK_MILLIS as i128 * 1_000_000;
/// Warn when live ticks fall more than this far behind the monotonic clock (N2 plan §4.4).
const BEHIND_WARNING_TICKS: u64 = 10;
/// At startup, catch up when the Universe is more than 1 s behind the wall clock. Smaller
/// gaps run back to back as live ticks.
const STARTUP_CATCH_UP_TICKS: u64 = 10;
/// While live, catch up when the Universe is more than 1 min behind the wall clock.
const LIVE_CATCH_UP_TICKS: u64 = 600;
/// How often, in ticks, live play compares the Universe with the wall clock.
const WALL_CHECK_TICKS: u64 = 10;
/// Catch-up is done when the wall clock moved at most this many ticks past the target.
const SETTLED_TICKS: u64 = 10;
/// Pending journal rows are committed once they span this many ticks (1 s, D-048).
const COMMIT_TICKS: u64 = 10;
/// Tick-time statistics are logged every this many live ticks (1 min, doc 20 §10).
const STATS_TICKS: usize = 600;
/// During catch-up, the inbox is read every this many ticks.
const CATCH_UP_POLL_TICKS: u64 = 1_024;
/// Catch-up progress is published this often (wall time) …
const PROGRESS_EVENT_INTERVAL: Duration = Duration::from_secs(1);
/// … and logged this often.
const PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(10);

// The catch-up markers need at least one tick between them.
const _: () = assert!(STARTUP_CATCH_UP_TICKS >= 2 && LIVE_CATCH_UP_TICKS >= 2);

/// Requests to the simulation thread.
#[derive(Debug)]
pub(crate) enum Control {
    /// An input for the next tick.
    Input(Input),
    /// Commit the journal of every computed tick, then reply.
    Flush(SyncSender<Result<(), String>>),
    /// Reply with the current tick and state hash.
    StateHash(SyncSender<Result<(Tick, u64), String>>),
    /// Stop cleanly: final snapshot, flush.
    Shutdown,
    /// Stop at once without a final snapshot, dropping the uncommitted journal batch.
    Abort,
}

/// How the thread stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Stop {
    /// Final snapshot written and journal flushed.
    Clean,
    /// Stopped like a crash.
    Abort,
}

/// Where live pacing is anchored: tick `tick` was due at monotonic time `nanos`.
#[derive(Clone, Copy, Debug)]
struct Anchor {
    tick: Tick,
    nanos: i128,
}

/// The simulation thread's state.
pub(crate) struct Sim {
    pub state: UniverseState,
    pub clock: Arc<dyn Clock>,
    pub alarm: Arc<Alarm>,
    pub inbox: Receiver<Control>,
    pub persist: SyncSender<PersistMessage>,
    pub shared: Arc<Shared>,
    pub build: String,
    pub snapshot_interval: Ticks,
    /// The last tick of the season (D-020).
    pub season_end: Tick,
}

/// The running loop, built from [`Sim`].
struct Runner {
    sim: Sim,
    snapshot_interval: u64,
    /// Inputs for the next tick.
    pending: TickInputs,
    /// First tick of the journal rows sent but not yet committed.
    batch_start: Option<Tick>,
    stop: Option<Stop>,
    anchor: Anchor,
    stats: TickStats,
    last_behind_warning: Option<Tick>,
}

impl Sim {
    /// Runs the thread to the end and reports how it stopped.
    pub fn run(self) -> Result<Stop, ServerError> {
        let snapshot_interval = self.snapshot_interval.get().max(1);
        let mut runner = Runner {
            sim: self,
            snapshot_interval,
            pending: TickInputs::default(),
            batch_start: None,
            stop: None,
            anchor: Anchor {
                tick: Tick::ZERO,
                nanos: 0,
            },
            stats: TickStats::default(),
            last_behind_warning: None,
        };
        let result = runner.run_phases().and_then(|()| runner.finish());
        if result.is_err() {
            // Commit what is valid; the persistence thread may already be gone.
            let _ = runner.sim.persist.send(PersistMessage::Stop);
        }
        // Events go out before the status changes, so that whoever waits for a status finds
        // the matching event already queued.
        runner.sim.shared.publish(&ServerEvent::Stopped);
        runner
            .sim
            .shared
            .set_phase(Phase::Stopped, runner.sim.state.tick());
        result.map(|()| runner.stop.unwrap_or(Stop::Clean))
    }
}

impl Runner {
    fn run_phases(&mut self) -> Result<(), ServerError> {
        let tick = self.sim.state.tick();
        if tick >= self.sim.season_end {
            warn_season_over(tick);
        }
        let target = self.wall_target();
        if target.get() > tick.get() + STARTUP_CATCH_UP_TICKS {
            self.catch_up()?;
        } else {
            if target < tick {
                warn!(
                    tick = tick.get(),
                    wall_clock_tick = target.get(),
                    "the wall clock is behind the Universe; it stays ahead"
                );
            }
            if self.sim.state.catching_up() {
                // Stopped right at the end of a catch-up: end the mode on the next tick.
                self.pending.operator.insert(0, OperatorInput::CatchUpEnd);
            }
        }
        if self.stop.is_none() {
            self.live()?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), ServerError> {
        match self.stop {
            Some(Stop::Abort) => {
                // A send error means the persistence thread is already gone, which is the
                // outcome an abort wants anyway.
                let _ = self.sim.persist.send(PersistMessage::Abort);
            }
            Some(Stop::Clean) | None => {
                let unapplied = self.pending.operator.len() + self.pending.commanders.len();
                if unapplied > 0 {
                    warn!(
                        inputs = unapplied,
                        "inputs submitted after the last tick are dropped"
                    );
                }
                self.send_snapshot(true)?;
                self.send(PersistMessage::Stop)?;
                info!(
                    tick = self.sim.state.tick().get(),
                    "simulation stopped; saving the final snapshot"
                );
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------- live

    fn live(&mut self) -> Result<(), ServerError> {
        self.go_live();
        loop {
            self.sim.alarm.reset();
            self.drain_inbox()?;
            if self.stop.is_some() {
                return Ok(());
            }
            let next = self.sim.state.tick().next();
            let now = self.monotonic_nanos();
            let due = self.due_nanos(next);
            if now < due {
                let deadline = Duration::from_nanos(u64::try_from(due).unwrap_or(u64::MAX));
                self.sim.clock.sleep_until(deadline, &self.sim.alarm);
                continue;
            }
            if next.get().is_multiple_of(WALL_CHECK_TICKS)
                && self.wall_target().get() > self.sim.state.tick().get() + LIVE_CATCH_UP_TICKS
            {
                warn!(
                    tick = self.sim.state.tick().get(),
                    wall_clock_tick = self.wall_target().get(),
                    "the Universe fell more than a minute behind the wall clock \
                     (the computer slept, or its clock changed)"
                );
                self.catch_up()?;
                if self.stop.is_some() {
                    return Ok(());
                }
                self.go_live();
                continue;
            }
            let behind = u64::try_from((now - due) / TICK_NANOS).unwrap_or(u64::MAX);
            self.warn_if_behind(next, behind);
            let outputs = self.advance()?;
            let elapsed = u64::try_from(self.monotonic_nanos() - now).unwrap_or(0);
            self.stats.record(elapsed, behind);
            let tick = outputs.tick;
            self.sim.shared.publish(&ServerEvent::Tick(outputs));
            self.sim.shared.set_tick(tick);
        }
    }

    /// Enters live play and anchors pacing so that each tick is due when the wall clock
    /// reaches its Universe time.
    fn go_live(&mut self) {
        let tick = self.sim.state.tick();
        let behind_ms = self
            .sim
            .clock
            .utc_ms()
            .saturating_sub(self.sim.state.clock().universe_time_ms(tick))
            .clamp(
                0,
                i64::try_from(LIVE_CATCH_UP_TICKS * TICK_MILLIS).unwrap_or(0),
            );
        self.anchor = Anchor {
            tick,
            nanos: self.monotonic_nanos() - i128::from(behind_ms) * 1_000_000,
        };
        self.stats = TickStats::default();
        self.sim.shared.publish(&ServerEvent::Live { tick });
        self.sim.shared.set_phase(Phase::Live, tick);
        info!(tick = tick.get(), "live");
    }

    fn due_nanos(&self, tick: Tick) -> i128 {
        let ticks = i128::from(tick.get()) - i128::from(self.anchor.tick.get());
        self.anchor.nanos + ticks * TICK_NANOS
    }

    fn warn_if_behind(&mut self, tick: Tick, behind: u64) {
        if behind <= BEHIND_WARNING_TICKS {
            return;
        }
        let quiet = self
            .last_behind_warning
            .is_some_and(|last| tick.since(last).get() < STATS_TICKS as u64);
        if !quiet {
            warn!(
                tick = tick.get(),
                behind_ticks = behind,
                "simulation is running behind real time"
            );
            self.last_behind_warning = Some(tick);
        }
    }

    // ------------------------------------------------------------ catch-up

    /// Fast-forwards to the wall clock (doc 11 §7, D-047). The caller has checked that the
    /// target is at least 2 ticks ahead.
    fn catch_up(&mut self) -> Result<(), ServerError> {
        let from = self.sim.state.tick();
        let mut target = self.wall_target();
        info!(
            from = from.get(),
            target = target.get(),
            missed = %ticks_text(target.since(from)),
            "catching up"
        );
        // An end marker queued for a resumed catch-up is superseded by this catch-up's own.
        self.pending
            .operator
            .retain(|input| *input != OperatorInput::CatchUpEnd);
        if !self.sim.state.catching_up() {
            self.pending.operator.insert(0, OperatorInput::CatchUpStart);
        }
        let started = self.sim.clock.monotonic();
        let mut progress = ProgressReporter::new(started);
        self.report_progress(&mut progress, from, target, true);
        loop {
            while self.sim.state.tick().next() < target {
                let outputs = self.advance()?;
                if outputs.tick.get().is_multiple_of(CATCH_UP_POLL_TICKS) {
                    self.sim.alarm.reset();
                    self.drain_inbox()?;
                    if self.stop.is_some() {
                        info!(
                            tick = outputs.tick.get(),
                            target = target.get(),
                            "catch-up interrupted; it resumes at the next start"
                        );
                        self.sim.shared.set_tick(outputs.tick);
                        return Ok(());
                    }
                    self.sim.shared.set_tick(outputs.tick);
                    self.report_progress(&mut progress, from, target, false);
                }
            }
            let next_target = self.wall_target();
            if next_target.get() <= target.get() + SETTLED_TICKS {
                break;
            }
            target = next_target;
        }
        // The target tick runs live: the mode ends as it begins.
        self.pending.operator.insert(0, OperatorInput::CatchUpEnd);
        let outputs = self.advance()?;
        let elapsed = self.sim.clock.monotonic().saturating_sub(started);
        let done = outputs.tick.since(from);
        let final_progress = CatchUpProgress {
            from,
            target: outputs.tick,
            tick: outputs.tick,
            elapsed,
        };
        info!(
            ticks = done.get(),
            universe_time = %ticks_text(done),
            elapsed = %duration_text(elapsed),
            speed = format_args!("{:.0}x real time", final_progress.speed()),
            ticks_per_second = format_args!("{:.0}", final_progress.ticks_per_second()),
            "caught up"
        );
        self.sim
            .shared
            .publish(&ServerEvent::CatchUp(final_progress));
        self.sim.shared.set_catch_up(final_progress);
        Ok(())
    }

    fn report_progress(
        &self,
        reporter: &mut ProgressReporter,
        from: Tick,
        target: Tick,
        force: bool,
    ) {
        let now = self.sim.clock.monotonic();
        let progress = CatchUpProgress {
            from,
            target,
            tick: self.sim.state.tick(),
            elapsed: now.saturating_sub(reporter.started),
        };
        if force || now.saturating_sub(reporter.last_event) >= PROGRESS_EVENT_INTERVAL {
            reporter.last_event = now;
            self.sim.shared.publish(&ServerEvent::CatchUp(progress));
            self.sim.shared.set_catch_up(progress);
        }
        if !force && now.saturating_sub(reporter.last_log) >= PROGRESS_LOG_INTERVAL {
            reporter.last_log = now;
            info!(
                tick = progress.tick.get(),
                target = target.get(),
                remaining = %ticks_text(progress.remaining()),
                speed = format_args!("{:.0}x real time", progress.speed()),
                ticks_per_second = format_args!("{:.0}", progress.ticks_per_second()),
                "catching up"
            );
        }
    }

    // ---------------------------------------------------------------- ticks

    /// Computes the next tick with the pending inputs, journals them and takes a snapshot
    /// when one is due.
    fn advance(&mut self) -> Result<TickOutputs, ServerError> {
        let inputs = std::mem::take(&mut self.pending);
        let outputs = step(&mut self.sim.state, &inputs);
        let tick = outputs.tick;
        if tick == self.sim.season_end {
            warn_season_over(tick);
        }
        if outputs.inputs_rejected > 0 {
            debug!(
                tick = tick.get(),
                rejected = outputs.inputs_rejected,
                "inputs rejected by the simulation"
            );
        }
        let entries = journal::encode(tick, &inputs, &self.sim.build)?;
        if !entries.is_empty() {
            self.send(PersistMessage::Journal(entries))?;
            self.batch_start.get_or_insert(tick);
        }
        if tick.get().is_multiple_of(self.snapshot_interval) {
            self.send_snapshot(false)?;
        } else if self
            .batch_start
            .is_some_and(|start| tick.since(start).get() + 1 >= COMMIT_TICKS)
        {
            self.send(PersistMessage::Commit)?;
            self.batch_start = None;
        }
        Ok(outputs)
    }

    fn send_snapshot(&mut self, announce: bool) -> Result<(), ServerError> {
        let state = &self.sim.state;
        let record = SnapshotRecord {
            tick: state.tick(),
            build: self.sim.build.clone(),
            format: snapshot::FORMAT,
            state_hash: state_hash(state)?,
            bytes: encode(state)?,
        };
        // The persistence thread commits the pending journal before the snapshot.
        self.batch_start = None;
        self.send(PersistMessage::Snapshot { record, announce })
    }

    fn send(&self, message: PersistMessage) -> Result<(), ServerError> {
        self.sim
            .persist
            .send(message)
            .map_err(|_| ServerError::PersistenceStopped)
    }

    // ---------------------------------------------------------------- inbox

    fn drain_inbox(&mut self) -> Result<(), ServerError> {
        loop {
            match self.sim.inbox.try_recv() {
                Ok(Control::Input(Input::Operator(input))) => self.pending.operator.push(input),
                Ok(Control::Input(Input::Commander(input))) => {
                    self.pending.commanders.push(input);
                }
                Ok(Control::Flush(reply)) => {
                    self.batch_start = None;
                    self.send(PersistMessage::Flush(reply))?;
                }
                Ok(Control::StateHash(reply)) => {
                    let result = state_hash(&self.sim.state)
                        .map(|hash| (self.sim.state.tick(), hash))
                        .map_err(|e| e.to_string());
                    let _ = reply.send(result);
                }
                Ok(Control::Shutdown) => self.request_stop(Stop::Clean),
                // A dropped handle without a goodbye is treated like an abort.
                Ok(Control::Abort) | Err(TryRecvError::Disconnected) => {
                    self.request_stop(Stop::Abort);
                    return Ok(());
                }
                Err(TryRecvError::Empty) => return Ok(()),
            }
        }
    }

    fn request_stop(&mut self, stop: Stop) {
        self.stop = Some(self.stop.map_or(stop, |current| current.max(stop)));
    }

    // ----------------------------------------------------------------- time

    fn wall_target(&self) -> Tick {
        wall_tick(self.sim.state.clock(), self.sim.clock.utc_ms())
    }

    fn monotonic_nanos(&self) -> i128 {
        i128::try_from(self.sim.clock.monotonic().as_nanos()).unwrap_or(i128::MAX)
    }
}

/// What happens at the season end (D-020) is not designed yet, so the Universe runs on.
fn warn_season_over(tick: Tick) {
    warn!(
        tick = tick.get(),
        "the season has ended; season end is not designed yet, so the Universe keeps running"
    );
}

/// The last tick whose Universe time is at or before `unix_ms`: where the Universe should
/// be at that wall-clock time (doc 11 §2). Tick 0 before the season start.
pub(crate) fn wall_tick(clock: SeasonClock, unix_ms: i64) -> Tick {
    let since_start = unix_ms.saturating_sub(clock.season_start_unix_ms());
    u64::try_from(since_start).map_or(Tick::ZERO, |ms| Tick::new(ms / TICK_MILLIS))
}

struct ProgressReporter {
    started: Duration,
    last_event: Duration,
    last_log: Duration,
}

impl ProgressReporter {
    fn new(started: Duration) -> Self {
        Self {
            started,
            last_event: started,
            last_log: started,
        }
    }
}

/// Live tick durations, logged as mean and 99th percentile once a minute (doc 20 §10).
#[derive(Default)]
struct TickStats {
    nanos: Vec<u64>,
    max_behind: u64,
}

impl TickStats {
    fn record(&mut self, nanos: u64, behind: u64) {
        self.nanos.push(nanos);
        self.max_behind = self.max_behind.max(behind);
        if self.nanos.len() >= STATS_TICKS {
            self.log_and_reset();
        }
    }

    fn log_and_reset(&mut self) {
        let mut sorted = std::mem::take(&mut self.nanos);
        sorted.sort_unstable();
        let count = sorted.len();
        if count == 0 {
            return;
        }
        let ms = |nanos: u64| nanos as f64 / 1_000_000.0;
        let total: u128 = sorted.iter().map(|&n| u128::from(n)).sum();
        let mean = total as f64 / count as f64 / 1_000_000.0;
        // Nearest-rank 99th percentile.
        let p99 = sorted[(count * 99).div_ceil(100).saturating_sub(1)];
        let max = sorted[count - 1];
        info!(
            ticks = count,
            mean_ms = format_args!("{mean:.3}"),
            p99_ms = format_args!("{:.3}", ms(p99)),
            max_ms = format_args!("{:.3}", ms(max)),
            max_behind_ticks = self.max_behind,
            "tick time"
        );
        self.max_behind = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_tick_is_the_last_tick_started() {
        let clock = SeasonClock::new(10_000);
        assert_eq!(wall_tick(clock, 0), Tick::ZERO);
        assert_eq!(wall_tick(clock, 10_000), Tick::ZERO);
        assert_eq!(wall_tick(clock, 10_099), Tick::ZERO);
        assert_eq!(wall_tick(clock, 10_100), Tick::new(1));
        assert_eq!(
            wall_tick(clock, 10_000 + 6 * 3_600_000),
            Tick::new(Ticks::from_hours(6).get())
        );
    }

    #[test]
    fn tick_stats_reset_after_a_minute() {
        let mut stats = TickStats::default();
        for i in 0..STATS_TICKS as u64 - 1 {
            stats.record(i * 1_000, i % 3);
        }
        assert_eq!(stats.nanos.len(), STATS_TICKS - 1);
        stats.record(5, 20);
        assert!(stats.nanos.is_empty());
        assert_eq!(stats.max_behind, 0);
    }
}
