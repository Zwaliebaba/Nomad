//! Journal entries: the external inputs of each tick (Design doc 11 §6, D-048; N2 plan §4.3).
//!
//! Every input that entered a tick is one row: `(tick, seq, kind, payload, build)`. `seq`
//! numbers the inputs of one tick in the order they were applied: operator inputs first,
//! then commander inputs. The payload is the input encoded with `postcard`. Replaying the
//! rows of a tick rebuilds exactly the `TickInputs` the simulation saw.

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
}

impl EntryKind {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Commander => "commander",
        }
    }

    /// Parses a stored name.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "operator" => Some(Self::Operator),
            "commander" => Some(Self::Commander),
            _ => None,
        }
    }
}

/// One journal row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    /// The tick the input applied on.
    pub tick: Tick,
    /// Its position among the inputs of that tick.
    pub seq: u32,
    /// What it is.
    pub kind: EntryKind,
    /// The input, `postcard`-encoded.
    pub payload: Vec<u8>,
    /// The build that wrote it (D-048).
    pub build: String,
}

/// The journal rows for the inputs that applied on `tick`. Empty if there were none.
pub fn encode(
    tick: Tick,
    inputs: &TickInputs,
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
    operator
        .chain(commanders)
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

/// Rebuilds the inputs of each tick from journal rows in `(tick, seq)` order.
///
/// Fails on the first row written by a build other than `build` (D-048) and on rows that
/// do not decode or are out of order.
pub fn decode(
    entries: &[JournalEntry],
    build: &str,
) -> Result<BTreeMap<Tick, TickInputs>, ServerError> {
    let mut by_tick: BTreeMap<Tick, TickInputs> = BTreeMap::new();
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
        let inputs = by_tick.entry(entry.tick).or_default();
        match entry.kind {
            EntryKind::Operator => {
                if !inputs.commanders.is_empty() {
                    return Err(corrupt("operator input after commander inputs".to_owned()));
                }
                inputs
                    .operator
                    .push(decode_payload::<OperatorInput>(&entry.payload).map_err(corrupt)?);
            }
            EntryKind::Commander => {
                inputs
                    .commanders
                    .push(decode_payload::<CommanderInput>(&entry.payload).map_err(corrupt)?);
            }
        }
    }
    Ok(by_tick)
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
        let rows = encode(Tick::new(5), &inputs(), "b1").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].kind, EntryKind::Commander);
        assert_eq!(rows.iter().map(|r| r.seq).collect::<Vec<_>>(), [0, 1, 2]);
        let decoded = decode(&rows, "b1").unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[&Tick::new(5)], inputs());
        assert!(
            encode(Tick::new(5), &TickInputs::default(), "b1")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn foreign_build_is_refused() {
        let rows = encode(Tick::new(5), &inputs(), "b1").unwrap();
        assert!(matches!(
            decode(&rows, "b2"),
            Err(ServerError::JournalFromOtherBuild { journal_build, .. }) if journal_build == "b1"
        ));
    }

    #[test]
    fn damage_is_an_error_not_a_panic() {
        let good = encode(Tick::new(5), &inputs(), "b1").unwrap();
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
