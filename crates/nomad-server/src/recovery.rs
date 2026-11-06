//! Recovery: load the latest snapshot and replay the journal after it (Design doc 11 §8;
//! doc 20 §7; D-048, D-049; N2 plan §4.5 step 1).
//!
//! Recovery may start from any snapshot at or after the journal floor, because the journal
//! after it is complete. Retention keeps two such snapshots, so a damaged latest snapshot
//! falls back to the one before it. A snapshot in a format this build cannot read is never
//! skipped: that is a version problem, not damage, and it stops startup (D-099). Journal
//! entries written by another build stop startup too (D-048).

use nomad_sim::snapshot::{self, decode, state_hash};
use nomad_sim::{Tick, TickInputs, UniverseState, step};
use tracing::{info, warn};

use crate::error::ServerError;
use crate::journal;
use crate::store::{Meta, SnapshotInfo, Store};

/// The state after recovery, and how it was reached.
#[derive(Debug)]
pub(crate) struct Recovered {
    /// The Universe at the last journaled tick, or at the snapshot if there is no journal
    /// after it.
    pub state: UniverseState,
    /// The snapshot recovery started from.
    pub snapshot_tick: Tick,
    /// Journal entries replayed.
    pub replayed_entries: usize,
}

/// Loads and replays the Universe in `store` as build `build`.
pub(crate) fn recover(store: &Store, meta: &Meta, build: &str) -> Result<Recovered, ServerError> {
    let snapshots = store.snapshots()?;
    if snapshots.is_empty() {
        return Err(ServerError::NoSnapshot);
    }
    let candidates: Vec<&SnapshotInfo> = snapshots
        .iter()
        .rev()
        .filter(|info| info.tick >= meta.journal_floor)
        .collect();
    if candidates.is_empty() {
        return Err(ServerError::NoUsableSnapshot {
            problems: vec![format!(
                "no snapshot at or after the journal floor (tick {})",
                meta.journal_floor.get()
            )],
        });
    }

    let mut problems = Vec::new();
    let mut base = None;
    for info in candidates {
        if info.format != snapshot::FORMAT {
            return Err(ServerError::UnsupportedSnapshotFormat {
                tick: info.tick,
                found: info.format,
                expected: snapshot::FORMAT,
            });
        }
        match load(store, meta, info) {
            Ok(state) => {
                base = Some(state);
                break;
            }
            Err(problem) => {
                warn!(tick = info.tick.get(), %problem, "snapshot is damaged; trying an older one");
                problems.push(format!("tick {}: {problem}", info.tick.get()));
            }
        }
    }
    let Some(mut state) = base else {
        return Err(ServerError::NoUsableSnapshot { problems });
    };
    let snapshot_tick = state.tick();

    let entries = store.journal_after(snapshot_tick)?;
    let by_tick = journal::decode(&entries, build)?;
    let mut rejected = 0_u64;
    if let Some(&last) = by_tick.keys().next_back() {
        let empty = TickInputs::default();
        while state.tick() < last {
            let next = state.tick().next();
            let outputs = step(&mut state, by_tick.get(&next).unwrap_or(&empty));
            rejected += u64::from(outputs.inputs_rejected);
        }
    }
    info!(
        snapshot_tick = snapshot_tick.get(),
        replayed_entries = entries.len(),
        tick = state.tick().get(),
        rejected_inputs = rejected,
        "Universe loaded"
    );
    Ok(Recovered {
        state,
        snapshot_tick,
        replayed_entries: entries.len(),
    })
}

/// Decodes and checks one snapshot. The error text says what is wrong with it.
fn load(store: &Store, meta: &Meta, info: &SnapshotInfo) -> Result<UniverseState, String> {
    let record = store.snapshot(info.tick).map_err(|e| e.to_string())?;
    let state = decode(&record.bytes).map_err(|e| e.to_string())?;
    if state.tick() != info.tick {
        return Err(format!("it holds tick {}", state.tick().get()));
    }
    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    if hash != record.state_hash {
        return Err(format!(
            "state hash {hash:016x}, recorded {:016x}",
            record.state_hash
        ));
    }
    let season = state.season();
    if season.seed != meta.seed || season.season_start_unix_ms != meta.season_start_unix_ms {
        return Err("it belongs to another season".to_owned());
    }
    Ok(state)
}
