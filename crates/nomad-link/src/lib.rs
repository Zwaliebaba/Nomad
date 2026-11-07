//! The client side of a Nomad connection (Design doc 20 §5, §8; N3 plan §4.8; D-125).
//!
//! - **Finding the server** ([`ConnectSource`]): the connection file `server.json` in a
//!   Universe's directory, which a local server writes (D-120, D-150), or connection details
//!   imported once from `nomad-server connect-info`, for a server whose files the client
//!   cannot read, such as one in a container (D-151, D-152).
//! - **[`Connection`]:** one QUIC connection with the pinned certificate and the control
//!   stream, driven with `async` calls; for tests and tools.
//! - **[`Link`]:** the same on a background thread with `std` channels and reconnects, for
//!   the game client (C7). The client's main thread never blocks on the network.
//!
//! The link moves messages and nothing else: the view-model store, the catch-up screen and
//! command bookkeeping belong to the client.

mod connection;
mod link;
mod tls;

use std::io;
use std::path::PathBuf;

use nomad_protocol::{
    ByeReason, CONNECTION_FILE, ConnectInfo, ConnectInfoError, ConnectionFile, ProtocolError,
};
use thiserror::Error;

pub use connection::{Connection, Welcome, connect};
pub use link::{Link, LinkEvent};
pub use tls::{ALPN, SERVER_NAME};

/// Where the link finds a server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectSource {
    /// The connection file of a server on this machine, read afresh at every attempt.
    UniverseDir(PathBuf),
    /// Details imported from `connect-info` (D-151): fixed until imported again.
    Imported(ConnectInfo),
}

impl ConnectSource {
    /// The connection details to use now.
    pub fn resolve(&self) -> Result<ConnectInfo, LinkError> {
        match self {
            Self::UniverseDir(dir) => read_connection_file(dir),
            Self::Imported(info) => Ok(info.clone()),
        }
    }
}

/// Reads the connection file in a Universe's directory.
pub fn read_connection_file(dir: &std::path::Path) -> Result<ConnectInfo, LinkError> {
    let path = dir.join(CONNECTION_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            LinkError::NotRunning(path.clone())
        } else {
            LinkError::Io(e)
        }
    })?;
    let file: ConnectionFile =
        serde_json::from_str(&text).map_err(|e| LinkError::ConnectionFile(e.to_string()))?;
    file.connect_info().map_err(LinkError::Details)
}

/// Why connecting or talking to the server failed.
#[derive(Debug, Error)]
pub enum LinkError {
    /// No connection file: the server is not running there.
    #[error("no server is running ({} is missing)", .0.display())]
    NotRunning(PathBuf),
    /// The connection file is not readable.
    #[error("the connection file is damaged: {0}")]
    ConnectionFile(String),
    /// The connection details are malformed.
    #[error("connection details: {0}")]
    Details(ConnectInfoError),
    /// A socket or thread could not be set up.
    #[error("{0}")]
    Io(io::Error),
    /// The QUIC connection could not be made.
    #[error("cannot connect: {0}")]
    Connect(String),
    /// The connection broke.
    #[error("connection lost: {0}")]
    ConnectionLost(String),
    /// The server finished the session.
    #[error("the server closed the session")]
    Closed,
    /// The server ended the session with a reason.
    #[error("the server ended the session: {0:?}")]
    Bye(ByeReason),
    /// The server sent bytes that are not a valid message.
    #[error("protocol error: {0}")]
    Protocol(ProtocolError),
    /// The server sent a message out of order.
    #[error("unexpected message: {0}")]
    Unexpected(String),
}
