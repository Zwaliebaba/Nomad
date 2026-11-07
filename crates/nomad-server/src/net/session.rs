//! One client connection (N3 plan §4.3, §4.4, §4.6; doc 20 §5, §9).
//!
//! 1. The client opens the control stream and sends `Hello` within the handshake timeout.
//!    The version is read from the frozen prefix (`hello_protocol`), so any client learns of
//!    a mismatch.
//! 2. `Auth` with the token, within the timeout, compared in constant time.
//! 3. The hub sends `Welcome`; the server opens the state stream (empty until N4).
//! 4. Commands pass the envelope checks (strictly increasing `client_seq`, the rate limit)
//!    and go to the hub; `Ping` is answered at once with its nonce.
//!
//! Anything else (a malformed or oversized frame, a message out of order) ends the session
//! with `Bye { ProtocolViolation }`. Nothing a client sends can panic the server.

use std::sync::Arc;
use std::time::Duration;

use nomad_protocol::{
    ByeReason, ClientMessage, CommandOutcome, FrameDecoder, PROTOCOL_VERSION, RejectReason,
    ServerMessage, check_version, decode_body, encode_frame, hello_protocol,
};
use nomad_sim::CommanderId;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{Instant, timeout_at};
use tracing::{debug, info, warn};

use super::Limits;
use super::hub::{SessionId, ToHub, ToSession};
use super::identity::Identity;

/// QUIC application close codes.
const CLOSE_NORMAL: u32 = 0;
const CLOSE_PROTOCOL: u32 = 1;
/// How long to wait for the client to receive a `Bye` before closing the connection.
const BYE_LINGER: Duration = Duration::from_secs(1);
/// Largest read from the control stream at once.
const READ_CHUNK: usize = 64 * 1024;

/// What a session needs from the server.
pub struct SessionContext {
    pub session: SessionId,
    pub commander: CommanderId,
    pub identity: Arc<Identity>,
    pub limits: Limits,
    pub hub: UnboundedSender<ToHub>,
}

/// Why the session ended, for the log.
#[derive(Debug)]
enum End {
    Bye(ByeReason),
    ClientClosed,
    ConnectionLost(String),
}

impl std::fmt::Display for End {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bye(reason) => write!(f, "bye: {reason:?}"),
            Self::ClientClosed => f.write_str("the client closed the session"),
            Self::ConnectionLost(why) => write!(f, "connection lost: {why}"),
        }
    }
}

/// Runs one connection to its end.
pub async fn run(connection: quinn::Connection, context: SessionContext) {
    let started = Instant::now();
    let remote = connection.remote_address();
    let end = match handshake(&connection, &context).await {
        Ok(Some(streams)) => {
            let end = serve(&connection, streams, &context).await;
            let _ = context.hub.send(ToHub::Leave {
                session: context.session,
            });
            end
        }
        Ok(None) => End::ClientClosed,
        Err(end) => end,
    };
    let code = match &end {
        End::Bye(ByeReason::ProtocolViolation | ByeReason::HandshakeTimeout) => CLOSE_PROTOCOL,
        _ => CLOSE_NORMAL,
    };
    connection.close(code.into(), b"");
    info!(
        session = context.session,
        %remote,
        reason = %end,
        duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "session ended"
    );
}

/// The control stream after a successful handshake.
struct Streams {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    decoder: FrameDecoder<ClientMessage>,
    outbox: UnboundedReceiver<ToSession>,
    /// The state stream, opened for N4.
    _state: Option<quinn::SendStream>,
}

/// Hello, Auth and Welcome. `Ok(None)` if the client went away before opening a stream.
async fn handshake(
    connection: &quinn::Connection,
    context: &SessionContext,
) -> Result<Option<Streams>, End> {
    let limits = &context.limits;
    let hello_deadline = Instant::now() + limits.hello_timeout;
    let (mut send, mut recv) = match timeout_at(hello_deadline, connection.accept_bi()).await {
        Ok(Ok(streams)) => streams,
        Ok(Err(_)) => return Ok(None),
        Err(_) => return Err(End::Bye(ByeReason::HandshakeTimeout)),
    };
    let mut decoder = FrameDecoder::<ClientMessage>::new();

    // Hello: the version first, from the frozen prefix.
    let body = match read_frame(&mut recv, &mut decoder, hello_deadline).await {
        Ok(body) => body,
        Err(end) => return Err(bye(&mut send, end).await),
    };
    let Ok(version) = hello_protocol(&body) else {
        return Err(bye(&mut send, End::Bye(ByeReason::ProtocolViolation)).await);
    };
    if check_version(version).is_err() {
        info!(
            session = context.session,
            client = version,
            server = PROTOCOL_VERSION,
            "protocol version mismatch"
        );
        let reason = ByeReason::VersionMismatch {
            server: PROTOCOL_VERSION,
        };
        return Err(bye(&mut send, End::Bye(reason)).await);
    }
    let Ok(ClientMessage::Hello { build, .. }) = decode_body::<ClientMessage>(&body) else {
        return Err(bye(&mut send, End::Bye(ByeReason::ProtocolViolation)).await);
    };

    // Auth.
    let auth_deadline = Instant::now() + limits.auth_timeout;
    let body = match read_frame(&mut recv, &mut decoder, auth_deadline).await {
        Ok(body) => body,
        Err(end) => return Err(bye(&mut send, end).await),
    };
    match decode_body::<ClientMessage>(&body) {
        Ok(ClientMessage::Auth { token }) if context.identity.token_matches(token.as_str()) => {}
        Ok(ClientMessage::Auth { .. }) => {
            // The token is never logged (AuthToken's Debug hides it).
            warn!(session = context.session, "authentication failed");
            return Err(bye(&mut send, End::Bye(ByeReason::AuthenticationFailed)).await);
        }
        _ => return Err(bye(&mut send, End::Bye(ByeReason::ProtocolViolation)).await),
    }

    let (outbox_tx, outbox) = mpsc::unbounded_channel();
    if context
        .hub
        .send(ToHub::Join {
            session: context.session,
            commander: context.commander,
            outbox: outbox_tx,
        })
        .is_err()
    {
        return Err(bye(&mut send, End::Bye(ByeReason::ShuttingDown)).await);
    }
    info!(
        session = context.session,
        commander = context.commander.0,
        client_build = %build,
        "session started"
    );
    // The state stream; N4 sends the view on it.
    let state = connection.open_uni().await.ok();
    Ok(Some(Streams {
        send,
        recv,
        decoder,
        outbox,
        _state: state,
    }))
}

/// The session after the handshake, until it ends.
async fn serve(connection: &quinn::Connection, mut s: Streams, context: &SessionContext) -> End {
    let mut limiter = RateLimiter::new(context.limits);
    let mut last_seq: Option<u64> = None;
    let mut buffer = vec![0; READ_CHUNK];
    loop {
        // Messages already buffered first.
        loop {
            match s.decoder.next_message() {
                Ok(Some(message)) => {
                    if let Err(end) =
                        handle(message, &mut s, context, &mut limiter, &mut last_seq).await
                    {
                        return bye(&mut s.send, end).await;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    debug!(session = context.session, %error, "bad frame");
                    return bye(&mut s.send, End::Bye(ByeReason::ProtocolViolation)).await;
                }
            }
        }
        tokio::select! {
            read = s.recv.read(&mut buffer) => match read {
                Ok(Some(n)) => s.decoder.push(&buffer[..n]),
                Ok(None) => return End::ClientClosed,
                Err(error) => return End::ConnectionLost(error.to_string()),
            },
            message = s.outbox.recv() => match message {
                Some(ToSession::Message(message)) => {
                    if let Err(end) = write(&mut s.send, &message).await {
                        return end;
                    }
                }
                Some(ToSession::Bye(reason)) => return bye(&mut s.send, End::Bye(reason)).await,
                // The hub is gone: the server is stopping.
                None => return bye(&mut s.send, End::Bye(ByeReason::ShuttingDown)).await,
            },
            reason = connection.closed() => return End::ConnectionLost(reason.to_string()),
        }
    }
}

/// One message from an authenticated client.
async fn handle(
    message: ClientMessage,
    s: &mut Streams,
    context: &SessionContext,
    limiter: &mut RateLimiter,
    last_seq: &mut Option<u64>,
) -> Result<(), End> {
    match message {
        ClientMessage::Command {
            client_seq,
            command,
        } => {
            if last_seq.is_some_and(|last| client_seq <= last) {
                return Err(End::Bye(ByeReason::ProtocolViolation));
            }
            *last_seq = Some(client_seq);
            if !limiter.take() {
                let reply = ServerMessage::CommandResult {
                    client_seq,
                    result: CommandOutcome::Rejected {
                        reason: RejectReason::RateLimited,
                    },
                };
                return write(&mut s.send, &reply).await;
            }
            context
                .hub
                .send(ToHub::Command {
                    session: context.session,
                    client_seq,
                    command,
                })
                .map_err(|_| End::Bye(ByeReason::ShuttingDown))
        }
        ClientMessage::Ping { nonce } => write(&mut s.send, &ServerMessage::Pong { nonce }).await,
        ClientMessage::Hello { .. } | ClientMessage::Auth { .. } => {
            Err(End::Bye(ByeReason::ProtocolViolation))
        }
    }
}

/// Reads the next frame body before `deadline`.
async fn read_frame(
    recv: &mut quinn::RecvStream,
    decoder: &mut FrameDecoder<ClientMessage>,
    deadline: Instant,
) -> Result<Vec<u8>, End> {
    let mut buffer = vec![0; READ_CHUNK];
    loop {
        match decoder.next_frame() {
            Ok(Some(body)) => return Ok(body),
            Ok(None) => {}
            // Malformed or oversized (ProtocolError): nothing more is read.
            Err(_) => return Err(End::Bye(ByeReason::ProtocolViolation)),
        }
        match timeout_at(deadline, recv.read(&mut buffer)).await {
            Ok(Ok(Some(n))) => decoder.push(&buffer[..n]),
            Ok(Ok(None)) => return Err(End::ClientClosed),
            Ok(Err(error)) => return Err(End::ConnectionLost(error.to_string())),
            Err(_) => return Err(End::Bye(ByeReason::HandshakeTimeout)),
        }
    }
}

async fn write(send: &mut quinn::SendStream, message: &ServerMessage) -> Result<(), End> {
    let mut frame = Vec::new();
    encode_frame(message, &mut frame).map_err(|e| End::ConnectionLost(e.to_string()))?;
    send.write_all(&frame)
        .await
        .map_err(|e| End::ConnectionLost(e.to_string()))
}

/// Sends `Bye` for `end`, if it is one, and gives the client a moment to read it.
async fn bye(send: &mut quinn::SendStream, end: End) -> End {
    if let End::Bye(reason) = &end {
        let _ = write(send, &ServerMessage::Bye { reason: *reason }).await;
        let _ = send.finish();
        let _ = tokio::time::timeout(BYE_LINGER, send.stopped()).await;
    }
    end
}

/// A token bucket: `commands_per_second` sustained, bursts of `command_burst` (§4.6).
struct RateLimiter {
    tokens: f64,
    capacity: f64,
    per_second: f64,
    last: Instant,
}

impl RateLimiter {
    fn new(limits: Limits) -> Self {
        let capacity = f64::from(limits.command_burst.max(1));
        Self {
            tokens: capacity,
            capacity,
            per_second: f64::from(limits.commands_per_second),
            last: Instant::now(),
        }
    }

    fn take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.per_second).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}
