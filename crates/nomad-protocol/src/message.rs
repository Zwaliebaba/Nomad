//! The two message enums, one per direction (Design doc 20 §5, D-087).
//!
//! Payloads are kept minimal: they carry what doc 20 §5 names and nothing that a rule not
//! yet agreed would decide. Adding a variant or a field changes the encoding, so it
//! increments [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION); the frozen variants are listed
//! in the [`version`](crate::version) module docs.
//!
//! Times are ticks, never countdowns (doc 11 §3). Clients turn ticks into Universe time with
//! the [`SeasonClock`] from [`ServerMessage::Welcome`] and render countdowns locally.

use std::fmt;

use nomad_types::{
    Command, CommanderId, HoldingId, InvalidReason, SeasonClock, SimId, Tick, UniverseId,
};
use serde::{Deserialize, Serialize};

use crate::frame::{MAX_CLIENT_FRAME_LEN, MAX_SERVER_FRAME_LEN, WireMessage};
use crate::view::{ViewChanges, ViewSnapshot};

/// A message from the client to the server (doc 20 §5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// The first message of a session: the version check. **Frozen:** variant 0, with
    /// `protocol` first ([`version`](crate::version)).
    Hello {
        /// The client's [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION).
        protocol: u32,
        /// The client's build, for logs and diagnostics.
        build: String,
    },
    /// Authentication with the local token (doc 20 §9).
    Auth {
        /// The token.
        token: AuthToken,
    },
    /// A player command. The server checks only the envelope (size, rate, session); the
    /// simulation's single validator checks its meaning (D-077). A command received during
    /// tick *t* applies at *t + 1* (doc 11 §6).
    Command {
        /// The client's sequence number for this command, echoed in
        /// [`ServerMessage::CommandResult`]. It must strictly increase within a session; the
        /// server ends a session that repeats or lowers it (N3 plan §4.4).
        client_seq: u64,
        /// The command.
        command: Command,
    },
    /// Liveness and round-trip time. The server answers at once with
    /// [`ServerMessage::Pong`] carrying the same nonce, outside the tick.
    Ping {
        /// Any value; echoed in the `Pong`, so the client can match replies to requests.
        nonce: u64,
    },
}

impl WireMessage for ClientMessage {
    const MAX_FRAME_LEN: u32 = MAX_CLIENT_FRAME_LEN;
}

/// A message from the server to the client (doc 20 §5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// The end of the session. **Frozen:** variant 0 ([`version`](crate::version)), so that a
    /// client of any version can read a version mismatch.
    Bye {
        /// Why the session ends.
        reason: ByeReason,
    },
    /// The start of a session, after a successful `Hello` and `Auth`.
    Welcome {
        /// The commander this session controls.
        commander: CommanderId,
        /// The Universe the server hosts.
        universe: UniverseId,
        /// The last completed tick.
        tick: Tick,
        /// The tick-to-UTC mapping, for Universe time on the client (doc 11 §2).
        clock: SeasonClock,
    },
    /// Progress while the server catches up after a start (D-047, doc 20 §7). Commands are
    /// rejected until it is live.
    CatchUp {
        /// How far it has got.
        progress: CatchUpProgress,
    },
    /// The outcome of one [`ClientMessage::Command`] (D-087).
    CommandResult {
        /// The command's `client_seq`.
        client_seq: u64,
        /// Applied, with its tick, or rejected, with the reason.
        result: CommandOutcome,
    },
    /// The commander's whole view (doc 16 §6); sent first, then deltas.
    ViewSnapshot(ViewSnapshot),
    /// Changes to the commander's view since the previous snapshot or delta (doc 16 §6).
    ViewDelta {
        /// The tick the view now reflects.
        tick: Tick,
        /// What changed.
        changes: ViewChanges,
    },
    /// Siege information addressed to this commander (doc 16 §2, D-075).
    Announcement(Announcement),
    /// An alert for the client's notification banner and toasts (doc 17 §6, D-092).
    Notification(Notification),
    /// The reply to [`ClientMessage::Ping`].
    Pong {
        /// The `Ping`'s nonce.
        nonce: u64,
    },
}

impl WireMessage for ServerMessage {
    const MAX_FRAME_LEN: u32 = MAX_SERVER_FRAME_LEN;
}

/// The local authentication token (doc 20 §9). Its `Debug` output hides the value, so that a
/// logged message never leaks it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthToken(String);

impl AuthToken {
    /// Wraps a token.
    pub const fn new(token: String) -> Self {
        Self(token)
    }

    /// The token's value, for comparing it with the stored one.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthToken(<redacted>)")
    }
}

/// Why the server ends a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ByeReason {
    /// The client's protocol version is not the server's. **Frozen:** variant 0, with this
    /// one field ([`version`](crate::version)).
    VersionMismatch {
        /// The server's [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION).
        server: u32,
    },
    /// The token was missing or wrong (doc 20 §9).
    AuthenticationFailed,
    /// The server is shutting down cleanly (doc 20 §7).
    ShuttingDown,
    /// The client sent something the protocol does not allow, such as a malformed or
    /// oversized frame, a message out of order or a `client_seq` that did not increase
    /// (doc 20 §9).
    ProtocolViolation,
    /// `Hello` or `Auth` did not arrive in time (N3 plan §4.6).
    HandshakeTimeout,
    /// The server lost track of this session's results, for example because it fell behind
    /// on the simulation's events. Reconnect; commands without a result may or may not have
    /// applied (N3 plan §4.1).
    Resync,
    /// A newer session for the same commander replaced this one (D-124).
    Replaced,
}

/// How far a catch-up has got (D-047). Two ticks, not a percentage, so that the client
/// computes progress and an estimate itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatchUpProgress {
    /// The last tick computed so far.
    pub reached: Tick,
    /// The tick that corresponds to the wall clock now; catch-up ends when it is reached.
    pub target: Tick,
}

/// The outcome of a command (doc 20 §5, D-087).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandOutcome {
    /// Accepted; it applies on this tick (doc 11 §6).
    Applied {
        /// The tick it applies on.
        tick: Tick,
    },
    /// Not applied.
    Rejected {
        /// Why.
        reason: RejectReason,
    },
}

/// Why a command was rejected. The envelope reasons come from doc 20 §5, §7 and §9; the
/// simulation's validator gives [`RejectReason::Invalid`] (D-077, D-123).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectReason {
    /// The server is still catching up (doc 20 §7).
    CatchingUp,
    /// The session's command rate limit was exceeded (doc 20 §9).
    RateLimited,
    /// The session is not authenticated.
    NotAuthenticated,
    /// The simulation's validator rejected it (D-077). The reason withholds whether unseen
    /// entities exist (D-123).
    Invalid {
        /// Why.
        reason: InvalidReason,
    },
    /// The season has ended; the Universe no longer advances (D-116).
    SeasonOver,
}

/// Siege information (doc 12 §4; doc 16 §2). It goes to the owner and to the commanders with
/// breach credit (D-057, D-075).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Announcement {
    /// A holding was breached and is Reinforced until its siege starts (D-015).
    Reinforced {
        /// The holding.
        holding: HoldingId,
        /// The tick its siege starts: the Reinforced exit time.
        siege_start: Tick,
    },
    /// A holding's siege has started.
    SiegeStarted {
        /// The holding.
        holding: HoldingId,
        /// The tick its siege clock runs out (D-015).
        siege_end: Tick,
    },
}

/// An in-client alert (doc 17 §6, D-092).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    /// When it happened.
    pub tick: Tick,
    /// What it is about, so the client can jump to it (doc 17 §6), if anything.
    pub subject: Option<SimId>,
    /// What happened.
    pub kind: NotificationKind,
}

/// What a notification is about: the banner and toast cases of doc 17 §6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationKind {
    /// One of the commander's holdings was breached.
    Breach,
    /// A siege that involves the commander started.
    Siege,
    /// Raiders attacked one of the commander's assets.
    Raid,
}
