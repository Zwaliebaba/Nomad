//! The Universe database: `SQLite` in write-ahead-log mode (N2 plan §4.3; doc 20 §6; D-048,
//! D-088).
//!
//! | Table | Holds |
//! |---|---|
//! | `meta` | One row: layout format, Universe ID and name, creating build, seed, season start and length, creation parameters, rules hash, journal floor |
//! | `snapshots` | One row per snapshot: tick, build, snapshot format, state hash, bytes |
//! | `journal` | One row per input, and every 600 ticks a state-hash checkpoint (D-109): tick, seq, kind, payload, build. Kept for 48 h (D-110) |
//!
//! Snapshots are opaque bytes plus the format number `nomad_sim::snapshot` reported when
//! they were written; this module never looks inside them. Every write is one transaction.
//! `synchronous = FULL` makes each commit durable on disk, so a journal batch that was
//! committed survives a crash or power loss (D-048: flushed at least once per second).

use std::path::{Path, PathBuf};
use std::time::Duration;

use nomad_sim::Tick;
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, params};

use crate::error::ServerError;
use crate::ids::Difficulty;
use crate::journal::{EntryKind, JournalEntry};
use crate::retention::{self, RetentionPlan};

/// The database layout this build reads and writes. Bumped whenever a table changes.
pub const SCHEMA_FORMAT: i64 = 1;

/// `SQLite` `application_id` of a Nomad Universe database: "NMAD".
const APPLICATION_ID: i64 = 0x4E4D_4144;

const SCHEMA: &str = "
CREATE TABLE meta (
    singleton       INTEGER PRIMARY KEY CHECK (singleton = 1),
    format          INTEGER NOT NULL,
    universe_id     TEXT    NOT NULL,
    name            TEXT,
    build           TEXT    NOT NULL,
    seed            INTEGER NOT NULL,
    season_start_ms INTEGER NOT NULL,
    season_weeks    INTEGER NOT NULL,
    ai_commanders   INTEGER NOT NULL,
    difficulty      TEXT    NOT NULL,
    created_at_ms   INTEGER NOT NULL,
    rules_hash      INTEGER,
    journal_floor   INTEGER NOT NULL
) STRICT;
CREATE TABLE snapshots (
    tick       INTEGER PRIMARY KEY,
    build      TEXT    NOT NULL,
    format     INTEGER NOT NULL,
    state_hash INTEGER NOT NULL,
    bytes      BLOB    NOT NULL
) STRICT;
CREATE TABLE journal (
    tick    INTEGER NOT NULL,
    seq     INTEGER NOT NULL,
    kind    TEXT    NOT NULL,
    payload BLOB    NOT NULL,
    build   TEXT    NOT NULL,
    PRIMARY KEY (tick, seq)
) STRICT, WITHOUT ROWID;
";

/// The `meta` row: what the Universe is and how it was created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// The Universe ID in text form (D-107).
    pub universe_id: String,
    /// Optional display name (D-107).
    pub name: Option<String>,
    /// The build that created the Universe.
    pub build: String,
    /// The season seed.
    pub seed: u64,
    /// Universe time of tick 0, UTC ms since the Unix epoch.
    pub season_start_unix_ms: i64,
    /// Season length in weeks (D-020, D-106).
    pub season_weeks: u32,
    /// AI commanders created with the Universe.
    pub ai_commanders: u32,
    /// Their difficulty (D-079).
    pub difficulty: Difficulty,
    /// Wall-clock time of creation, UTC ms.
    pub created_at_unix_ms: i64,
    /// Hash of the rules copied into the Universe (D-104). `None` until the rules file
    /// exists (S3).
    pub rules_hash: Option<u64>,
    /// The journal is complete after this tick, so recovery may start from any snapshot at
    /// or after it (doc 11 §8). Moved forward by retention.
    pub journal_floor: Tick,
}

/// A snapshot's row without its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotInfo {
    /// The tick it was taken at, after that tick was computed.
    pub tick: Tick,
    /// The build that wrote it.
    pub build: String,
    /// Its snapshot format (`nomad_sim::snapshot::FORMAT` of that build).
    pub format: u32,
    /// The state hash at that tick.
    pub state_hash: u64,
    /// Its size in bytes.
    pub size: u64,
}

/// A snapshot with its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRecord {
    /// The tick it was taken at.
    pub tick: Tick,
    /// The build that wrote it.
    pub build: String,
    /// Its snapshot format.
    pub format: u32,
    /// The state hash at that tick (`nomad_sim::snapshot::state_hash`).
    pub state_hash: u64,
    /// The encoded state.
    pub bytes: Vec<u8>,
}

/// Size of the journal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JournalStats {
    /// Number of entries.
    pub entries: u64,
    /// Total payload size in bytes.
    pub payload_bytes: u64,
    /// Tick of the oldest entry.
    pub first_tick: Option<Tick>,
    /// Tick of the newest entry.
    pub last_tick: Option<Tick>,
    /// State-hash checkpoints among the entries (D-109).
    pub checkpoints: u64,
    /// Tick of the newest checkpoint.
    pub last_checkpoint: Option<Tick>,
}

/// An open Universe database.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Creates the database at `path` with its `meta` row and first snapshot, in one
    /// transaction. Fails if a file already exists there.
    pub fn create(
        path: &Path,
        meta: &Meta,
        snapshot: &SnapshotRecord,
    ) -> Result<Self, ServerError> {
        if path.exists() {
            return Err(ServerError::InvalidParameter(format!(
                "{} already exists",
                path.display()
            )));
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let conn =
            Connection::open_with_flags(path, flags).map_err(|e| db_error(path, "create", e))?;
        let mut store = Self::configure(conn, path)?;
        let tx = begin(&mut store.conn, path, "create")?;
        let fail = |context| move |e| db_error(path, context, e);
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(fail("set application id"))?;
        tx.execute_batch(SCHEMA).map_err(fail("create tables"))?;
        tx.execute(
            "INSERT INTO meta (singleton, format, universe_id, name, build, seed,
                 season_start_ms, season_weeks, ai_commanders, difficulty, created_at_ms,
                 rules_hash, journal_floor)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                SCHEMA_FORMAT,
                meta.universe_id,
                meta.name,
                meta.build,
                meta.seed.cast_signed(),
                meta.season_start_unix_ms,
                meta.season_weeks,
                meta.ai_commanders,
                meta.difficulty.as_str(),
                meta.created_at_unix_ms,
                meta.rules_hash.map(u64::cast_signed),
                tick_to_sql(meta.journal_floor)?,
            ],
        )
        .map_err(fail("write meta"))?;
        insert_snapshot(&tx, path, snapshot)?;
        tx.commit().map_err(fail("commit"))?;
        Ok(store)
    }

    /// Opens an existing Universe database and checks that it is one this build can read.
    pub fn open(path: &Path) -> Result<Self, ServerError> {
        if !path.is_file() {
            return Err(ServerError::UniverseNotFound {
                path: path.to_owned(),
            });
        }
        // Never create: a missing file is a missing Universe, not a new one.
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e| db_error(path, "open", e))?;
        let store = Self::configure(conn, path)?;
        store.check_layout()?;
        Ok(store)
    }

    fn configure(conn: Connection, path: &Path) -> Result<Self, ServerError> {
        let fail = |context| move |e| db_error(path, context, e);
        // The first statement that reads the file: a file that is not SQLite fails here.
        let mode: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .map_err(fail("enable write-ahead log"))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(ServerError::CorruptDatabase {
                path: path.to_owned(),
                detail: format!("cannot use write-ahead-log mode (journal mode is {mode})"),
            });
        }
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(fail("set synchronous"))?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(fail("set busy timeout"))?;
        Ok(Self {
            conn,
            path: path.to_owned(),
        })
    }

    fn check_layout(&self) -> Result<(), ServerError> {
        let not_a_universe = |detail: String| ServerError::NotAUniverse {
            path: self.path.clone(),
            detail,
        };
        let application_id: i64 = self
            .conn
            .query_row("PRAGMA application_id", [], |row| row.get(0))
            .map_err(|e| self.error("read application id", e))?;
        if application_id != APPLICATION_ID {
            return Err(not_a_universe(format!(
                "application id {application_id:#x}, expected {APPLICATION_ID:#x}"
            )));
        }
        let check: String = self
            .conn
            .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
            .map_err(|e| self.error("check integrity", e))?;
        if check != "ok" {
            return Err(ServerError::CorruptDatabase {
                path: self.path.clone(),
                detail: check,
            });
        }
        for table in ["meta", "snapshots", "journal"] {
            let exists: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
                    [table],
                    |row| row.get(0),
                )
                .map_err(|e| self.error("read schema", e))?;
            if !exists {
                return Err(not_a_universe(format!("table {table} is missing")));
            }
        }
        let format: Option<i64> = self
            .conn
            .query_row("SELECT format FROM meta WHERE singleton = 1", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| self.error("read meta", e))?;
        match format {
            None => Err(not_a_universe("the meta row is missing".to_owned())),
            Some(SCHEMA_FORMAT) => Ok(()),
            Some(found) => Err(ServerError::UnsupportedDatabaseFormat {
                path: self.path.clone(),
                found,
                expected: SCHEMA_FORMAT,
            }),
        }
    }

    /// The `meta` row.
    pub fn meta(&self) -> Result<Meta, ServerError> {
        type Raw = (
            String,
            Option<String>,
            String,
            i64,
            i64,
            i64,
            i64,
            String,
            i64,
            Option<i64>,
            i64,
        );
        let raw: Raw = self
            .conn
            .query_row(
                "SELECT universe_id, name, build, seed, season_start_ms, season_weeks,
                        ai_commanders, difficulty, created_at_ms, rules_hash, journal_floor
                 FROM meta WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                    ))
                },
            )
            .map_err(|e| self.error("read meta", e))?;
        let (id, name, build, seed, start, weeks, ai, difficulty, created, rules, floor) = raw;
        let corrupt = |what: &str| self.corrupt(format!("meta.{what} is out of range"));
        Ok(Meta {
            universe_id: id,
            name,
            build,
            seed: seed.cast_unsigned(),
            season_start_unix_ms: start,
            season_weeks: u32::try_from(weeks).map_err(|_| corrupt("season_weeks"))?,
            ai_commanders: u32::try_from(ai).map_err(|_| corrupt("ai_commanders"))?,
            difficulty: difficulty.parse().map_err(|_| corrupt("difficulty"))?,
            created_at_unix_ms: created,
            rules_hash: rules.map(i64::cast_unsigned),
            journal_floor: tick_from_sql(floor).ok_or_else(|| corrupt("journal_floor"))?,
        })
    }

    /// Every snapshot, oldest first, without its bytes.
    pub fn snapshots(&self) -> Result<Vec<SnapshotInfo>, ServerError> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT tick, build, format, state_hash, length(bytes)
                 FROM snapshots ORDER BY tick",
            )
            .map_err(|e| self.error("list snapshots", e))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| self.error("list snapshots", e))?;
        let mut snapshots = Vec::new();
        for row in rows {
            let (tick, build, format, hash, size) =
                row.map_err(|e| self.error("list snapshots", e))?;
            snapshots.push(SnapshotInfo {
                tick: tick_from_sql(tick)
                    .ok_or_else(|| self.corrupt(format!("snapshot tick {tick}")))?,
                build,
                format: u32::try_from(format)
                    .map_err(|_| self.corrupt(format!("snapshot format {format}")))?,
                state_hash: hash.cast_unsigned(),
                size: u64::try_from(size).unwrap_or(0),
            });
        }
        Ok(snapshots)
    }

    /// The snapshot at `tick`, with its bytes.
    pub fn snapshot(&self, tick: Tick) -> Result<SnapshotRecord, ServerError> {
        let row: Option<(String, i64, i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT build, format, state_hash, bytes FROM snapshots WHERE tick = ?1",
                [tick_to_sql(tick)?],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|e| self.error("read snapshot", e))?;
        let (build, format, hash, bytes) = row.ok_or(ServerError::NoSnapshot)?;
        Ok(SnapshotRecord {
            tick,
            build,
            format: u32::try_from(format)
                .map_err(|_| self.corrupt(format!("snapshot format {format}")))?,
            state_hash: hash.cast_unsigned(),
            bytes,
        })
    }

    /// Journal entries after `tick`, in `(tick, seq)` order.
    pub fn journal_after(&self, tick: Tick) -> Result<Vec<JournalEntry>, ServerError> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT tick, seq, kind, payload, build FROM journal
                 WHERE tick > ?1 ORDER BY tick, seq",
            )
            .map_err(|e| self.error("read journal", e))?;
        let rows = statement
            .query_map([tick_to_sql(tick)?], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| self.error("read journal", e))?;
        let mut entries = Vec::new();
        for row in rows {
            let (tick, seq, kind, payload, build) =
                row.map_err(|e| self.error("read journal", e))?;
            let tick =
                tick_from_sql(tick).ok_or_else(|| self.corrupt(format!("journal tick {tick}")))?;
            let seq = u32::try_from(seq).map_err(|_| ServerError::CorruptJournal {
                tick,
                seq: 0,
                detail: format!("sequence number {seq}"),
            })?;
            let kind = EntryKind::parse(&kind).ok_or_else(|| ServerError::CorruptJournal {
                tick,
                seq,
                detail: format!("unknown kind {kind:?}"),
            })?;
            entries.push(JournalEntry {
                tick,
                seq,
                kind,
                payload,
                build,
            });
        }
        Ok(entries)
    }

    /// The size of the journal.
    pub fn journal_stats(&self) -> Result<JournalStats, ServerError> {
        let (entries, bytes, first, last): (i64, i64, Option<i64>, Option<i64>) = self
            .conn
            .query_row(
                "SELECT count(*), coalesce(sum(length(payload)), 0), min(tick), max(tick)
                 FROM journal",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| self.error("measure journal", e))?;
        let (checkpoints, last_checkpoint): (i64, Option<i64>) = self
            .conn
            .query_row(
                "SELECT count(*), max(tick) FROM journal WHERE kind = ?1",
                [EntryKind::StateHash.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| self.error("count checkpoints", e))?;
        Ok(JournalStats {
            entries: u64::try_from(entries).unwrap_or(0),
            payload_bytes: u64::try_from(bytes).unwrap_or(0),
            first_tick: first.and_then(tick_from_sql),
            last_tick: last.and_then(tick_from_sql),
            checkpoints: u64::try_from(checkpoints).unwrap_or(0),
            last_checkpoint: last_checkpoint.and_then(tick_from_sql),
        })
    }

    /// Appends a batch of journal entries in one transaction.
    pub fn append_journal(&mut self, entries: &[JournalEntry]) -> Result<(), ServerError> {
        if entries.is_empty() {
            return Ok(());
        }
        let path = self.path.as_path();
        let tx = begin(&mut self.conn, path, "append journal")?;
        {
            let mut statement = tx
                .prepare_cached(
                    "INSERT INTO journal (tick, seq, kind, payload, build)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| db_error(path, "append journal", e))?;
            for entry in entries {
                statement
                    .execute(params![
                        tick_to_sql(entry.tick)?,
                        entry.seq,
                        entry.kind.as_str(),
                        entry.payload,
                        entry.build,
                    ])
                    .map_err(|e| db_error(path, "append journal", e))?;
            }
        }
        tx.commit().map_err(|e| db_error(path, "commit journal", e))
    }

    /// Writes a snapshot and applies retention (doc 11 §8), in one transaction. Returns
    /// what retention kept and deleted.
    pub fn write_snapshot(
        &mut self,
        snapshot: &SnapshotRecord,
    ) -> Result<RetentionPlan, ServerError> {
        let path = self.path.as_path();
        let fail = |context| move |e| db_error(path, context, e);
        let tx = begin(&mut self.conn, path, "write snapshot")?;
        insert_snapshot(&tx, path, snapshot)?;
        let ticks = {
            let mut statement = tx
                .prepare_cached("SELECT tick FROM snapshots ORDER BY tick")
                .map_err(fail("list snapshots"))?;
            let rows = statement
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(fail("list snapshots"))?;
            let mut ticks = Vec::new();
            for row in rows {
                let raw = row.map_err(fail("list snapshots"))?;
                ticks.push(
                    tick_from_sql(raw).ok_or_else(|| ServerError::CorruptDatabase {
                        path: path.to_owned(),
                        detail: format!("snapshot tick {raw}"),
                    })?,
                );
            }
            ticks
        };
        let plan = retention::plan(&ticks);
        {
            let mut delete = tx
                .prepare_cached("DELETE FROM snapshots WHERE tick = ?1")
                .map_err(fail("delete snapshot"))?;
            for &tick in &plan.delete {
                delete
                    .execute([tick_to_sql(tick)?])
                    .map_err(fail("delete snapshot"))?;
            }
        }
        let floor = tick_to_sql(plan.journal_floor)?;
        tx.execute("DELETE FROM journal WHERE tick <= ?1", [floor])
            .map_err(fail("compact journal"))?;
        tx.execute(
            "UPDATE meta SET journal_floor = ?1 WHERE singleton = 1",
            [floor],
        )
        .map_err(fail("update journal floor"))?;
        tx.commit().map_err(fail("commit snapshot"))?;
        Ok(plan)
    }

    fn error(&self, context: &'static str, e: rusqlite::Error) -> ServerError {
        db_error(&self.path, context, e)
    }

    fn corrupt(&self, detail: String) -> ServerError {
        ServerError::CorruptDatabase {
            path: self.path.clone(),
            detail,
        }
    }
}

fn begin<'c>(
    conn: &'c mut Connection,
    path: &Path,
    context: &'static str,
) -> Result<Transaction<'c>, ServerError> {
    conn.transaction().map_err(|e| db_error(path, context, e))
}

fn insert_snapshot(
    tx: &Transaction<'_>,
    path: &Path,
    snapshot: &SnapshotRecord,
) -> Result<(), ServerError> {
    // The final snapshot of a clean shutdown can fall on a periodic snapshot's tick; it is
    // the same state, so it replaces the row.
    tx.execute(
        "INSERT OR REPLACE INTO snapshots (tick, build, format, state_hash, bytes)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            tick_to_sql(snapshot.tick)?,
            snapshot.build,
            snapshot.format,
            snapshot.state_hash.cast_signed(),
            snapshot.bytes,
        ],
    )
    .map(drop)
    .map_err(|e| db_error(path, "write snapshot", e))
}

/// Maps `SQLite` errors; a damaged file becomes [`ServerError::CorruptDatabase`].
fn db_error(path: &Path, context: &'static str, source: rusqlite::Error) -> ServerError {
    match source.sqlite_error_code() {
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => {
            ServerError::CorruptDatabase {
                path: path.to_owned(),
                detail: format!("{context}: {source}"),
            }
        }
        _ => ServerError::Database {
            path: path.to_owned(),
            context,
            source,
        },
    }
}

fn tick_to_sql(tick: Tick) -> Result<i64, ServerError> {
    i64::try_from(tick.get())
        .map_err(|_| ServerError::Encode(format!("tick {} does not fit SQLite", tick.get())))
}

fn tick_from_sql(raw: i64) -> Option<Tick> {
    u64::try_from(raw).ok().map(Tick::new)
}
