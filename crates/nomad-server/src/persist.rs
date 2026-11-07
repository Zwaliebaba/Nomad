//! The persistence thread (N2 plan §4.4; doc 20 §4; D-048).
//!
//! Owns the database connection while the server runs. The simulation thread sends it the
//! journal rows of each tick with inputs, a commit request at least once per second of
//! Universe time while rows are pending, and the encoded snapshots. Journal rows are
//! committed in batches; each snapshot is written with retention in one transaction.
//!
//! On a clean stop it commits what is pending. On an abort, the stand-in for a crash, it
//! drops the uncommitted batch, exactly as a crash would lose it.

use std::sync::mpsc::{Receiver, SyncSender};

use tracing::{debug, error, info};

use crate::clock::Clock;
use crate::error::ServerError;
use crate::journal::JournalEntry;
use crate::retention::RetentionPolicy;
use crate::store::{SnapshotRecord, Store};

/// Bound of the channel from the simulation thread. Live play sends a few messages per
/// second; while catching up, a full channel holds the simulation back to the disk's pace.
pub(crate) const CHANNEL_BOUND: usize = 256;

/// Work for the persistence thread, in the order the simulation produced it.
#[derive(Debug)]
pub(crate) enum PersistMessage {
    /// The journal rows of one tick. Pending until the next commit.
    Journal(Vec<JournalEntry>),
    /// Commit the pending rows.
    Commit,
    /// Commit the pending rows, then write this snapshot and apply retention.
    Snapshot {
        /// The snapshot.
        record: SnapshotRecord,
        /// Log it at `info` (the final snapshot) instead of `debug`.
        announce: bool,
    },
    /// Commit the pending rows, then report the outcome.
    Flush(SyncSender<Result<(), String>>),
    /// Commit the pending rows and stop.
    Stop,
    /// Stop without committing: the uncommitted rows are lost, as in a crash.
    Abort,
}

/// Runs the persistence thread until [`PersistMessage::Stop`] or [`PersistMessage::Abort`],
/// applying `retention` after every snapshot.
pub(crate) fn run(
    mut store: Store,
    inbox: &Receiver<PersistMessage>,
    clock: &dyn Clock,
    retention: RetentionPolicy,
) -> Result<(), ServerError> {
    let result = serve(&mut store, inbox, clock, retention);
    if let Err(e) = &result {
        error!(error = %e, "persistence failed; the server stops");
    }
    result
}

fn serve(
    store: &mut Store,
    inbox: &Receiver<PersistMessage>,
    clock: &dyn Clock,
    retention: RetentionPolicy,
) -> Result<(), ServerError> {
    let mut pending: Vec<JournalEntry> = Vec::new();
    // A closed channel means the simulation thread is gone without saying how; treat it as
    // an abort.
    while let Ok(message) = inbox.recv() {
        match message {
            PersistMessage::Journal(entries) => pending.extend(entries),
            PersistMessage::Commit => commit(store, &mut pending)?,
            PersistMessage::Snapshot { record, announce } => {
                commit(store, &mut pending)?;
                let started = clock.monotonic();
                let plan = store.write_snapshot(&record, retention)?;
                let elapsed_ms = format_args!(
                    "{:.1}",
                    clock.monotonic().saturating_sub(started).as_secs_f64() * 1_000.0
                );
                if announce {
                    info!(
                        tick = record.tick.get(),
                        bytes = record.bytes.len(),
                        elapsed_ms,
                        "snapshot written"
                    );
                } else {
                    debug!(
                        tick = record.tick.get(),
                        bytes = record.bytes.len(),
                        elapsed_ms,
                        kept = plan.keep.len(),
                        deleted = plan.delete.len(),
                        journal_floor = plan.journal_floor.get(),
                        "snapshot written"
                    );
                }
            }
            PersistMessage::Flush(reply) => {
                let result = commit(store, &mut pending);
                // The requester may have given up waiting; that is not an error here.
                let _ = reply.send(result.as_ref().map(drop).map_err(ToString::to_string));
                result?;
            }
            PersistMessage::Stop => return commit(store, &mut pending),
            PersistMessage::Abort => {
                if !pending.is_empty() {
                    debug!(
                        lost = pending.len(),
                        "abort: uncommitted journal rows dropped"
                    );
                }
                return Ok(());
            }
        }
    }
    Ok(())
}

fn commit(store: &mut Store, pending: &mut Vec<JournalEntry>) -> Result<(), ServerError> {
    if pending.is_empty() {
        return Ok(());
    }
    store.append_journal(pending)?;
    pending.clear();
    Ok(())
}
