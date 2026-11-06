//! The headless, authoritative Nomad server shell (Design doc 20 §2, §4, §6, §7, §10;
//! doc 11 §2, §6–8; N2 plan).
//!
//! One server hosts one Universe (D-026, D-085). This crate creates it, runs it in real
//! time, saves it continuously, recovers it after a crash and catches it up after
//! downtime. It contains no game rules: those live in `nomad-sim`, which this crate drives
//! through `step`, the snapshot codec and the state hash (D-050). There is no networking
//! yet; N3 puts [`ServerHandle`] behind QUIC.
//!
//! - [`create()`] generates the MVP stage-1 Universe and writes its `meta` row and tick-0
//!   snapshot (doc 10 §4.4; D-106, D-107).
//! - [`ServerHandle::start`] takes the Universe's lock, loads the latest snapshot and
//!   replays the journal (refusing a journal from another build, D-048, and checking its
//!   state-hash checkpoints, D-109), then starts two threads (doc 20 §4):
//!   - the **simulation** thread catches up to the wall clock with journaled catch-up
//!     markers (D-047) and then paces one tick per 100 ms, publishing [`ServerEvent`]s and
//!     tick-time statistics. It journals a state-hash checkpoint every 600 ticks (D-109),
//!     and at the season's final tick it stops advancing (D-116);
//!   - the **persistence** thread commits journal batches at least once per second and
//!     writes a snapshot every 3,000 ticks, applying retention (doc 11 §8; the journal is
//!     kept for 48 h, D-110).
//! - [`ServerHandle::shutdown`], or [`ShutdownTrigger::shutdown`] from a Ctrl+C handler,
//!   stops cleanly with a final snapshot. Dropping the handle stops like a crash.
//! - [`status::inspect`] reports what is on disk without running anything.
//!
//! The wall clock is read only through [`Clock`]; tests use [`FakeClock`] (D-050).
//! Storage is `SQLite` in write-ahead-log mode, one file per Universe in the per-user data
//! directory ([`DataDir`]; D-088). Snapshots are opaque bytes plus the format number of
//! `nomad_sim::snapshot`; an unknown format is refused, not migrated (D-099).

mod clock;
mod create;
mod error;
mod ids;
mod journal;
mod paths;
mod persist;
mod recovery;
mod retention;
mod server;
mod sim;
pub mod status;
mod store;
mod timefmt;

pub use clock::{Alarm, Clock, FakeClock, SystemClock};
pub use create::{
    CreateParams, Created, DEFAULT_SEASON_WEEKS, HUMAN_COMMANDER, NAME_MAX_CHARS, SEASON_WEEKS,
    STAGE1_AI_COMMANDERS, create,
};
pub use error::ServerError;
pub use ids::{
    Difficulty, ParseDifficultyError, ParseUniverseIdError, UNIVERSE_ID_LEN, parse_universe_id,
    random_universe_id, universe_id_text,
};
pub use paths::{DATA_DIR_ENV, DATABASE_FILE, DataDir, LOCK_FILE, UniverseLock};
pub use recovery::{CheckpointMismatch, ReplayCheck};
pub use retention::{
    HOURLY_WINDOW, JOURNAL_WINDOW, KEEP_LATEST, RetentionPlan, plan as retention_plan,
};
pub use server::{
    CHECKPOINT_INTERVAL, CatchUpProgress, Input, Phase, SNAPSHOT_INTERVAL, ServerConfig,
    ServerEvent, ServerHandle, ServerStatus, ShutdownReport, ShutdownTrigger, StartReport,
    SubmitError, season_end_tick,
};
pub use store::{JournalStats, Meta, SCHEMA_FORMAT, SnapshotInfo};
pub use timefmt::{duration_text, ticks_text, utc_text};

/// This build's identity: the git commit it was built from, or `unknown` (N2 plan §4.6).
/// Snapshots and journal entries record it, and a journal is replayed only by the build
/// that wrote it (D-048, D-049).
pub const BUILD_ID: &str = env!("NOMAD_BUILD_ID");

/// The first 12 characters of a build identity, for display.
pub fn short_build(build: &str) -> &str {
    build.get(..12).unwrap_or(build)
}
