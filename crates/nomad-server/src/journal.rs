//! Journal entries: the external inputs of each tick (Design doc 11 §6, D-048; N2 plan §4.3),
//! and state-hash checkpoints (doc 11 §5, D-109).
//!
//! Every input that entered a tick is one row: `(tick, seq, kind, payload, build)`. `seq`
//! numbers the rows of one tick in the order they were applied: operator inputs first,
//! then commander inputs. The payload is the input encoded with `postcard`. Replaying the
//! rows of a tick rebuilds exactly the `TickInputs` the simulation saw.
//!
//! Every [`crate::CHECKPOINT_INTERVAL`] ticks the tick's rows end with a checkpoint: the
//! state hash after that tick (`nomad_sim::snapshot::state_hash`, `postcard`-encoded).
//! Checkpoints are checks, not inputs: replay compares them and never passes them to the
//! simulation (D-109). A journal without checkpoints, as older Universes have, replays the
//! same way.

use std::collections::BTreeMap;

use nomad_sim::{CommanderInput, OperatorInput, Tick, TickInputs};

use crate::error::ServerError;

/// What a journal row holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// An [`OperatorInput`], including the catch-up markers.
    Operator,
    /// A [`CommanderInput`].
    Commander,
    /// A state-hash checkpoint, the last row of its tick (D-109).
    StateHash,
}

impl EntryKind {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Commander => "commander",
            Self::StateHash => "state_hash",
        }
    }

    /// Parses a stored name.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "operator" => Some(Self::Operator),
            "commander" => Some(Self::Commander),
            "state_hash" => Some(Self::StateHash),
            _ => None,
        }
    }
}

/// One journal row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    /// The tick the input applied on, or the checkpoint was taken after.
    pub tick: Tick,
    /// Its position among the inputs of that tick.
    pub seq: u32,
    /// What it is.
    pub kind: EntryKind,
    /// The input or the state hash, `postcard`-encoded.
    pub payload: Vec<u8>,
    /// The build that wrote it (D-048).
    pub build: String,
}

/// The journal rows for the inputs that applied on `tick`, followed by the checkpoint
/// `state_hash` if there is one (D-109). Empty if there is neither.
pub fn encode(
    tick: Tick,
    inputs: &TickInputs,
    state_hash: Option<u64>,
    build: &str,
) -> Result<Vec<JournalEntry>, ServerError> {
    let operator = inputs
        .operator
        .iter()
        .map(|input| (EntryKind::Operator, postcard::to_stdvec(input)));
    let commanders = inputs
        .commanders
        .iter()
        .map(|input| (EntryKind::Commander, postcard::to_stdvec(input)));
    let checkpoint = state_hash.map(|hash| (EntryKind::StateHash, postcard::to_stdvec(&hash)));
    operator
        .chain(commanders)
        .chain(checkpoint)
        .enumerate()
        .map(|(seq, (kind, payload))| {
            Ok(JournalEntry {
                tick,
                seq: u32::try_from(seq)
                    .map_err(|_| ServerError::Encode("too many inputs in one tick".to_owned()))?,
                kind,
                payload: payload.map_err(|e| ServerError::Encode(e.to_string()))?,
                build: build.to_owned(),
            })
        })
        .collect()
}

/// What a run of journal rows holds, ready for replay.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Replay {
    /// The inputs of each tick that had any.
    pub inputs: BTreeMap<Tick, TickInputs>,
    /// The checkpoints: the state hash after each tick that has one (D-109).
    pub checkpoints: BTreeMap<Tick, u64>,
    /// Input rows, not counting checkpoints.
    pub input_entries: usize,
}

impl Replay {
    /// The last tick the rows cover: the server computed at least this far.
    pub fn last_tick(&self) -> Option<Tick> {
        let inputs = self.inputs.keys().next_back();
        let checkpoints = self.checkpoints.keys().next_back();
        inputs.max(checkpoints).copied()
    }
}

/// Rebuilds the inputs and checkpoints of each tick from journal rows in `(tick, seq)`
/// order.
///
/// Fails on the first row written by a build other than `build` (D-048) and on rows that
/// do not decode, are out of order, or follow a checkpoint within its tick.
pub fn decode(entries: &[JournalEntry], build: &str) -> Result<Replay, ServerError> {
    let mut replay = Replay::default();
    let mut previous: Option<(Tick, u32)> = None;
    for entry in entries {
        let corrupt = |detail: String| ServerError::CorruptJournal {
            tick: entry.tick,
            seq: entry.seq,
            detail,
        };
        if entry.build != build {
            return Err(ServerError::JournalFromOtherBuild {
                tick: entry.tick,
                journal_build: entry.build.clone(),
                this_build: build.to_owned(),
            });
        }
        if previous.is_some_and(|key| key >= (entry.tick, entry.seq)) {
            return Err(corrupt("entries are out of order".to_owned()));
        }
        previous = Some((entry.tick, entry.seq));
        if replay.checkpoints.contains_key(&entry.tick) {
            return Err(corrupt("entry after the checkpoint of its tick".to_owned()));
        }
        match entry.kind {
            EntryKind::StateHash => {
                let hash = decode_payload::<u64>(&entry.payload).map_err(corrupt)?;
                replay.checkpoints.insert(entry.tick, hash);
            }
            EntryKind::Operator => {
                let inputs = replay.inputs.entry(entry.tick).or_default();
                if !inputs.commanders.is_empty() {
                    return Err(corrupt("operator input after commander inputs".to_owned()));
                }
                inputs
                    .operator
                    .push(decode_payload::<OperatorInput>(&entry.payload).map_err(corrupt)?);
                replay.input_entries += 1;
            }
            EntryKind::Commander => {
                let inputs = replay.inputs.entry(entry.tick).or_default();
                inputs
                    .commanders
                    .push(decode_payload::<CommanderInput>(&entry.payload).map_err(corrupt)?);
                replay.input_entries += 1;
            }
        }
    }
    Ok(replay)
}

fn decode_payload<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, String> {
    match postcard::take_from_bytes::<T>(payload) {
        Ok((value, [])) => Ok(value),
        Ok((_, rest)) => Err(format!("{} trailing bytes", rest.len())),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomad_sim::{CommanderId, CommanderInputKind, Ticks};

    fn inputs() -> TickInputs {
        TickInputs {
            operator: vec![
                OperatorInput::CatchUpEnd,
                OperatorInput::SpawnDiagnostics {
                    count: 3,
                    mean_interval: Ticks::new(7),
                },
            ],
            commanders: vec![CommanderInput {
                commander: CommanderId(2),
                client_seq: 9,
                kind: CommanderInputKind::Connected,
            }],
        }
    }

    #[test]
    fn round_trip_keeps_order_and_kinds() {
        let rows = encode(Tick::new(5), &inputs(), None, "b1").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].kind, EntryKind::Commander);
        assert_eq!(rows.iter().map(|r| r.seq).collect::<Vec<_>>(), [0, 1, 2]);
        let decoded = decode(&rows, "b1").unwrap();
        assert_eq!(decoded.inputs.len(), 1);
        assert_eq!(decoded.inputs[&Tick::new(5)], inputs());
        assert!(decoded.checkpoints.is_empty());
        assert_eq!(decoded.input_entries, 3);
        assert_eq!(decoded.last_tick(), Some(Tick::new(5)));
        assert!(
            encode(Tick::new(5), &TickInputs::default(), None, "b1")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_checkpoint_follows_the_inputs_of_its_tick() {
        // D-109: the state hash comes after the tick's inputs and is not an input.
        let mut rows = encode(Tick::new(5), &inputs(), Some(0xfeed), "b1").unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3].kind, EntryKind::StateHash);
        assert_eq!(rows[3].seq, 3);
        // A tick with nothing but a checkpoint.
        rows.extend(encode(Tick::new(9), &TickInputs::default(), Some(7), "b1").unwrap());
        let decoded = decode(&rows, "b1").unwrap();
        assert_eq!(decoded.inputs.len(), 1);
        assert_eq!(decoded.inputs[&Tick::new(5)], inputs());
        assert_eq!(
            decoded.checkpoints.into_iter().collect::<Vec<_>>(),
            [(Tick::new(5), 0xfeed), (Tick::new(9), 7)]
        );
        assert_eq!(decoded.input_entries, 3);

        // Replay covers the checkpoint-only tick too.
        let rows = encode(Tick::new(9), &TickInputs::default(), Some(7), "b1").unwrap();
        assert_eq!(decode(&rows, "b1").unwrap().last_tick(), Some(Tick::new(9)));
    }

    #[test]
    fn nothing_may_follow_a_checkpoint_in_its_tick() {
        let mut rows = encode(Tick::new(5), &TickInputs::default(), Some(1), "b1").unwrap();
        let mut late = encode(Tick::new(5), &inputs(), None, "b1")
            .unwrap()
            .remove(0);
        late.seq = 1;
        rows.push(late);
        assert!(matches!(
            decode(&rows, "b1"),
            Err(ServerError::CorruptJournal { seq: 1, .. })
        ));
        let mut garbage = encode(Tick::new(5), &TickInputs::default(), Some(1), "b1").unwrap();
        garbage[0].payload = vec![0xff; 11];
        assert!(matches!(
            decode(&garbage, "b1"),
            Err(ServerError::CorruptJournal { .. })
        ));
        assert_eq!(EntryKind::parse("state_hash"), Some(EntryKind::StateHash));
    }

    #[test]
    fn foreign_build_is_refused() {
        let rows = encode(Tick::new(5), &inputs(), Some(3), "b1").unwrap();
        assert!(matches!(
            decode(&rows, "b2"),
            Err(ServerError::JournalFromOtherBuild { journal_build, .. }) if journal_build == "b1"
        ));
    }

    #[test]
    fn damage_is_an_error_not_a_panic() {
        let good = encode(Tick::new(5), &inputs(), None, "b1").unwrap();
        let mut garbage = good.clone();
        garbage[0].payload = vec![0xff; 3];
        assert!(matches!(
            decode(&garbage, "b1"),
            Err(ServerError::CorruptJournal { .. })
        ));
        let mut trailing = good.clone();
        trailing[0].payload.push(0);
        assert!(matches!(
            decode(&trailing, "b1"),
            Err(ServerError::CorruptJournal { .. })
        ));
        let mut swapped = good.clone();
        swapped.swap(0, 1);
        assert!(matches!(
            decode(&swapped, "b1"),
            Err(ServerError::CorruptJournal { .. })
        ));
        let mut misplaced = good;
        misplaced[2].kind = EntryKind::Operator;
        misplaced[1].kind = EntryKind::Commander;
        assert!(matches!(
            decode(&misplaced, "b1"),
            Err(ServerError::CorruptJournal { .. })
        ));
        assert_eq!(EntryKind::parse("operator"), Some(EntryKind::Operator));
        assert_eq!(EntryKind::parse("other"), None);
    }
}
