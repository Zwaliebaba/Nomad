//! Canonical snapshots and the state hash (Design doc 11 §5, §8; D-048, D-049).
//!
//! A snapshot is `MAGIC | format (u32 LE) | postcard(UniverseState)`. Ordered maps make the
//! encoding canonical: equal states give equal bytes, so the state hash can be compared
//! across runs. Compression and storage belong to the server, not to this crate.

use thiserror::Error;

use crate::state::UniverseState;
use crate::util::fnv1a64;

/// Identifies a Nomad snapshot.
pub const MAGIC: [u8; 8] = *b"NOMADSNP";
/// The snapshot schema version this build writes and reads. Newer builds migrate older
/// versions forward (Design doc 11 §8).
pub const FORMAT: u32 = 1;

/// Why a snapshot could not be written or read.
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// The bytes do not start with [`MAGIC`] and a format version.
    #[error("not a Nomad snapshot")]
    BadMagic,
    /// The format version is not one this build can read.
    #[error("unsupported snapshot format {found} (this build reads {expected})")]
    UnsupportedFormat {
        /// Version found in the snapshot.
        found: u32,
        /// Version this build reads.
        expected: u32,
    },
    /// The snapshot has bytes after the encoded state.
    #[error("snapshot has {0} trailing bytes")]
    TrailingBytes(usize),
    /// Serialisation failed.
    #[error("snapshot encoding failed: {0}")]
    Encode(postcard::Error),
    /// Deserialisation failed.
    #[error("snapshot decoding failed: {0}")]
    Decode(postcard::Error),
}

/// Encodes the full Universe state.
pub fn encode(state: &UniverseState) -> Result<Vec<u8>, SnapshotError> {
    let body = postcard::to_stdvec(state).map_err(SnapshotError::Encode)?;
    let mut bytes = Vec::with_capacity(MAGIC.len() + 4 + body.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&FORMAT.to_le_bytes());
    bytes.extend_from_slice(&body);
    Ok(bytes)
}

/// Decodes a snapshot written by [`encode`]. Rejects unknown format versions.
pub fn decode(bytes: &[u8]) -> Result<UniverseState, SnapshotError> {
    let rest = bytes
        .strip_prefix(&MAGIC[..])
        .ok_or(SnapshotError::BadMagic)?;
    let (version, body) = rest
        .split_first_chunk::<4>()
        .ok_or(SnapshotError::BadMagic)?;
    let found = u32::from_le_bytes(*version);
    if found != FORMAT {
        return Err(SnapshotError::UnsupportedFormat {
            found,
            expected: FORMAT,
        });
    }
    let (state, remaining) = postcard::take_from_bytes(body).map_err(SnapshotError::Decode)?;
    if remaining.is_empty() {
        Ok(state)
    } else {
        Err(SnapshotError::TrailingBytes(remaining.len()))
    }
}

/// A 64-bit hash of the canonical snapshot encoding, for determinism checks.
pub fn state_hash(state: &UniverseState) -> Result<u64, SnapshotError> {
    encode(state).map(|bytes| fnv1a64(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SeasonConfig, TickInputs, step};

    fn universe() -> UniverseState {
        UniverseState::new(SeasonConfig {
            seed: 5,
            season_start_unix_ms: 1_793_923_200_000,
        })
    }

    #[test]
    fn round_trip_is_exact() {
        let mut state = universe();
        step(&mut state, &TickInputs::default());
        let decoded = decode(&encode(&state).unwrap()).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn rejects_foreign_and_future_snapshots() {
        assert!(matches!(decode(b"garbage"), Err(SnapshotError::BadMagic)));
        let mut bytes = encode(&universe()).unwrap();
        bytes[8..12].copy_from_slice(&(FORMAT + 1).to_le_bytes());
        assert!(matches!(
            decode(&bytes),
            Err(SnapshotError::UnsupportedFormat { .. })
        ));
        let mut bytes = encode(&universe()).unwrap();
        bytes.push(0);
        assert!(matches!(
            decode(&bytes),
            Err(SnapshotError::TrailingBytes(1))
        ));
    }

    #[test]
    fn hash_changes_with_state() {
        let mut state = universe();
        let before = state_hash(&state).unwrap();
        step(&mut state, &TickInputs::default());
        assert_ne!(state_hash(&state).unwrap(), before);
    }
}
