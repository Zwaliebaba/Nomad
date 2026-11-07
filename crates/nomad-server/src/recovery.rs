//! Recovery: load the latest snapshot and replay the journal after it (Design doc 11 §8;
//! doc 20 §7; D-048, D-049; N2 plan §4.5 step 1).
//!
//! Recovery may start from any snapshot at or after the journal floor, because the journal
//! after it is complete. The journal is kept for 48 h (D-110), so a damaged latest snapshot
//! falls back to the one before it, and further, to any snapshot of the last 48 h. A
//! snapshot in a format this build cannot read is never skipped: that is a version problem,
//! not damage, and it stops startup (D-099). Journal entries written by another build stop
//! startup too (D-048).
//!
//! After recovery the season read model is re-synced with the recovered state, since it is a
//! projection (doc 11 §8, D-196; [`sync_read_model`]). Replay stops at the season's final
//! tick: no tick follows it (D-116).
//!
//! Replay checks the state-hash checkpoints in the journal (doc 11 §5, D-109). A mismatch
//! means replay diverged from the run that wrote the journal: a determinism bug. It is
//! logged as an error with the tick and both hashes and reported in [`ReplayCheck`], and
//! recovery carries on, because the replayed state is still a valid Universe.

use nomad_sim::snapshot::{self, decode, state_hash};
use nomad_sim::{Tick, TickInputs, UniverseState, step};
use tracing::{error, info, warn};

use crate::error::ServerError;
use crate::journal;
use crate::store::read_model::ReadModelSync;
use crate::store::{Meta, SnapshotInfo, Store};

/// The state after recovery, and how it was reached.
#[derive(Debug)]
pub(crate) struct Recovered {
    /// The Universe at the last journaled tick, or at the snapshot if there is no journal
    /// after it.
    pub state: UniverseState,
    /// The snapshot recovery started from.
    pub snapshot_tick: Tick,
    /// Journal input entries replayed, not counting checkpoints.
    pub replayed_entries: usize,
    /// What the checkpoints in the replayed journal showed (D-109).
    pub check: ReplayCheck,
}

/// The outcome of the state-hash checkpoints met during replay (doc 11 §5, D-109).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayCheck {
    /// Checkpoints compared.
    pub checked: usize,
    /// Checkpoints whose hash differed from the replayed state, in tick order.
    pub mismatches: Vec<CheckpointMismatch>,
}

/// A checkpoint the replay did not reproduce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointMismatch {
    /// The tick of the checkpoint.
    pub tick: Tick,
    /// The hash in the journal, from the run that wrote it.
    pub journaled: u64,
    /// The hash of the replayed state at that tick.
    pub replayed: u64,
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
    let replay = journal::decode(&entries, build)?;
    let mut rejected = 0_u64;
    let mut check = ReplayCheck::default();
    if let Some(last) = replay.last_tick() {
        let empty = TickInputs::default();
        while state.tick() < last {
            if let Some(result) = state.season_result() {
                // No tick follows the final one (D-116): the server never journals past it.
                warn!(
                    tick = state.tick().get(),
                    last_journaled = last.get(),
                    reason = ?result.reason,
                    "the journal goes past the season's final tick; the rest is not replayed"
                );
                break;
            }
            let next = state.tick().next();
            let outputs = step(&mut state, replay.inputs.get(&next).unwrap_or(&empty));
            rejected += u64::from(outputs.inputs_rejected);
            if let Some(&journaled) = replay.checkpoints.get(&next) {
                compare(&state, journaled, &mut check)?;
            }
        }
    }
    info!(
        snapshot_tick = snapshot_tick.get(),
        replayed_entries = replay.input_entries,
        checkpoints = check.checked,
        mismatches = check.mismatches.len(),
        tick = state.tick().get(),
        rejected_inputs = rejected,
        "Universe loaded"
    );
    Ok(Recovered {
        state,
        snapshot_tick,
        replayed_entries: replay.input_entries,
        check,
    })
}

/// Re-syncs the season read model with the recovered `state` (D-196): it is a projection,
/// so it follows the state (doc 11 §8).
pub(crate) fn sync_read_model(
    store: &mut Store,
    state: &UniverseState,
) -> Result<ReadModelSync, ServerError> {
    let sync = store.sync_read_model(state)?;
    if !sync.unchanged() {
        info!(
            tick = state.tick().get(),
            dropped_rows = sync.dropped_rows,
            appended = sync.appended,
            result_written = sync.result_written,
            "season read model re-synced with the recovered state"
        );
    }
    Ok(sync)
}

/// Compares the replayed `state` with a checkpoint's `journaled` hash (D-109).
fn compare(
    state: &UniverseState,
    journaled: u64,
    check: &mut ReplayCheck,
) -> Result<(), ServerError> {
    let replayed = state_hash(state)?;
    check.checked += 1;
    if replayed != journaled {
        error!(
            tick = state.tick().get(),
            journaled = format_args!("{journaled:016x}"),
            replayed = format_args!("{replayed:016x}"),
            "replay diverged from the journal: the state hash differs at a checkpoint \
             (a determinism bug, D-109); recovery continues with the replayed state"
        );
        check.mismatches.push(CheckpointMismatch {
            tick: state.tick(),
            journaled,
            replayed,
        });
    }
    Ok(())
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
    if season.seed != meta.seed
        || season.season_start_unix_ms != meta.season_start_unix_ms
        || season.final_tick != meta.final_tick
    {
        return Err("it belongs to another season".to_owned());
    }
    Ok(state)
}
