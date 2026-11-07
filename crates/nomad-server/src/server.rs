//! The in-process server API: start a Universe, submit inputs, follow its outputs, shut it
//! down (N2 plan §3, §4.4–4.5; doc 20 §4, §7). With [`ServerConfig::net`] set, the network
//! endpoint serves the same API to clients (N3).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nomad_sim::{
    CommanderInput, OperatorInput, TICKS_PER_SECOND, Tick, TickOutputs, Ticks, UniverseState,
};
use tracing::{info, warn};

use crate::BUILD_ID;
use crate::clock::{Alarm, Clock};
use crate::error::ServerError;
use crate::ids::parse_universe_id;
use crate::net::hub::Submit;
use crate::net::identity::Identity;
use crate::net::{NetConfig, Network, UniverseInfo};
use crate::paths::{DATABASE_FILE, UniverseLock};
use crate::persist::{self, CHANNEL_BOUND};
use crate::recovery::{self, ReplayCheck};
use crate::retention::RetentionPolicy;
use crate::sim::{Control, Sim, Stop};
use crate::store::{Meta, Store};

/// Snapshot interval: 5 min of Universe time, 3,000 ticks (doc 11 §8, D-048).
pub const SNAPSHOT_INTERVAL: Ticks = Ticks::from_minutes(5);

/// State-hash checkpoint interval: 60 s of Universe time, 600 ticks (doc 11 §5, D-109).
pub const CHECKPOINT_INTERVAL: Ticks = Ticks::from_secs(60);

/// Events a subscriber can fall behind by before newer events are dropped for it.
const SUBSCRIBER_BOUND: usize = 4_096;

/// How to run a Universe.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The Universe's directory (`<data>/universes/<id>`).
    pub universe_dir: PathBuf,
    /// The build identity written to the journal and snapshots and checked on replay
    /// (D-048). Always [`BUILD_ID`] outside tests.
    pub build: String,
    /// Ticks between periodic snapshots. [`SNAPSHOT_INTERVAL`] outside tests.
    pub snapshot_interval: Ticks,
    /// Ticks between state-hash checkpoints in the journal (D-109). [`CHECKPOINT_INTERVAL`]
    /// outside tests.
    pub checkpoint_interval: Ticks,
    /// Which snapshots and how much journal to keep (doc 11 §8, D-048, D-110).
    /// [`RetentionPolicy::default`] outside tests. Tests may run it in scaled time
    /// ([`RetentionPolicy::scaled`]), with the two intervals above scaled alike so that the
    /// snapshot interval still divides the hour.
    pub retention: RetentionPolicy,
    /// The season's final tick, after which the Universe stops advancing (D-116). `None`
    /// takes it from the Universe's season length (D-020, D-106); tests set a short season.
    pub season_end: Option<Tick>,
    /// The network endpoint (N3; D-150). `None` runs without one, as most tests do.
    pub net: Option<NetConfig>,
}

impl ServerConfig {
    /// The production configuration for the Universe in `universe_dir`.
    pub fn new(universe_dir: impl Into<PathBuf>) -> Self {
        Self {
            universe_dir: universe_dir.into(),
            build: BUILD_ID.to_owned(),
            snapshot_interval: SNAPSHOT_INTERVAL,
            checkpoint_interval: CHECKPOINT_INTERVAL,
            retention: RetentionPolicy::default(),
            season_end: None,
            net: None,
        }
    }
}

/// The season's final tick for a season of `weeks` (D-020): the Universe stops advancing
/// after it (D-116).
pub fn season_end_tick(weeks: u32) -> Tick {
    Tick::ZERO.after(Ticks::from_days(7 * u64::from(weeks)))
}

/// An external input for the simulation (doc 11 §6). It applies on the tick after the one
/// during which it was submitted. Not `Eq`: commands carry `f64` positions (S3).
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// From a commander's client.
    Commander(CommanderInput),
    /// An operator command.
    Operator(OperatorInput),
}

/// Why an input was not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SubmitError {
    /// The server is starting or catching up; inputs are taken only while live (doc 20 §7).
    #[error("the server is not live")]
    NotLive,
    /// The catch-up markers are written by the server only.
    #[error("this input is reserved for the server")]
    Reserved,
    /// The season has ended and the Universe no longer advances (D-116).
    #[error("the season has ended")]
    SeasonOver,
    /// The server has stopped.
    #[error("the server has stopped")]
    Stopped,
}

/// Where the server is in its lifecycle (doc 11 §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Loaded and replayed; the simulation thread has not decided yet.
    Starting,
    /// Fast-forwarding to the wall clock (D-047).
    CatchingUp,
    /// One tick every 100 ms.
    Live,
    /// The season's final tick has been computed: the Universe no longer advances, and the
    /// server serves its final state until it is shut down (D-116).
    SeasonOver,
    /// The simulation thread has ended.
    Stopped,
}

/// How far a catch-up has come, for logs and later the client's catch-up screen (N3/U6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatchUpProgress {
    /// The tick catch-up started from.
    pub from: Tick,
    /// The tick it is heading for.
    pub target: Tick,
    /// The tick reached.
    pub tick: Tick,
    /// Real time spent so far.
    pub elapsed: Duration,
}

impl CatchUpProgress {
    /// Ticks computed so far.
    pub fn done(&self) -> Ticks {
        self.tick.since(self.from)
    }

    /// Ticks still to compute.
    pub fn remaining(&self) -> Ticks {
        self.target.since(self.tick)
    }

    /// Share of the work done, 0 to 1.
    pub fn fraction(&self) -> f64 {
        let total = self.target.since(self.from).get();
        if total == 0 {
            1.0
        } else {
            self.done().get() as f64 / total as f64
        }
    }

    /// Ticks per second of real time so far; 0 before any time has passed.
    pub fn ticks_per_second(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs > 0.0 {
            self.done().get() as f64 / secs
        } else {
            0.0
        }
    }

    /// Universe time per real time so far (doc 11 §4.3 asks ≥ 500× at stage 1).
    pub fn speed(&self) -> f64 {
        self.ticks_per_second() / TICKS_PER_SECOND as f64
    }
}

/// A snapshot of the server's progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerStatus {
    /// The lifecycle phase.
    pub phase: Phase,
    /// The last computed tick. Updated every tick while live, and in steps of about a
    /// thousand ticks while catching up.
    pub tick: Tick,
    /// The latest catch-up progress, kept after catch-up ends.
    pub catch_up: Option<CatchUpProgress>,
}

/// What subscribers receive (N2 plan §4.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerEvent {
    /// A live tick was computed.
    Tick(TickOutputs),
    /// Catch-up progress, about once a second and at the end. Catch-up ticks are not
    /// published one by one (doc 20 §7: clients see only progress then).
    CatchUp(CatchUpProgress),
    /// The server went live at this tick.
    Live {
        /// The last tick computed before live play.
        tick: Tick,
    },
    /// The season ended: this is the final tick, and the Universe no longer advances
    /// (D-116).
    SeasonOver {
        /// The season's final tick.
        tick: Tick,
    },
    /// The simulation thread ended.
    Stopped,
}

/// How a server stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownReport {
    /// The last computed tick, which the final snapshot holds.
    pub tick: Tick,
}

/// What [`ServerHandle::start`] found on disk.
#[derive(Clone, Debug)]
pub struct StartReport {
    /// The Universe's `meta` row.
    pub meta: Meta,
    /// The snapshot recovery started from.
    pub snapshot_tick: Tick,
    /// Journal input entries replayed after it, not counting checkpoints.
    pub replayed_entries: usize,
    /// What the state-hash checkpoints met during replay showed (D-109).
    pub replay_check: ReplayCheck,
    /// The tick after replay, where catch-up or live play starts.
    pub tick: Tick,
    /// The season's final tick (D-116).
    pub season_end: Tick,
    /// Where the network endpoint listens, if it runs (N3; D-150).
    pub endpoint: Option<SocketAddr>,
}

/// Requests a clean shutdown from another thread, for example a Ctrl+C handler.
#[derive(Clone, Debug)]
pub struct ShutdownTrigger {
    inbox: Sender<Control>,
    alarm: Arc<Alarm>,
}

impl ShutdownTrigger {
    /// Asks the server to stop cleanly: final snapshot, flush, exit. Returns at once.
    pub fn shutdown(&self) {
        // A send error means the server has stopped already.
        let _ = self.inbox.send(Control::Shutdown);
        self.alarm.ring();
    }
}

/// A running Universe: the simulation and persistence threads and the Universe lock.
///
/// [`ServerHandle::shutdown`] stops it cleanly. Dropping the handle without that stops it
/// like a crash: no final snapshot, and the journal batch not yet committed (at most 1 s of
/// inputs) is lost. Recovery covers that case (D-048).
#[derive(Debug)]
pub struct ServerHandle {
    report: StartReport,
    inbox: Sender<Control>,
    alarm: Arc<Alarm>,
    shared: Arc<Shared>,
    sim: Option<JoinHandle<Result<Stop, ServerError>>>,
    persist: Option<JoinHandle<Result<(), ServerError>>>,
    /// The network endpoint, if it runs (N3).
    network: Option<Network>,
    // Dropped after the threads have been joined.
    _lock: UniverseLock,
}

impl ServerHandle {
    /// Starts the Universe in `config.universe_dir`: takes its lock, loads the latest
    /// snapshot and replays the journal, checking its state-hash checkpoints (D-109;
    /// [`StartReport::replay_check`]), then starts the threads. The simulation thread
    /// catches up to `clock`'s wall-clock time and then runs live, until the season's final
    /// tick, after which it serves the final state until shutdown (D-116).
    ///
    /// Fails without starting anything if the Universe is locked, damaged, in a format this
    /// build cannot read, or has a journal written by another build.
    pub fn start(config: ServerConfig, clock: Arc<dyn Clock>) -> Result<Self, ServerError> {
        let dir = config.universe_dir;
        let db_path = dir.join(DATABASE_FILE);
        if !db_path.is_file() {
            return Err(ServerError::UniverseNotFound { path: dir });
        }
        let lock = UniverseLock::acquire(&dir)?;
        let store = Store::open(&db_path)?;
        let meta = store.meta()?;
        if dir.file_name().and_then(|name| name.to_str()) != Some(meta.universe_id.as_str()) {
            warn!(
                universe = %meta.universe_id,
                dir = %dir.display(),
                "the directory name differs from the Universe ID"
            );
        }
        info!(
            universe = %meta.universe_id,
            build = %config.build,
            dir = %dir.display(),
            "opening Universe"
        );
        let recovered = recovery::recover(&store, &meta, &config.build)?;
        let season_end = config
            .season_end
            .unwrap_or_else(|| season_end_tick(meta.season_weeks));
        let mut report = StartReport {
            meta,
            snapshot_tick: recovered.snapshot_tick,
            replayed_entries: recovered.replayed_entries,
            replay_check: recovered.check,
            tick: recovered.state.tick(),
            season_end,
            endpoint: None,
        };

        let shared = Arc::new(Shared::new(recovered.state.tick()));
        let network = match config.net {
            Some(net) => Some(PreparedNetwork::prepare(
                net,
                &dir,
                &report.meta,
                &recovered.state,
                &shared,
                &config.build,
            )?),
            None => None,
        };
        let alarm = Alarm::new();
        let (persist_tx, persist_rx) = mpsc::sync_channel(CHANNEL_BOUND);
        let persist_clock = Arc::clone(&clock);
        let retention = config.retention;
        let persist = thread::Builder::new()
            .name("nomad-persist".to_owned())
            .spawn(move || persist::run(store, &persist_rx, &*persist_clock, retention))
            .map_err(|e| ServerError::io("cannot start the persistence thread", e))?;
        let (inbox_tx, inbox_rx) = mpsc::channel();
        let sim = Sim {
            state: recovered.state,
            clock,
            alarm: Arc::clone(&alarm),
            inbox: inbox_rx,
            persist: persist_tx.clone(),
            shared: Arc::clone(&shared),
            build: config.build,
            snapshot_interval: config.snapshot_interval,
            checkpoint_interval: config.checkpoint_interval,
            season_end,
        };
        let sim = match thread::Builder::new()
            .name("nomad-sim".to_owned())
            .spawn(move || sim.run())
        {
            Ok(sim) => sim,
            Err(e) => {
                let _ = persist_tx.send(persist::PersistMessage::Abort);
                let _ = persist.join();
                return Err(ServerError::io("cannot start the simulation thread", e));
            }
        };
        let mut handle = Self {
            report: report.clone(),
            inbox: inbox_tx,
            alarm,
            shared,
            sim: Some(sim),
            persist: Some(persist),
            network: None,
            _lock: lock,
        };
        if let Some(prepared) = network {
            // On failure the handle is dropped, which stops the threads like a crash.
            let network = prepared.start(handle.access())?;
            report.endpoint = Some(network.local_addr);
            handle.report = report;
            handle.network = Some(network);
        }
        Ok(handle)
    }

    /// The submit part of the handle, for the network thread.
    pub(crate) fn access(&self) -> ServerAccess {
        ServerAccess {
            inbox: self.inbox.clone(),
            shared: Arc::clone(&self.shared),
        }
    }

    /// What startup found on disk.
    pub fn start_report(&self) -> &StartReport {
        &self.report
    }

    /// Submits an input. It applies on the next tick the server computes (doc 11 §6).
    pub fn submit(&self, input: Input) -> Result<(), SubmitError> {
        submit(&self.inbox, &self.shared, input)
    }

    /// Subscribes to [`ServerEvent`]s from now on. A subscriber that falls more than a few
    /// thousand events behind misses the newer ones; the tick numbers show the gap.
    pub fn subscribe(&self) -> Receiver<ServerEvent> {
        self.shared.subscribe()
    }

    /// The current phase and tick.
    pub fn status(&self) -> ServerStatus {
        self.shared.status()
    }

    /// Waits up to `timeout` for a status that satisfies `done`, and returns it; `None` on
    /// timeout. Also returns once the server has stopped.
    pub fn wait_for(
        &self,
        timeout: Duration,
        done: impl Fn(&ServerStatus) -> bool,
    ) -> Option<ServerStatus> {
        self.shared.wait_for(timeout, done)
    }

    /// Waits up to `timeout` until the server is live, and returns its status.
    pub fn wait_until_live(&self, timeout: Duration) -> Option<ServerStatus> {
        self.wait_for(timeout, |status| status.phase == Phase::Live)
            .filter(|status| status.phase == Phase::Live)
    }

    /// Waits up to `timeout` until `tick` has been computed. Returns whether it was.
    pub fn wait_for_tick(&self, tick: Tick, timeout: Duration) -> bool {
        self.wait_for(timeout, |status| status.tick >= tick)
            .is_some_and(|status| status.tick >= tick)
    }

    /// Commits the journal of every tick computed so far, and waits until it is on disk.
    pub fn flush(&self) -> Result<(), ServerError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.control(Control::Flush(reply))?;
        match answer.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err(ServerError::Persistence(message)),
            Err(_) => Err(ServerError::Stopped),
        }
    }

    /// The current tick and its state hash, for determinism checks.
    pub fn state_hash(&self) -> Result<(Tick, u64), ServerError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.control(Control::StateHash(reply))?;
        match answer.recv() {
            Ok(Ok(hash)) => Ok(hash),
            Ok(Err(message)) => Err(ServerError::Encode(message)),
            Err(_) => Err(ServerError::Stopped),
        }
    }

    /// A trigger that requests a clean shutdown from another thread.
    pub fn shutdown_trigger(&self) -> ShutdownTrigger {
        ShutdownTrigger {
            inbox: self.inbox.clone(),
            alarm: Arc::clone(&self.alarm),
        }
    }

    /// Stops cleanly: final snapshot, flush, exit (N2 plan §4.5 step 4). Waits for it.
    pub fn shutdown(self) -> Result<ShutdownReport, ServerError> {
        self.shutdown_trigger().shutdown();
        self.join()
    }

    /// Waits until the server stops, after [`ShutdownTrigger::shutdown`] or a fatal error.
    pub fn join(mut self) -> Result<ShutdownReport, ServerError> {
        let sim = self.sim.take().map(JoinHandle::join);
        let persist = self.persist.take().map(JoinHandle::join);
        // The network thread ends once the simulation has published that it stopped. The
        // connection file goes only after a clean stop (N3 plan §4.2).
        let clean = matches!(sim, Some(Ok(Ok(Stop::Clean)))) && matches!(persist, Some(Ok(Ok(()))));
        if let Some(network) = self.network.take() {
            network.join(clean);
        }
        // A persistence failure is the root cause of the simulation stopping, so report it
        // first.
        match persist {
            Some(Ok(result)) => result?,
            Some(Err(_)) => return Err(ServerError::ThreadPanicked("persistence")),
            None => return Err(ServerError::Stopped),
        }
        match sim {
            Some(Ok(result)) => result?,
            Some(Err(_)) => return Err(ServerError::ThreadPanicked("simulation")),
            None => return Err(ServerError::Stopped),
        };
        Ok(ShutdownReport {
            tick: self.shared.status().tick,
        })
    }

    fn control(&self, control: Control) -> Result<(), ServerError> {
        self.inbox.send(control).map_err(|_| ServerError::Stopped)?;
        self.alarm.ring();
        Ok(())
    }
}

impl Drop for ServerHandle {
    /// Stops like a crash if the server was not shut down (see [`ServerHandle`]).
    fn drop(&mut self) {
        if self.sim.is_none() && self.persist.is_none() {
            return;
        }
        let _ = self.inbox.send(Control::Abort);
        self.alarm.ring();
        if let Some(sim) = self.sim.take() {
            let _ = sim.join();
        }
        if let Some(persist) = self.persist.take() {
            let _ = persist.join();
        }
        // Stops like a crash: the connection file stays, and the free lock marks it stale.
        if let Some(network) = self.network.take() {
            network.join(false);
        }
    }
}

/// What the network endpoint needs before the server's threads start (N3): the identity,
/// loaded first so a damaged one stops the start (D-151); the event subscription, made
/// before the simulation runs so no event is missed; and the bound socket, so a busy port
/// stops the start cleanly (D-150).
struct PreparedNetwork {
    net: NetConfig,
    identity: Identity,
    info: UniverseInfo,
    events: Receiver<ServerEvent>,
    socket: std::net::UdpSocket,
}

impl PreparedNetwork {
    fn prepare(
        net: NetConfig,
        dir: &std::path::Path,
        meta: &Meta,
        state: &UniverseState,
        shared: &Shared,
        build: &str,
    ) -> Result<Self, ServerError> {
        let identity = Identity::load_or_create(dir)?;
        let universe = parse_universe_id(&meta.universe_id)
            .map_err(|e| ServerError::InvalidParameter(format!("the Universe ID in meta: {e}")))?;
        let socket = crate::net::bind(&net)?;
        Ok(Self {
            net,
            identity,
            info: UniverseInfo {
                dir: dir.to_owned(),
                universe,
                clock: state.clock(),
                tick: state.tick(),
                build: build.to_owned(),
            },
            events: shared.subscribe(),
            socket,
        })
    }

    fn start(self, server: ServerAccess) -> Result<Network, ServerError> {
        Network::start(
            self.socket,
            &self.net,
            self.identity,
            &self.info,
            server,
            self.events,
        )
    }
}

/// The checks and hand-over of [`ServerHandle::submit`], shared with [`ServerAccess`].
fn submit(inbox: &Sender<Control>, shared: &Shared, input: Input) -> Result<(), SubmitError> {
    if matches!(
        input,
        Input::Operator(OperatorInput::CatchUpStart | OperatorInput::CatchUpEnd)
    ) {
        return Err(SubmitError::Reserved);
    }
    match shared.status().phase {
        Phase::Live => {}
        Phase::SeasonOver => return Err(SubmitError::SeasonOver),
        Phase::Stopped => return Err(SubmitError::Stopped),
        Phase::Starting | Phase::CatchingUp => return Err(SubmitError::NotLive),
    }
    inbox
        .send(Control::Input(input))
        .map_err(|_| SubmitError::Stopped)
}

/// What the network thread may do with a running server: submit inputs (N3).
#[derive(Clone, Debug)]
pub(crate) struct ServerAccess {
    inbox: Sender<Control>,
    shared: Arc<Shared>,
}

impl Submit for ServerAccess {
    fn submit(&self, input: Input) -> Result<(), SubmitError> {
        submit(&self.inbox, &self.shared, input)
    }
}

/// State shared between the handle and the simulation thread.
#[derive(Debug)]
pub(crate) struct Shared {
    status: Mutex<ServerStatus>,
    changed: Condvar,
    subscribers: Mutex<Vec<SyncSender<ServerEvent>>>,
}

impl Shared {
    fn new(tick: Tick) -> Self {
        Self {
            status: Mutex::new(ServerStatus {
                phase: Phase::Starting,
                tick,
                catch_up: None,
            }),
            changed: Condvar::new(),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    fn lock_status(&self) -> MutexGuard<'_, ServerStatus> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn status(&self) -> ServerStatus {
        *self.lock_status()
    }

    fn update(&self, change: impl FnOnce(&mut ServerStatus)) {
        change(&mut self.lock_status());
        self.changed.notify_all();
    }

    pub fn set_tick(&self, tick: Tick) {
        self.update(|status| status.tick = tick);
    }

    pub fn set_phase(&self, phase: Phase, tick: Tick) {
        self.update(|status| {
            status.phase = phase;
            status.tick = tick;
        });
    }

    pub fn set_catch_up(&self, progress: CatchUpProgress) {
        self.update(|status| {
            status.phase = Phase::CatchingUp;
            status.tick = progress.tick;
            status.catch_up = Some(progress);
        });
    }

    fn wait_for(
        &self,
        timeout: Duration,
        done: impl Fn(&ServerStatus) -> bool,
    ) -> Option<ServerStatus> {
        let guard = self.lock_status();
        let (guard, result) = self
            .changed
            .wait_timeout_while(guard, timeout, |status| {
                !done(status) && status.phase != Phase::Stopped
            })
            .unwrap_or_else(PoisonError::into_inner);
        (!result.timed_out()).then_some(*guard)
    }

    fn subscribe(&self) -> Receiver<ServerEvent> {
        let (sender, receiver) = mpsc::sync_channel(SUBSCRIBER_BOUND);
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(sender);
        receiver
    }

    /// Sends `event` to every subscriber without blocking; drops subscribers that are gone.
    pub fn publish(&self, event: &ServerEvent) {
        let mut subscribers = self
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if subscribers.is_empty() {
            return;
        }
        subscribers.retain(|subscriber| match subscriber.try_send(event.clone()) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        });
    }
}
