//! `status`: what is on disk for a Universe, without running it (N2 plan §4.1, §6 task 8).
//!
//! Reads `meta`, the snapshots and the size of the journal, and checks whether a server
//! holds the lock. The database is only read; a running server is not disturbed.

use std::fmt;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use nomad_protocol::{CONNECTION_FILE, ConnectionFile};
use nomad_sim::{SeasonClock, Tick};

use crate::clock::Clock;
use crate::error::ServerError;
use crate::paths::{DATABASE_FILE, UniverseLock};
use crate::server::season_end_tick;
use crate::short_build;
use crate::sim::wall_tick;
use crate::store::{JournalStats, Meta, SnapshotInfo, Store};
use crate::timefmt::{bytes_text, ticks_text, utc_text};

/// Snapshots listed one by one; older ones are counted.
const LISTED_SNAPSHOTS: usize = 8;

/// The state of one Universe on disk.
#[derive(Clone, Debug)]
pub struct UniverseReport {
    /// Its directory.
    pub dir: PathBuf,
    /// Its `meta` row.
    pub meta: Meta,
    /// Whether a server holds its lock.
    pub running: bool,
    /// Where the running server listens, from its connection file (N3 plan §4.9).
    pub endpoint: Option<SocketAddr>,
    /// Its snapshots, oldest first.
    pub snapshots: Vec<SnapshotInfo>,
    /// The size of its journal.
    pub journal: JournalStats,
    /// Size of the database file.
    pub database_bytes: u64,
    /// Size of the write-ahead log.
    pub wal_bytes: u64,
    /// The tick the wall clock maps to now.
    pub wall_tick: Tick,
    /// The build reading it.
    pub this_build: String,
}

impl UniverseReport {
    /// The newest tick saved on disk: the latest snapshot or journal entry. The Universe was
    /// at least this far when it last ran; ticks without inputs leave no trace until the
    /// next snapshot.
    pub fn last_saved_tick(&self) -> Tick {
        let snapshot = self.snapshots.last().map_or(Tick::ZERO, |s| s.tick);
        self.journal.last_tick.map_or(snapshot, |j| j.max(snapshot))
    }

    /// The season's final tick (D-020): the Universe never advances past it (D-116).
    pub fn season_end(&self) -> Tick {
        season_end_tick(self.meta.season_weeks)
    }

    /// Whether the season has ended: the saved Universe has reached its final tick.
    pub fn season_over(&self) -> bool {
        self.last_saved_tick() >= self.season_end()
    }
}

/// Reads the state of the Universe in `dir`.
pub fn inspect(dir: &Path, clock: &dyn Clock, build: &str) -> Result<UniverseReport, ServerError> {
    let db_path = dir.join(DATABASE_FILE);
    let store = Store::open(&db_path)?;
    let meta = store.meta()?;
    let snapshots = store.snapshots()?;
    let journal = store.journal_stats()?;
    drop(store);
    let running = UniverseLock::is_held(dir)?;
    // A connection file without the lock is stale, left by a crash (N3 plan §4.2).
    let endpoint = running
        .then(|| fs::read_to_string(dir.join(CONNECTION_FILE)).ok())
        .flatten()
        .and_then(|text| serde_json::from_str::<ConnectionFile>(&text).ok())
        .map(|file| file.address);
    let size = |path: PathBuf| fs::metadata(path).map_or(0, |m| m.len());
    let mut wal = db_path.clone().into_os_string();
    wal.push("-wal");
    Ok(UniverseReport {
        dir: dir.to_owned(),
        running,
        endpoint,
        wall_tick: wall_tick(SeasonClock::new(meta.season_start_unix_ms), clock.utc_ms()),
        meta,
        snapshots,
        journal,
        database_bytes: size(db_path),
        wal_bytes: size(PathBuf::from(wal)),
        this_build: build.to_owned(),
    })
}

impl fmt::Display for UniverseReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_meta(f)?;
        self.fmt_progress(f)?;
        self.fmt_storage(f)
    }
}

impl UniverseReport {
    fn fmt_meta(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let meta = &self.meta;
        let clock = SeasonClock::new(meta.season_start_unix_ms);
        let season_end = clock.universe_time_ms(self.season_end());
        match &meta.name {
            Some(name) => writeln!(f, "Universe     {} \"{name}\"", meta.universe_id)?,
            None => writeln!(f, "Universe     {}", meta.universe_id)?,
        }
        writeln!(f, "Directory    {}", self.dir.display())?;
        match (self.running, self.endpoint) {
            (true, Some(endpoint)) => writeln!(f, "Server       running on {endpoint} (UDP)")?,
            (true, None) => writeln!(f, "Server       running (the lock is held)")?,
            (false, _) => writeln!(f, "Server       not running")?,
        }
        writeln!(
            f,
            "Created      {} by build {} (this build: {})",
            utc_text(meta.created_at_unix_ms),
            short_build(&meta.build),
            short_build(&self.this_build)
        )?;
        writeln!(f, "Seed         {}", meta.seed)?;
        writeln!(
            f,
            "Season       {} weeks, {} to {}{}",
            meta.season_weeks,
            utc_text(meta.season_start_unix_ms),
            utc_text(season_end),
            if self.season_over() {
                " (ended: the Universe no longer advances, D-116)"
            } else {
                ""
            }
        )?;
        writeln!(
            f,
            "Commanders   1 human, {} AI ({})",
            meta.ai_commanders, meta.difficulty
        )?;
        match meta.rules_hash {
            Some(hash) => writeln!(f, "Rules hash   {hash:016x}")?,
            None => writeln!(f, "Rules hash   none (no rules file yet, D-104)")?,
        }
        Ok(())
    }

    fn fmt_progress(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let clock = SeasonClock::new(self.meta.season_start_unix_ms);
        let last = self.last_saved_tick();
        writeln!(
            f,
            "Last tick    {} ({}, {} into the season)",
            last.get(),
            utc_text(clock.universe_time_ms(last)),
            ticks_text(last.since(Tick::ZERO))
        )?;
        // A run catches up to the wall clock, but never past the season end (D-116).
        let target = self.wall_tick.min(self.season_end());
        let behind = target.since(last);
        if self.season_over() {
            writeln!(
                f,
                "Wall clock   tick {}; the season ended at tick {}",
                self.wall_tick.get(),
                self.season_end().get()
            )?;
        } else if self.running {
            writeln!(f, "Wall clock   tick {}", self.wall_tick.get())?;
        } else if behind.get() > 0 {
            writeln!(
                f,
                "Wall clock   tick {}: the next run catches up {}{}",
                self.wall_tick.get(),
                ticks_text(behind),
                if target < self.wall_tick {
                    ", to the season end"
                } else {
                    ""
                }
            )?;
        } else {
            writeln!(f, "Wall clock   tick {} (up to date)", self.wall_tick.get())?;
        }
        Ok(())
    }

    fn fmt_storage(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let clock = SeasonClock::new(self.meta.season_start_unix_ms);
        let total: u64 = self.snapshots.iter().map(|s| s.size).sum();
        writeln!(
            f,
            "Snapshots    {}, {} in total",
            self.snapshots.len(),
            bytes_text(total)
        )?;
        let hidden = self.snapshots.len().saturating_sub(LISTED_SNAPSHOTS);
        for snapshot in self.snapshots.iter().rev().take(LISTED_SNAPSHOTS) {
            writeln!(
                f,
                "  tick {:>10}  {}  build {}  format {}  {}",
                snapshot.tick.get(),
                utc_text(clock.universe_time_ms(snapshot.tick)),
                short_build(&snapshot.build),
                snapshot.format,
                bytes_text(snapshot.size)
            )?;
        }
        if hidden > 0 {
            writeln!(f, "  … and {hidden} older")?;
        }

        let journal = &self.journal;
        match (journal.first_tick, journal.last_tick) {
            (Some(first), Some(last)) => writeln!(
                f,
                "Journal      {} {}, {}, ticks {}–{}; complete after tick {}",
                journal.entries,
                if journal.entries == 1 {
                    "entry"
                } else {
                    "entries"
                },
                bytes_text(journal.payload_bytes),
                first.get(),
                last.get(),
                self.meta.journal_floor.get()
            )?,
            _ => writeln!(
                f,
                "Journal      empty; complete after tick {}",
                self.meta.journal_floor.get()
            )?,
        }
        match journal.last_checkpoint {
            Some(last) => writeln!(
                f,
                "Checkpoints  {} state hashes in the journal, the newest at tick {} (D-109)",
                journal.checkpoints,
                last.get()
            )?,
            None => writeln!(f, "Checkpoints  none in the journal (D-109)")?,
        }
        write!(
            f,
            "Database     {}, write-ahead log {}",
            bytes_text(self.database_bytes),
            bytes_text(self.wal_bytes)
        )
    }
}

#[cfg(test)]
mod tests {
    use nomad_sim::Ticks;

    use super::*;
    use crate::ids::Difficulty;

    fn report(last_snapshot: Tick, wall: Tick) -> UniverseReport {
        UniverseReport {
            dir: PathBuf::from("universes/abcdefgh"),
            meta: Meta {
                universe_id: "abcdefgh".to_owned(),
                name: None,
                build: "test-build".to_owned(),
                seed: 1,
                season_start_unix_ms: 0,
                season_weeks: 8,
                ai_commanders: 3,
                difficulty: Difficulty::Normal,
                created_at_unix_ms: 0,
                rules_hash: None,
                journal_floor: Tick::ZERO,
            },
            running: false,
            endpoint: None,
            snapshots: vec![SnapshotInfo {
                tick: last_snapshot,
                build: "test-build".to_owned(),
                format: 1,
                state_hash: 0,
                size: 10,
            }],
            journal: JournalStats::default(),
            database_bytes: 0,
            wal_bytes: 0,
            wall_tick: wall,
            this_build: "test-build".to_owned(),
        }
    }

    #[test]
    fn an_ended_season_is_reported() {
        let end = season_end_tick(8);
        let day = Ticks::from_days(1).get();
        let after = end.after(Ticks::from_days(3));

        let ended = report(end, after);
        assert!(ended.season_over());
        let text = ended.to_string();
        assert!(
            text.contains("(ended: the Universe no longer advances, D-116)"),
            "{text}"
        );
        assert!(
            text.contains(&format!("the season ended at tick {}", end.get())),
            "{text}"
        );
        assert!(!text.contains("catches up"), "{text}");

        // A day before the end, with the wall clock past it: the next run catches up only to
        // the season end.
        let behind = report(Tick::new(end.get() - day), after);
        assert!(!behind.season_over());
        let text = behind.to_string();
        assert!(
            text.contains("the next run catches up 1 d, to the season end"),
            "{text}"
        );
        assert!(text.contains("Checkpoints  none in the journal"), "{text}");
    }
}
