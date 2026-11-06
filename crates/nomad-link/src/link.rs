//! The client link on its own thread (N3 plan §4.8; doc 20 §8).
//!
//! The caller sends [`ClientMessage`]s with [`Link::send`] and reads [`LinkEvent`]s from a
//! `std` channel, so the client's main thread never blocks on the network. The link
//! reconnects with backoff (0.5 s, doubling, up to 5 s), reading the connection details
//! afresh each time, and reports every connect and disconnect: after a reconnect the
//! client's command bookkeeping starts over. Messages sent while disconnected are dropped.

use std::sync::mpsc as std_mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nomad_protocol::{ByeReason, ClientMessage, ServerMessage};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{debug, info};

use crate::connection::{Connection, Welcome, connect};
use crate::{ConnectSource, LinkError};

const FIRST_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(5);

/// What the link reports.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkEvent {
    /// Connected and authenticated; messages follow.
    Connected(Welcome),
    /// A message from the server. `Welcome` arrives as [`LinkEvent::Connected`] instead.
    Message(ServerMessage),
    /// The connection ended or could not be made. With `retrying` the link tries again;
    /// without it the link has stopped (a version mismatch, a refused token from imported
    /// details, or a newer session that replaced this one).
    Disconnected {
        /// Why.
        reason: String,
        /// Whether the link will try again.
        retrying: bool,
    },
}

/// A connection to a server, kept up on a background thread.
#[derive(Debug)]
pub struct Link {
    outgoing: Option<UnboundedSender<ClientMessage>>,
    events: std_mpsc::Receiver<LinkEvent>,
    thread: Option<JoinHandle<()>>,
}

impl Link {
    /// Starts the link thread; it connects in the background. `build` is sent in `Hello`.
    pub fn start(source: ConnectSource, build: impl Into<String>) -> Result<Self, LinkError> {
        let build = build.into();
        let (outgoing, outgoing_rx) = unbounded_channel();
        let (events_tx, events) = std_mpsc::channel();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(LinkError::Io)?;
        let thread = thread::Builder::new()
            .name("nomad-link".to_owned())
            .spawn(move || runtime.block_on(run(source, build, outgoing_rx, events_tx)))
            .map_err(LinkError::Io)?;
        Ok(Self {
            outgoing: Some(outgoing),
            events,
            thread: Some(thread),
        })
    }

    /// Queues a message for the server. Dropped if the link is not connected.
    pub fn send(&self, message: ClientMessage) -> Result<(), LinkError> {
        self.outgoing
            .as_ref()
            .ok_or(LinkError::Closed)?
            .send(message)
            .map_err(|_| LinkError::Closed)
    }

    /// The events, in order.
    pub fn events(&self) -> &std_mpsc::Receiver<LinkEvent> {
        &self.events
    }
}

impl Drop for Link {
    /// Closes the connection and stops the thread.
    fn drop(&mut self) {
        self.outgoing = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How a connection ended.
enum Ended {
    /// The caller dropped the link.
    Stop,
    /// Try again.
    Retry(String),
    /// Do not try again.
    Fatal(String),
}

async fn run(
    source: ConnectSource,
    build: String,
    mut outgoing: UnboundedReceiver<ClientMessage>,
    events: std_mpsc::Sender<LinkEvent>,
) {
    let mut backoff = FIRST_BACKOFF;
    loop {
        let ended = match source.resolve() {
            Err(e) => Ended::Retry(e.to_string()),
            Ok(info) => match connect(&info, &build).await {
                Ok((connection, welcome)) => {
                    backoff = FIRST_BACKOFF;
                    info!(address = %info.address, "connected");
                    if events.send(LinkEvent::Connected(welcome)).is_err() {
                        return;
                    }
                    pump(connection, &mut outgoing, &events).await
                }
                Err(LinkError::Bye(ByeReason::VersionMismatch { server })) => Ended::Fatal(
                    format!("the server speaks protocol {server}; update the client or server"),
                ),
                Err(LinkError::Bye(ByeReason::AuthenticationFailed))
                    if matches!(source, ConnectSource::Imported(_)) =>
                {
                    Ended::Fatal("the token was refused; import fresh connection details".into())
                }
                Err(e) => Ended::Retry(e.to_string()),
            },
        };
        match ended {
            Ended::Stop => return,
            Ended::Fatal(reason) => {
                let _ = events.send(LinkEvent::Disconnected {
                    reason,
                    retrying: false,
                });
                return;
            }
            Ended::Retry(reason) => {
                debug!(%reason, "disconnected; retrying");
                if events
                    .send(LinkEvent::Disconnected {
                        reason,
                        retrying: true,
                    })
                    .is_err()
                {
                    return;
                }
            }
        }
        // Wait, dropping what the caller sends meanwhile; stop if it drops the link.
        let wake = tokio::time::sleep(backoff);
        tokio::pin!(wake);
        loop {
            tokio::select! {
                () = &mut wake => break,
                message = outgoing.recv() => if message.is_none() { return },
            }
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Moves messages both ways until the connection ends.
async fn pump(
    mut connection: Connection,
    outgoing: &mut UnboundedReceiver<ClientMessage>,
    events: &std_mpsc::Sender<LinkEvent>,
) -> Ended {
    loop {
        tokio::select! {
            message = connection.recv() => match message {
                Ok(ServerMessage::Bye { reason }) => {
                    return match reason {
                        ByeReason::Replaced => {
                            Ended::Fatal("a newer session replaced this one".to_owned())
                        }
                        ByeReason::VersionMismatch { server } => {
                            Ended::Fatal(format!("the server speaks protocol {server}"))
                        }
                        other => Ended::Retry(format!("the server ended the session: {other:?}")),
                    };
                }
                Ok(message) => {
                    if events.send(LinkEvent::Message(message)).is_err() {
                        connection.close().await;
                        return Ended::Stop;
                    }
                }
                Err(e) => return Ended::Retry(e.to_string()),
            },
            message = outgoing.recv() => {
                let Some(message) = message else {
                    connection.close().await;
                    return Ended::Stop;
                };
                if let Err(e) = connection.send(&message).await {
                    return Ended::Retry(e.to_string());
                }
            }
        }
    }
}
