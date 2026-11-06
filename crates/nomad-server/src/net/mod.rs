//! The network endpoint: QUIC on UDP, sessions and the connection file (N3 plan; Design
//! doc 20 §4, §5, §9; D-085, D-087, D-120, D-122 to D-124, D-150, D-151).
//!
//! - **Bind** (D-150): 127.0.0.1 on [`DEFAULT_PORT`] unless `--bind`/`NOMAD_BIND` or
//!   `--port`/`NOMAD_PORT` say otherwise. Port 0 lets the operating system choose, as tests
//!   do. The Docker image binds 0.0.0.0 and is published on the host's loopback only, which
//!   keeps D-085's "localhost only".
//! - **Threads** (§4.1): the network thread runs a current-thread `tokio` runtime with the
//!   `quinn` endpoint, the [`hub`] and one task per connection. A bridge thread moves the
//!   simulation's events into the runtime. The simulation thread never waits on either.
//! - **Identity** (D-151): the certificate, key and token in `identity.json`, created once.
//! - **Connection file** (D-120, D-150): `server.json` with the address a local client
//!   connects to, the fingerprint and the token, written once the socket is bound and
//!   removed on a clean shutdown.

pub mod hub;
pub mod identity;
mod session;
mod tls;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nomad_protocol::{CONNECTION_FILE, CONNECTION_FILE_FORMAT, ConnectionFile, PROTOCOL_VERSION};
use nomad_sim::Tick;
use nomad_types::{SeasonClock, UniverseId};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::create::HUMAN_COMMANDER;
use crate::error::ServerError;
use crate::ids::universe_id_text;
use crate::server::{ServerAccess, ServerEvent};
use hub::{Hub, ToHub};
use identity::{Identity, write_private};

/// The default UDP port (D-150). In the registered range (1024–49151) and below every
/// operating system's ephemeral range (Linux from 32768, Windows from 49152), so no
/// outgoing connection is ever handed it; not assigned in the IANA registry as far as
/// checked (N3 plan §4.2).
pub const DEFAULT_PORT: u16 = 31_790;

/// Environment variable for the bind address (D-150).
pub const BIND_ENV: &str = "NOMAD_BIND";

/// Environment variable for the port (D-150).
pub const PORT_ENV: &str = "NOMAD_PORT";

/// The ALPN protocol name (D-087). No version: `Hello` checks that.
pub const ALPN: &[u8] = b"nomad";

/// Timeouts and limits of the endpoint (N3 plan §4.6). Tests shorten them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Time from the connection to `Hello`.
    pub hello_timeout: Duration,
    /// Time from `Hello` to `Auth`.
    pub auth_timeout: Duration,
    /// QUIC idle timeout: a silent connection ends after this.
    pub idle_timeout: Duration,
    /// Keep-alive interval, well inside the idle timeout.
    pub keep_alive: Duration,
    /// Commands per second a session may send, sustained.
    pub commands_per_second: u32,
    /// Commands a session may send in a burst.
    pub command_burst: u32,
    /// Connections at once, counting ones still in the handshake.
    pub max_connections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            hello_timeout: Duration::from_secs(5),
            auth_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(30),
            keep_alive: Duration::from_secs(5),
            commands_per_second: 20,
            command_burst: 60,
            max_connections: 4,
        }
    }
}

/// Where the endpoint listens, and its limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetConfig {
    /// The address and UDP port to bind (D-150).
    pub bind: SocketAddr,
    /// Timeouts and limits.
    pub limits: Limits,
}

impl NetConfig {
    /// Listening on `bind` with the default limits.
    pub fn new(bind: SocketAddr) -> Self {
        Self {
            bind,
            limits: Limits::default(),
        }
    }

    /// 127.0.0.1 on `port` (0: chosen by the operating system).
    pub fn localhost(port: u16) -> Self {
        Self::new(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
    }
}

impl Default for NetConfig {
    /// 127.0.0.1 on [`DEFAULT_PORT`] (D-085, D-150).
    fn default() -> Self {
        Self::localhost(DEFAULT_PORT)
    }
}

/// The address a client on this machine connects to when the server binds `bound`: the
/// address itself, or loopback when it binds to all addresses.
pub fn local_connect_address(bound: SocketAddr) -> SocketAddr {
    let ip = match bound.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, bound.port())
}

/// What the network thread needs to know about the Universe.
pub(crate) struct UniverseInfo {
    pub dir: PathBuf,
    pub universe: UniverseId,
    pub clock: SeasonClock,
    pub tick: Tick,
    pub build: String,
}

/// The running endpoint.
#[derive(Debug)]
pub(crate) struct Network {
    /// The bound address.
    pub local_addr: SocketAddr,
    connection_file: PathBuf,
    thread: Option<JoinHandle<()>>,
}

/// Binds the endpoint's UDP socket (D-150). Done before the server's threads start, so a busy
/// port stops the start cleanly.
pub(crate) fn bind(config: &NetConfig) -> Result<UdpSocket, ServerError> {
    UdpSocket::bind(config.bind).map_err(|source| ServerError::Bind {
        addr: config.bind,
        source,
    })
}

impl Network {
    /// Writes the connection file and starts the network thread on the bound `socket`. It
    /// serves until the simulation publishes [`ServerEvent::Stopped`].
    pub fn start(
        socket: UdpSocket,
        config: &NetConfig,
        identity: Identity,
        universe: &UniverseInfo,
        server: ServerAccess,
        events: Receiver<ServerEvent>,
    ) -> Result<Self, ServerError> {
        let local_addr = socket
            .local_addr()
            .map_err(|e| ServerError::io("cannot read the bound address", e))?;
        if !local_addr.ip().is_loopback() {
            warn!(
                %local_addr,
                "listening on a non-loopback address: other machines can reach the server \
                 unless a firewall or a loopback-only port mapping stops them (D-085, D-150)"
            );
        }
        let server_config =
            tls::server_config(&identity, &config.limits).map_err(ServerError::Network)?;
        let connection_file = universe.dir.join(CONNECTION_FILE);
        write_connection_file(&connection_file, local_addr, &identity, universe)?;
        identity::warn_if_exposed(&universe.dir);

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        thread::Builder::new()
            .name("nomad-net-bridge".to_owned())
            .spawn(move || bridge(&events, &event_tx))
            .map_err(|e| ServerError::io("cannot start the network bridge thread", e))?;
        let hub = Hub::new(server, universe.universe, universe.clock, universe.tick);
        let limits = config.limits;
        let identity = Arc::new(identity);
        let thread = thread::Builder::new()
            .name("nomad-net".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(e) => {
                        warn!(error = %e, "cannot start the network runtime");
                        return;
                    }
                };
                runtime.block_on(serve(
                    socket,
                    server_config,
                    hub,
                    event_rx,
                    identity,
                    limits,
                ));
            })
            .map_err(|e| ServerError::io("cannot start the network thread", e))?;
        info!(%local_addr, "listening (QUIC)");
        Ok(Self {
            local_addr,
            connection_file,
            thread: Some(thread),
        })
    }

    /// Waits for the network thread; with `clean`, removes the connection file.
    pub fn join(mut self, clean: bool) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if clean {
            let _ = std::fs::remove_file(&self.connection_file);
        }
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Moves the simulation's events into the runtime, until the simulation stops.
fn bridge(events: &Receiver<ServerEvent>, hub: &mpsc::UnboundedSender<ServerEvent>) {
    for event in events {
        let stopped = event == ServerEvent::Stopped;
        if hub.send(event).is_err() || stopped {
            return;
        }
    }
}

/// The endpoint, the hub and the sessions.
async fn serve(
    socket: UdpSocket,
    server_config: quinn::ServerConfig,
    mut hub: Hub<ServerAccess>,
    mut events: mpsc::UnboundedReceiver<ServerEvent>,
    identity: Arc<Identity>,
    limits: Limits,
) {
    let endpoint = match quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(server_config),
        socket,
        Arc::new(quinn::TokioRuntime),
    ) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            warn!(error = %e, "cannot start the QUIC endpoint");
            // Keep draining events so the bridge never backs up, until the server stops.
            while let Some(event) = events.recv().await {
                if event == ServerEvent::Stopped {
                    return;
                }
            }
            return;
        }
    };
    let (hub_tx, mut hub_rx) = mpsc::unbounded_channel::<ToHub>();
    let connections = Arc::new(AtomicUsize::new(0));
    let mut next_session: u64 = 1;
    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Some(event) => hub.on_event(event),
                    None => hub.on_event(ServerEvent::Stopped),
                }
                if hub.stopped() {
                    break;
                }
            }
            Some(request) = hub_rx.recv() => hub.on_request(request),
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { break };
                if connections.load(Ordering::Relaxed) >= limits.max_connections {
                    debug!("connection refused: too many connections");
                    incoming.refuse();
                    continue;
                }
                let session = next_session;
                next_session += 1;
                let guard = ConnectionGuard::new(&connections);
                let context = session::SessionContext {
                    session,
                    commander: HUMAN_COMMANDER,
                    identity: Arc::clone(&identity),
                    limits,
                    hub: hub_tx.clone(),
                };
                tokio::spawn(async move {
                    let _guard = guard;
                    match incoming.await {
                        Ok(connection) => session::run(connection, context).await,
                        Err(e) => debug!(error = %e, "connection attempt failed"),
                    }
                });
            }
        }
    }
    // Let the sessions deliver their Bye, then close.
    let drain = async {
        while connections.load(Ordering::Relaxed) > 0 {
            tokio::select! {
                Some(request) = hub_rx.recv() => hub.on_request(request),
                () = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(2), drain).await;
    endpoint.close(0_u32.into(), b"server stopped");
    let _ = tokio::time::timeout(Duration::from_secs(1), endpoint.wait_idle()).await;
    info!("network stopped");
}

/// Counts open connections for the connection limit.
struct ConnectionGuard(Arc<AtomicUsize>);

impl ConnectionGuard {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::Relaxed);
        Self(Arc::clone(count))
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn write_connection_file(
    path: &Path,
    bound: SocketAddr,
    identity: &Identity,
    universe: &UniverseInfo,
) -> Result<(), ServerError> {
    let file = ConnectionFile {
        format: CONNECTION_FILE_FORMAT,
        address: local_connect_address(bound),
        fingerprint: identity.fingerprint().to_hex(),
        token: identity.token().as_str().to_owned(),
        universe: universe_id_text(universe.universe),
        build: universe.build.clone(),
        protocol: PROTOCOL_VERSION,
        pid: std::process::id(),
    };
    let text = serde_json::to_string_pretty(&file)
        .map_err(|e| ServerError::Encode(format!("connection file: {e}")))?;
    write_private(path, text.as_bytes(), true).map(|_| ())
}
