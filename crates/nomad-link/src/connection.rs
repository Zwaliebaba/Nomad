//! One connection, driven with `async` calls: for tests, tools and the [`Link`](crate::Link)
//! thread.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use nomad_protocol::{
    AuthToken, ClientMessage, ConnectInfo, FrameDecoder, PROTOCOL_VERSION, ServerMessage,
    encode_frame,
};
use nomad_types::{CommanderId, SeasonClock, Tick, UniverseId};

use crate::LinkError;
use crate::tls::{SERVER_NAME, client_config};

/// Largest read at once.
const READ_CHUNK: usize = 256 * 1024;

/// What the server said in `Welcome`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Welcome {
    /// The commander this session controls.
    pub commander: CommanderId,
    /// The Universe.
    pub universe: UniverseId,
    /// The server's last completed tick.
    pub tick: Tick,
    /// The tick-to-UTC mapping (doc 11 §2).
    pub clock: SeasonClock,
}

/// A QUIC connection with its control stream (N3 plan §4.3).
#[derive(Debug)]
pub struct Connection {
    endpoint: quinn::Endpoint,
    quic: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    decoder: FrameDecoder<ServerMessage>,
    buffer: Vec<u8>,
}

impl Connection {
    /// Opens the QUIC connection to `info.address`, pinning `info.fingerprint`, and the
    /// control stream. Sends nothing yet. Must run inside a `tokio` runtime.
    pub async fn open(info: &ConnectInfo) -> Result<Self, LinkError> {
        let local = SocketAddr::new(
            match info.address.ip() {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            },
            0,
        );
        let mut endpoint = quinn::Endpoint::client(local).map_err(LinkError::Io)?;
        endpoint.set_default_client_config(
            client_config(info.fingerprint).map_err(LinkError::Connect)?,
        );
        let connection = endpoint
            .connect(info.address, SERVER_NAME)
            .map_err(|e| LinkError::Connect(e.to_string()))?
            .await
            .map_err(|e| LinkError::Connect(e.to_string()))?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|e| LinkError::Connect(e.to_string()))?;
        Ok(Self {
            endpoint,
            quic: connection,
            send,
            recv,
            decoder: FrameDecoder::new(),
            buffer: vec![0; READ_CHUNK],
        })
    }

    /// Sends `Hello` and `Auth`, and waits for `Welcome`. A `Bye` instead becomes
    /// [`LinkError::Bye`].
    pub async fn handshake(
        &mut self,
        token: &AuthToken,
        build: &str,
    ) -> Result<Welcome, LinkError> {
        self.send(&ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            build: build.to_owned(),
        })
        .await?;
        self.send(&ClientMessage::Auth {
            token: token.clone(),
        })
        .await?;
        match self.recv().await? {
            ServerMessage::Welcome {
                commander,
                universe,
                tick,
                clock,
            } => Ok(Welcome {
                commander,
                universe,
                tick,
                clock,
            }),
            ServerMessage::Bye { reason } => Err(LinkError::Bye(reason)),
            other => Err(LinkError::Unexpected(format!("{other:?}"))),
        }
    }

    /// Sends one message.
    pub async fn send(&mut self, message: &ClientMessage) -> Result<(), LinkError> {
        let mut frame = Vec::new();
        encode_frame(message, &mut frame).map_err(LinkError::Protocol)?;
        self.send_bytes(&frame).await
    }

    /// Sends raw bytes on the control stream: for tests of what the server does with
    /// malformed input.
    pub async fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), LinkError> {
        self.send
            .write_all(bytes)
            .await
            .map_err(|e| LinkError::ConnectionLost(e.to_string()))
    }

    /// The next message from the server. [`LinkError::Closed`] once it has finished the
    /// stream.
    pub async fn recv(&mut self) -> Result<ServerMessage, LinkError> {
        loop {
            if let Some(message) = self.decoder.next_message().map_err(LinkError::Protocol)? {
                return Ok(message);
            }
            match self.recv.read(&mut self.buffer).await {
                Ok(Some(n)) => self.decoder.push(&self.buffer[..n]),
                Ok(None) => {
                    self.decoder.finish().map_err(LinkError::Protocol)?;
                    return Err(LinkError::Closed);
                }
                Err(e) => return Err(LinkError::ConnectionLost(e.to_string())),
            }
        }
    }

    /// The QUIC connection, for tests.
    pub fn quic(&self) -> &quinn::Connection {
        &self.quic
    }

    /// Ends the session and waits briefly for the server to learn of it.
    pub async fn close(mut self) {
        let _ = self.send.finish();
        self.quic.close(0_u32.into(), b"bye");
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            self.endpoint.wait_idle(),
        )
        .await;
    }
}

/// Opens a connection and authenticates: [`Connection::open`] and
/// [`Connection::handshake`].
pub async fn connect(info: &ConnectInfo, build: &str) -> Result<(Connection, Welcome), LinkError> {
    let mut connection = Connection::open(info).await?;
    let welcome = connection.handshake(&info.token, build).await?;
    Ok((connection, welcome))
}
