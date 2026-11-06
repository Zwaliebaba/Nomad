//! Server errors.
//!
//! Nothing read from disk may panic the server (`CODING_STANDARDS.md` §5): a corrupt
//! database, a missing snapshot, an unknown format or a foreign journal each become one of
//! these errors, with a message an operator can act on.

use std::io;
use std::path::PathBuf;

use nomad_sim::Tick;
use nomad_sim::galaxy::GenerationError;
use nomad_sim::snapshot::SnapshotError;
use thiserror::Error;

/// Why the server could not create, open, run or inspect a Universe.
#[derive(Debug, Error)]
pub enum ServerError {
    /// No data directory was given and the platform has none.
    #[error(
        "no data directory: set --data-dir or NOMAD_DATA (the platform reports no per-user data directory)"
    )]
    NoDataDir,
    /// A create or run parameter is out of range.
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
    /// No Universe with that identity exists in the data directory.
    #[error("no Universe at {}", .path.display())]
    UniverseNotFound {
        /// Where it was looked for.
        path: PathBuf,
    },
    /// Several Universes exist and none was chosen.
    #[error("{count} Universes in {}; choose one with --universe <id>", .data_dir.display())]
    UniverseAmbiguous {
        /// How many there are.
        count: usize,
        /// The data directory.
        data_dir: PathBuf,
    },
    /// Another server holds the Universe's lock (N2 plan §4.2).
    #[error(
        "the Universe in {} is already in use by another server (lock {} is held)",
        .dir.display(),
        .lock.display()
    )]
    Locked {
        /// The Universe directory.
        dir: PathBuf,
        /// The lock file.
        lock: PathBuf,
    },
    /// A file-system operation failed.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The underlying error.
        source: io::Error,
    },
    /// `SQLite` reported an error.
    #[error("database {}: {context}: {source}", .path.display())]
    Database {
        /// The database file.
        path: PathBuf,
        /// What was being done.
        context: &'static str,
        /// The underlying error.
        source: rusqlite::Error,
    },
    /// The database file is damaged or not a database at all.
    #[error("database {} is corrupt: {detail}", .path.display())]
    CorruptDatabase {
        /// The database file.
        path: PathBuf,
        /// What is wrong.
        detail: String,
    },
    /// The file is a database, but not a Nomad Universe.
    #[error("database {} is not a Nomad Universe: {detail}", .path.display())]
    NotAUniverse {
        /// The database file.
        path: PathBuf,
        /// What is missing.
        detail: String,
    },
    /// The database layout is from a different server version.
    #[error(
        "database {} has layout format {found}; this build reads format {expected}",
        .path.display()
    )]
    UnsupportedDatabaseFormat {
        /// The database file.
        path: PathBuf,
        /// Format found.
        found: i64,
        /// Format this build reads.
        expected: i64,
    },
    /// The Universe has no snapshot to start from.
    #[error("the Universe has no snapshot; it cannot be loaded")]
    NoSnapshot,
    /// The newest snapshot was written in a snapshot format this build cannot read. Formats
    /// are not migrated before β (D-099 item 9).
    #[error(
        "the snapshot at tick {} has format {found}; this build reads format {expected} \
         (snapshots are not migrated before β, D-099)",
        .tick.get()
    )]
    UnsupportedSnapshotFormat {
        /// Its tick.
        tick: Tick,
        /// Format found.
        found: u32,
        /// Format this build reads.
        expected: u32,
    },
    /// Every snapshot that recovery could start from is damaged.
    #[error("no usable snapshot: {}", .problems.join("; "))]
    NoUsableSnapshot {
        /// What was wrong with each candidate, newest first.
        problems: Vec<String>,
    },
    /// The journal after the snapshot was written by another build, so replaying it would
    /// not be deterministic (D-048, D-049).
    #[error(
        "the journal entry at tick {} was written by build {journal_build}, but this is build \
         {this_build}; a journal is replayed only by the build that wrote it (D-048). Start \
         build {journal_build} and stop it cleanly with Ctrl+C so that it writes a final \
         snapshot, then start this build",
        .tick.get()
    )]
    JournalFromOtherBuild {
        /// Tick of the first foreign entry.
        tick: Tick,
        /// The build that wrote it.
        journal_build: String,
        /// This build.
        this_build: String,
    },
    /// A journal entry cannot be decoded.
    #[error("journal entry {seq} at tick {} is corrupt: {detail}", .tick.get())]
    CorruptJournal {
        /// Its tick.
        tick: Tick,
        /// Its sequence number within the tick.
        seq: u32,
        /// What is wrong.
        detail: String,
    },
    /// Encoding a snapshot or journal entry failed.
    #[error("encoding failed: {0}")]
    Encode(String),
    /// The snapshot could not be encoded.
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    /// The stage-1 generator failed.
    #[error("Universe generation failed: {0}")]
    Generation(#[from] GenerationError),
    /// The persistence thread stopped, so nothing more can be saved.
    #[error("the persistence thread stopped")]
    PersistenceStopped,
    /// The persistence thread failed to write.
    #[error("persistence failed: {0}")]
    Persistence(String),
    /// The server has stopped.
    #[error("the server has stopped")]
    Stopped,
    /// A server thread panicked.
    #[error("the {0} thread panicked")]
    ThreadPanicked(&'static str),
    /// The network endpoint could not bind its UDP socket (N3; D-150).
    #[error(
        "cannot listen on UDP {addr}: {source} (another program may use the port; choose \
         another with --port or NOMAD_PORT)"
    )]
    Bind {
        /// The address and port.
        addr: std::net::SocketAddr,
        /// The underlying error.
        source: io::Error,
    },
    /// The server's identity (certificate, key and token, D-151) is damaged or could not be
    /// made.
    #[error("server identity {}: {detail}", .path.display())]
    Identity {
        /// The identity file.
        path: PathBuf,
        /// What is wrong.
        detail: String,
    },
    /// The network endpoint could not be set up (TLS or QUIC configuration).
    #[error("network endpoint: {0}")]
    Network(String),
}

impl ServerError {
    /// An I/O error with context.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
