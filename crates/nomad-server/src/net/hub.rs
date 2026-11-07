//! The hub: one task between the sessions and the simulation (N3 plan §4.1, §4.4, §4.5).
//!
//! It owns the session registry, turns commands into simulation inputs, routes each tick's
//! command outcomes back to the session that sent them, and keeps the simulation's view of
//! who is connected in step with the open sessions. It never blocks: submitting is a channel
//! send, and the simulation's events arrive through a bridge thread.
//!
//! - **Sequence numbers.** The simulation orders a commander's inputs by `client_seq`. The
//!   hub hands out its own per-commander numbers, from 1 upwards, and remembers which
//!   session and client number each belongs to. So results stay unambiguous across
//!   reconnects and replaced sessions, and a session's `Connected` sorts before its first
//!   command. The numbers are journaled with the inputs, so replay agrees.
//! - **The server decides.** The hub learns of the server's phase from events, which lag
//!   behind it, so it never refuses a command or a connection change on its own view:
//!   everything goes through the server's `submit`, which refuses while the server is not
//!   live (doc 20 §7). A refused submission gives its sequence number back.
//! - **Connected and disconnected** (doc 11 §6–7, D-047). A commander counts as connected
//!   while it has a session. When catch-up starts, the simulation thread disconnects
//!   everyone; the hub forgets who it had connected and reconnects the open sessions when
//!   the server is live again.
//! - **Commands across catch-up.** While catching up, commands are refused with
//!   `CatchingUp`. A command submitted just before catch-up either applies on the first
//!   catch-up tick, whose outcomes the simulation publishes, or is dropped by the
//!   simulation. So the commands still pending when the hub learns of a catch-up are marked,
//!   and those of them still unanswered when it learns the server is live again get
//!   `CatchingUp`. Commands submitted after the server went live are never marked, even if
//!   the hub had not heard yet.
//! - **Lag** (§4.1). Live ticks arrive one by one. A gap means the bridge fell behind and
//!   events were lost; sessions with unanswered commands then get `Bye { Resync }`.

use std::collections::{BTreeMap, BTreeSet};

use nomad_protocol::{ByeReason, CatchUpProgress, CommandOutcome, RejectReason, ServerMessage};
use nomad_sim::{CommanderId, CommanderInput, CommanderInputKind, Tick, TickOutputs};
use nomad_types::{Command, SeasonClock, UniverseId};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info, warn};

use crate::reject::wire_reason;
use crate::server::{Input, ServerEvent, SubmitError};

/// A session's number, unique for the server's lifetime.
pub type SessionId = u64;

/// Something that takes simulation inputs: the running server, or a recorder in tests.
pub trait Submit {
    /// Submits an input for the next tick.
    fn submit(&self, input: Input) -> Result<(), SubmitError>;
}

/// From the hub to one session.
#[derive(Clone, Debug, PartialEq)]
pub enum ToSession {
    /// Send this message (boxed: messages are large, goodbyes small).
    Message(Box<ServerMessage>),
    /// Send `Bye` with this reason and end the session.
    Bye(ByeReason),
}

/// From a session to the hub.
#[derive(Debug)]
pub enum ToHub {
    /// The session authenticated for `commander`; the hub answers with `Welcome` on `outbox`.
    Join {
        /// The session.
        session: SessionId,
        /// Its commander.
        commander: CommanderId,
        /// Where the hub sends the session's messages.
        outbox: UnboundedSender<ToSession>,
    },
    /// A command that passed the session's envelope checks.
    Command {
        /// The session.
        session: SessionId,
        /// The client's sequence number.
        client_seq: u64,
        /// The command.
        command: Command,
    },
    /// The session ended.
    Leave {
        /// The session.
        session: SessionId,
    },
}

/// The server's lifecycle as the hub has seen it in events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HubPhase {
    Starting,
    CatchingUp,
    Live,
    SeasonOver,
    Stopped,
}

struct SessionEntry {
    commander: CommanderId,
    outbox: UnboundedSender<ToSession>,
}

/// A command waiting for its outcome.
struct Pending {
    session: SessionId,
    client_seq: u64,
    /// Submitted before a catch-up began, so the simulation may have dropped it.
    before_catch_up: bool,
}

/// The hub's state.
pub struct Hub<S> {
    server: S,
    universe: UniverseId,
    clock: SeasonClock,
    phase: HubPhase,
    /// The last tick the hub knows of.
    tick: Tick,
    /// The last live tick, for spotting lost events; `None` outside live play.
    last_live_tick: Option<Tick>,
    catch_up: Option<CatchUpProgress>,
    sessions: BTreeMap<SessionId, SessionEntry>,
    /// The session of each commander (D-124: at most one).
    active: BTreeMap<CommanderId, SessionId>,
    /// Who the simulation counts as connected through the hub's inputs.
    connected: BTreeSet<CommanderId>,
    next_seq: BTreeMap<CommanderId, u64>,
    pending: BTreeMap<(CommanderId, u64), Pending>,
}

impl<S: Submit> Hub<S> {
    /// A hub for the Universe `universe` with season clock `clock`, at `tick`.
    pub fn new(server: S, universe: UniverseId, clock: SeasonClock, tick: Tick) -> Self {
        Self {
            server,
            universe,
            clock,
            phase: HubPhase::Starting,
            tick,
            last_live_tick: None,
            catch_up: None,
            sessions: BTreeMap::new(),
            active: BTreeMap::new(),
            connected: BTreeSet::new(),
            next_seq: BTreeMap::new(),
            pending: BTreeMap::new(),
        }
    }

    /// Whether the server has stopped; the hub has said goodbye to every session.
    pub fn stopped(&self) -> bool {
        self.phase == HubPhase::Stopped
    }

    /// Handles one event from the simulation.
    pub fn on_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::Tick(outputs) => self.on_tick(&outputs),
            ServerEvent::CatchUp(progress) => {
                if self.phase != HubPhase::CatchingUp {
                    // The simulation disconnects everyone when catch-up starts (D-047), and
                    // may drop the commands that were still in flight.
                    self.phase = HubPhase::CatchingUp;
                    self.connected.clear();
                    self.last_live_tick = None;
                    for pending in self.pending.values_mut() {
                        pending.before_catch_up = true;
                    }
                }
                self.tick = progress.tick;
                let message = CatchUpProgress {
                    reached: progress.tick,
                    target: progress.target,
                };
                self.catch_up = Some(message);
                self.broadcast(&ServerMessage::CatchUp { progress: message });
            }
            ServerEvent::Live { tick } => {
                self.phase = HubPhase::Live;
                self.tick = tick;
                self.last_live_tick = Some(tick);
                self.catch_up = None;
                // Submitted before the catch-up and still unanswered: dropped by the
                // simulation.
                let (dropped, kept) = std::mem::take(&mut self.pending)
                    .into_iter()
                    .partition(|(_, p)| p.before_catch_up);
                self.pending = kept;
                self.reject(dropped, RejectReason::CatchingUp);
                self.reconcile();
            }
            ServerEvent::SeasonOver { tick, .. } => {
                self.phase = HubPhase::SeasonOver;
                self.tick = tick;
                self.last_live_tick = None;
                // Submitted after the final tick: the simulation dropped them (D-116).
                let dropped = std::mem::take(&mut self.pending);
                self.reject(dropped, RejectReason::SeasonOver);
            }
            ServerEvent::Stopped => {
                self.phase = HubPhase::Stopped;
                for entry in self.sessions.values() {
                    let _ = entry.outbox.send(ToSession::Bye(ByeReason::ShuttingDown));
                }
                self.sessions.clear();
                self.active.clear();
                self.pending.clear();
            }
        }
    }

    fn on_tick(&mut self, outputs: &TickOutputs) {
        if self.phase == HubPhase::Live {
            if let Some(last) = self.last_live_tick
                && outputs.tick != last.next()
            {
                warn!(
                    expected = last.next().get(),
                    got = outputs.tick.get(),
                    "events from the simulation were lost; resyncing affected sessions"
                );
                self.resync_waiting_sessions();
            }
            self.last_live_tick = Some(outputs.tick);
        }
        self.tick = self.tick.max(outputs.tick);
        for result in &outputs.commands {
            let Some(pending) = self.pending.remove(&(result.commander, result.client_seq)) else {
                continue;
            };
            let outcome = match result.outcome {
                Ok(()) => CommandOutcome::Applied { tick: outputs.tick },
                Err(rejection) => CommandOutcome::Rejected {
                    reason: RejectReason::Invalid {
                        reason: wire_reason(rejection),
                    },
                },
            };
            self.send(
                pending.session,
                ServerMessage::CommandResult {
                    client_seq: pending.client_seq,
                    result: outcome,
                },
            );
        }
    }

    /// Handles one request from a session.
    pub fn on_request(&mut self, request: ToHub) {
        match request {
            ToHub::Join {
                session,
                commander,
                outbox,
            } => self.join(session, commander, outbox),
            ToHub::Command {
                session,
                client_seq,
                command,
            } => self.command(session, client_seq, command),
            ToHub::Leave { session } => self.leave(session),
        }
    }

    fn join(
        &mut self,
        session: SessionId,
        commander: CommanderId,
        outbox: UnboundedSender<ToSession>,
    ) {
        if self.phase == HubPhase::Stopped {
            let _ = outbox.send(ToSession::Bye(ByeReason::ShuttingDown));
            return;
        }
        // D-124: the newest session replaces the old one. The commander stays connected, so
        // no Disconnected/Connected pair reaches the simulation.
        if let Some(old) = self.active.insert(commander, session)
            && let Some(entry) = self.sessions.remove(&old)
        {
            info!(
                commander = commander.0,
                old,
                new = session,
                "session replaced"
            );
            let _ = entry.outbox.send(ToSession::Bye(ByeReason::Replaced));
            self.pending.retain(|_, p| p.session != old);
        }
        let _ = outbox.send(ToSession::Message(Box::new(ServerMessage::Welcome {
            commander,
            universe: self.universe,
            tick: self.tick,
            clock: self.clock,
        })));
        if self.phase == HubPhase::CatchingUp
            && let Some(progress) = self.catch_up
        {
            let _ = outbox.send(ToSession::Message(Box::new(ServerMessage::CatchUp {
                progress,
            })));
        }
        self.sessions
            .insert(session, SessionEntry { commander, outbox });
        self.reconcile();
    }

    fn command(&mut self, session: SessionId, client_seq: u64, command: Command) {
        let Some(commander) = self.sessions.get(&session).map(|e| e.commander) else {
            return;
        };
        let reject = |hub: &Self, reason| {
            hub.send(
                session,
                ServerMessage::CommandResult {
                    client_seq,
                    result: CommandOutcome::Rejected { reason },
                },
            );
        };
        match self.phase {
            HubPhase::SeasonOver => return reject(self, RejectReason::SeasonOver),
            HubPhase::Stopped => return,
            // The server decides; the hub's view may lag behind it.
            HubPhase::Starting | HubPhase::CatchingUp | HubPhase::Live => {}
        }
        match self.submit(commander, CommanderInputKind::Command(command)) {
            Ok(seq) => {
                self.pending.insert(
                    (commander, seq),
                    Pending {
                        session,
                        client_seq,
                        before_catch_up: false,
                    },
                );
            }
            Err(SubmitError::NotLive | SubmitError::Reserved) => {
                reject(self, RejectReason::CatchingUp);
            }
            Err(SubmitError::SeasonOver) => reject(self, RejectReason::SeasonOver),
            // The Stopped event follows and says goodbye.
            Err(SubmitError::Stopped) => {}
        }
    }

    fn leave(&mut self, session: SessionId) {
        let Some(entry) = self.sessions.remove(&session) else {
            return;
        };
        if self.active.get(&entry.commander) == Some(&session) {
            self.active.remove(&entry.commander);
        }
        self.pending.retain(|_, p| p.session != session);
        self.reconcile();
    }

    /// Submits `Connected` or `Disconnected` wherever the simulation's view differs from the
    /// open sessions. The server refuses while it is not live; the next `Live` event tries
    /// again.
    fn reconcile(&mut self) {
        if matches!(self.phase, HubPhase::SeasonOver | HubPhase::Stopped) {
            return;
        }
        let wanted: BTreeSet<CommanderId> = self.active.keys().copied().collect();
        let changes: Vec<(CommanderId, bool)> = wanted
            .symmetric_difference(&self.connected)
            .map(|&commander| (commander, wanted.contains(&commander)))
            .collect();
        for (commander, connect) in changes {
            let kind = if connect {
                CommanderInputKind::Connected
            } else {
                CommanderInputKind::Disconnected
            };
            match self.submit(commander, kind) {
                Ok(_) => {
                    debug!(
                        commander = commander.0,
                        connect, "connection change submitted"
                    );
                    if connect {
                        self.connected.insert(commander);
                    } else {
                        self.connected.remove(&commander);
                    }
                }
                // Not live after all; the next Live event reconciles again.
                Err(_) => return,
            }
        }
    }

    /// Submits an input for `commander` with the next sequence number, which a refused
    /// submission gives back, so the journaled numbers have no gaps.
    fn submit(
        &mut self,
        commander: CommanderId,
        kind: CommanderInputKind,
    ) -> Result<u64, SubmitError> {
        let next = self.next_seq.entry(commander).or_insert(1);
        let seq = *next;
        let result = self.server.submit(Input::Commander(CommanderInput {
            commander,
            client_seq: seq,
            kind,
        }));
        if result.is_ok() {
            *next += 1;
        }
        result.map(|()| seq)
    }

    fn reject(&self, dropped: BTreeMap<(CommanderId, u64), Pending>, reason: RejectReason) {
        for pending in dropped.into_values() {
            self.send(
                pending.session,
                ServerMessage::CommandResult {
                    client_seq: pending.client_seq,
                    result: CommandOutcome::Rejected { reason },
                },
            );
        }
    }

    fn resync_waiting_sessions(&mut self) {
        let waiting: BTreeSet<SessionId> = self.pending.values().map(|p| p.session).collect();
        self.pending.clear();
        for session in waiting {
            if let Some(entry) = self.sessions.remove(&session) {
                let _ = entry.outbox.send(ToSession::Bye(ByeReason::Resync));
                if self.active.get(&entry.commander) == Some(&session) {
                    self.active.remove(&entry.commander);
                }
            }
        }
        self.reconcile();
    }

    fn send(&self, session: SessionId, message: ServerMessage) {
        if let Some(entry) = self.sessions.get(&session) {
            let _ = entry.outbox.send(ToSession::Message(Box::new(message)));
        }
    }

    fn broadcast(&self, message: &ServerMessage) {
        for entry in self.sessions.values() {
            let _ = entry
                .outbox
                .send(ToSession::Message(Box::new(message.clone())));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::time::Duration;

    use nomad_sim::{CommandRejection, CommandResult};
    use nomad_types::{FleetId, FleetOrder, Location, PositionMetres, Queueing, SimId, SystemId};
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    use super::*;
    use crate::server::CatchUpProgress as SimProgress;

    const HUMAN: CommanderId = CommanderId(1);

    /// Records submitted inputs; refuses them when `live` is false.
    #[derive(Default)]
    struct Recorder {
        inputs: RefCell<Vec<Input>>,
        refuse: RefCell<Option<SubmitError>>,
    }

    impl Submit for &Recorder {
        fn submit(&self, input: Input) -> Result<(), SubmitError> {
            if let Some(error) = *self.refuse.borrow() {
                return Err(error);
            }
            self.inputs.borrow_mut().push(input);
            Ok(())
        }
    }

    impl Recorder {
        fn take(&self) -> Vec<(u64, String)> {
            self.inputs
                .borrow_mut()
                .drain(..)
                .map(|input| match input {
                    Input::Commander(c) => (
                        c.client_seq,
                        match c.kind {
                            CommanderInputKind::Connected => "connected".to_owned(),
                            CommanderInputKind::Disconnected => "disconnected".to_owned(),
                            CommanderInputKind::Command(_) => "command".to_owned(),
                        },
                    ),
                    Input::Operator(op) => (0, format!("{op:?}")),
                })
                .collect()
        }
    }

    fn hub(recorder: &Recorder) -> Hub<&Recorder> {
        Hub::new(recorder, UniverseId(7), SeasonClock::new(0), Tick::new(100))
    }

    fn join(hub: &mut Hub<&Recorder>, session: SessionId) -> UnboundedReceiver<ToSession> {
        let (tx, rx) = unbounded_channel();
        hub.on_request(ToHub::Join {
            session,
            commander: HUMAN,
            outbox: tx,
        });
        rx
    }

    fn drain(rx: &mut UnboundedReceiver<ToSession>) -> Vec<ToSession> {
        let mut out = Vec::new();
        while let Ok(message) = rx.try_recv() {
            out.push(message);
        }
        out
    }

    fn command() -> Command {
        Command::Fleet {
            fleet: FleetId(SimId::from_raw(1)),
            order: FleetOrder::Move {
                to: Location {
                    system: SystemId(SimId::from_raw(2)),
                    position: PositionMetres { x: 0.0, y: 0.0 },
                },
            },
            queue: Queueing::Replace,
        }
    }

    fn send_command(hub: &mut Hub<&Recorder>, session: SessionId, client_seq: u64) {
        hub.on_request(ToHub::Command {
            session,
            client_seq,
            command: command(),
        });
    }

    fn tick(n: u64, results: &[(u64, Result<(), CommandRejection>)]) -> ServerEvent {
        ServerEvent::Tick(TickOutputs {
            tick: Tick::new(n),
            commands: results
                .iter()
                .map(|&(seq, outcome)| CommandResult {
                    commander: HUMAN,
                    client_seq: seq,
                    outcome,
                })
                .collect(),
            ..TickOutputs::default()
        })
    }

    fn catch_up(at: u64) -> ServerEvent {
        ServerEvent::CatchUp(SimProgress {
            from: Tick::new(at),
            target: Tick::new(at + 1_000),
            tick: Tick::new(at),
            elapsed: Duration::ZERO,
        })
    }

    fn result(client_seq: u64, result: CommandOutcome) -> ToSession {
        ToSession::Message(Box::new(ServerMessage::CommandResult {
            client_seq,
            result,
        }))
    }

    fn rejected(client_seq: u64, reason: RejectReason) -> ToSession {
        result(client_seq, CommandOutcome::Rejected { reason })
    }

    #[test]
    fn welcome_then_connected_once_live() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        // Starting: the server refuses inputs.
        *recorder.refuse.borrow_mut() = Some(SubmitError::NotLive);
        let mut rx = join(&mut hub, 1);
        let messages = drain(&mut rx);
        assert!(matches!(
            messages.as_slice(),
            [ToSession::Message(m)] if matches!(**m, ServerMessage::Welcome { commander, .. } if commander == HUMAN)
        ));
        assert!(recorder.take().is_empty());
        *recorder.refuse.borrow_mut() = None;
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        // The refused attempt gave its number back.
        assert_eq!(recorder.take(), [(1, "connected".to_owned())]);
    }

    /// The hub hears of `Live` after the server went live. A command in between is the
    /// server's to accept, and must not be taken for one dropped by a catch-up.
    #[test]
    fn a_command_accepted_before_the_hub_hears_of_live_is_kept() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        let mut rx = join(&mut hub, 1);
        send_command(&mut hub, 1, 7);
        assert_eq!(
            recorder.take(),
            [(1, "connected".to_owned()), (2, "command".to_owned())]
        );
        drain(&mut rx);
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        assert!(drain(&mut rx).is_empty());
        hub.on_event(tick(101, &[(2, Ok(()))]));
        assert_eq!(
            drain(&mut rx),
            [result(
                7,
                CommandOutcome::Applied {
                    tick: Tick::new(101)
                }
            )]
        );
        // Already connected: Live submits nothing more.
        assert!(recorder.take().is_empty());
    }

    #[test]
    fn commands_get_their_outcomes_with_the_hub_sequence() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        let mut rx = join(&mut hub, 1);
        drain(&mut rx);
        send_command(&mut hub, 1, 40);
        send_command(&mut hub, 1, 41);
        // seq 1 was the Connected input; the commands got 2 and 3.
        assert_eq!(
            recorder.take(),
            [
                (1, "connected".to_owned()),
                (2, "command".to_owned()),
                (3, "command".to_owned())
            ]
        );
        hub.on_event(tick(
            101,
            &[(2, Ok(())), (3, Err(CommandRejection::NotOwner))],
        ));
        assert_eq!(
            drain(&mut rx),
            [
                result(
                    40,
                    CommandOutcome::Applied {
                        tick: Tick::new(101)
                    }
                ),
                rejected(
                    41,
                    RejectReason::Invalid {
                        reason: nomad_types::InvalidReason::NotFound
                    }
                ),
            ]
        );
    }

    #[test]
    fn catch_up_refuses_commands_and_reconnects_afterwards() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        let mut rx = join(&mut hub, 1);
        send_command(&mut hub, 1, 1); // sim seq 2, in flight
        drain(&mut rx);
        recorder.take();

        // D-117: the computer slept. The simulation disconnects everyone and refuses inputs.
        hub.on_event(catch_up(101));
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [ToSession::Message(m)] if matches!(**m, ServerMessage::CatchUp { .. })
        ));
        *recorder.refuse.borrow_mut() = Some(SubmitError::NotLive);
        send_command(&mut hub, 1, 2);
        assert_eq!(drain(&mut rx), [rejected(2, RejectReason::CatchingUp)]);
        assert!(recorder.take().is_empty());
        *recorder.refuse.borrow_mut() = None;

        // Live again: the in-flight command was dropped, and the session reconnects.
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(1_101),
        });
        assert_eq!(drain(&mut rx), [rejected(1, RejectReason::CatchingUp)]);
        assert_eq!(recorder.take(), [(3, "connected".to_owned())]);
    }

    #[test]
    fn a_command_applied_on_the_first_catch_up_tick_is_answered() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        let mut rx = join(&mut hub, 1);
        send_command(&mut hub, 1, 9);
        drain(&mut rx);
        hub.on_event(catch_up(100));
        // The simulation publishes catch-up ticks that carry command outcomes.
        hub.on_event(tick(101, &[(2, Ok(()))]));
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(1_100),
        });
        let messages = drain(&mut rx);
        assert!(messages.contains(&result(
            9,
            CommandOutcome::Applied {
                tick: Tick::new(101)
            }
        )));
        assert!(!messages.contains(&rejected(9, RejectReason::CatchingUp)));
    }

    #[test]
    fn a_new_session_replaces_the_old_one_without_reconnecting() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        let mut first = join(&mut hub, 1);
        send_command(&mut hub, 1, 5);
        drain(&mut first);
        recorder.take();
        let mut second = join(&mut hub, 2);
        assert_eq!(drain(&mut first), [ToSession::Bye(ByeReason::Replaced)]);
        assert!(matches!(
            drain(&mut second).as_slice(),
            [ToSession::Message(m)] if matches!(**m, ServerMessage::Welcome { .. })
        ));
        // No Disconnected/Connected pair (D-124).
        assert!(recorder.take().is_empty());
        // The old session's late Leave changes nothing; its result goes nowhere.
        hub.on_request(ToHub::Leave { session: 1 });
        hub.on_event(tick(101, &[(2, Ok(()))]));
        assert!(recorder.take().is_empty());
        assert!(drain(&mut second).is_empty());
        // The new session leaving disconnects the commander.
        hub.on_request(ToHub::Leave { session: 2 });
        assert_eq!(recorder.take(), [(3, "disconnected".to_owned())]);
    }

    #[test]
    fn season_over_refuses_commands_and_answers_dropped_ones() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        let mut rx = join(&mut hub, 1);
        send_command(&mut hub, 1, 1);
        drain(&mut rx);
        hub.on_event(ServerEvent::SeasonOver {
            tick: Tick::new(101),
            result: nomad_sim::SeasonResult {
                tick: Tick::new(101),
                reason: nomad_sim::SeasonEndReason::FinalTick,
                winners: vec![CommanderId(1)],
            },
        });
        assert_eq!(drain(&mut rx), [rejected(1, RejectReason::SeasonOver)]);
        send_command(&mut hub, 1, 2);
        assert_eq!(drain(&mut rx), [rejected(2, RejectReason::SeasonOver)]);
    }

    #[test]
    fn lost_events_resync_sessions_that_wait_for_results() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        let mut rx = join(&mut hub, 1);
        send_command(&mut hub, 1, 1);
        drain(&mut rx);
        recorder.take();
        hub.on_event(tick(101, &[]));
        hub.on_event(tick(105, &[])); // 102 to 104 were lost
        assert_eq!(drain(&mut rx), [ToSession::Bye(ByeReason::Resync)]);
        assert_eq!(recorder.take(), [(3, "disconnected".to_owned())]);
    }

    #[test]
    fn stopping_says_goodbye_and_refuses_new_sessions() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        let mut rx = join(&mut hub, 1);
        drain(&mut rx);
        hub.on_event(ServerEvent::Stopped);
        assert!(hub.stopped());
        assert_eq!(drain(&mut rx), [ToSession::Bye(ByeReason::ShuttingDown)]);
        let mut late = join(&mut hub, 2);
        assert_eq!(drain(&mut late), [ToSession::Bye(ByeReason::ShuttingDown)]);
    }

    #[test]
    fn a_refused_connection_change_is_retried_on_the_next_live_tick() {
        let recorder = Recorder::default();
        let mut hub = hub(&recorder);
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(100),
        });
        *recorder.refuse.borrow_mut() = Some(SubmitError::NotLive);
        let mut rx = join(&mut hub, 1);
        drain(&mut rx);
        assert!(recorder.take().is_empty());
        *recorder.refuse.borrow_mut() = None;
        hub.on_event(catch_up(100));
        hub.on_event(ServerEvent::Live {
            tick: Tick::new(200),
        });
        // The refused attempt gave its number back.
        assert_eq!(recorder.take(), [(1, "connected".to_owned())]);
    }
}
