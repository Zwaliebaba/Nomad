//! Canonical snapshots and the state hash (Design doc 11 §5, §8; D-048, D-049).
//!
//! A snapshot is `MAGIC | format (u32 LE) | postcard(UniverseState)`. Ordered maps make the
//! encoding canonical: equal states give equal bytes, so the state hash can be compared
//! across runs. Compression and storage belong to the server, not to this crate.

use thiserror::Error;

use crate::rules::RulesError;
use crate::state::UniverseState;
use crate::util::fnv1a64;

/// Identifies a Nomad snapshot.
pub const MAGIC: [u8; 8] = *b"NOMADSNP";
/// The snapshot schema version this build writes and reads. Newer builds migrate older
/// versions forward (Design doc 11 §8).
///
/// Version history. None of these was persisted outside tests, so there is nothing to migrate
/// from; until milestone β a new version simply rejects older snapshots (D-099 item 9):
///
/// - 2: the season galaxy and holdings (Design doc 10);
/// - 3: the catch-up mode (doc 11 §7, D-047; N2 plan §4.5);
/// - 4: the season's rules, copied into the Universe at creation (D-104);
/// - 5: galaxy positions in metres instead of km (D-099 item 2);
/// - 6: ships and fleets as canonical entity tables, the jump-transit queue, mother-station
///   and dry-dock fields on holdings, and the rules without the separation response time
///   (S3 plan §4.2);
/// - 7: the rules gain the combat section (S5 plan §4.1);
/// - 8: ship combat state: damage, weapons, attackers, engagement deadlines and projectiles
///   in flight (S5 plan §4.9);
/// - 9: station modules as entities (module, hull, construction and disabled tables) and the
///   economy: pools, holding economies, queues, convoys and cargo (S4); rules format 3, which
///   holds both the combat and the economy sections;
/// - 10: each queue item keeps the price fixed when it was queued, and the rules gain the
///   core kit's growth per further holding (D-189); rules format 4;
/// - 11: station combat (S5b): domes with their sectors, switches, ability cooldowns and the
///   damage ledger, turret weapon state; projectiles name their shooter and gun, and weapons
///   may aim at structures; rules format 5, which adds the defence section.
/// - 12: commander records with presence, the disconnect grace, the activity history and
///   the exposure window, and their deadlines (S6a); the rules gain the sieges section,
///   rules format 6.
/// - 13: the economy rules gain D-217's diminishing yield F; rules format 7.
/// - 14: the season's final tick in the season configuration (D-195) and the season record:
///   the cached control map, the standings with their *since*, the near-win set and the
///   result (S8; D-191); the rules gain the season section, rules format 8.
pub const FORMAT: u32 = 14;

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
    /// The rules stored in the snapshot are out of range (D-104). A snapshot comes from
    /// outside the process, so its rules are checked like a rules file.
    #[error("snapshot rules are invalid: {0}")]
    InvalidRules(#[source] RulesError),
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
    let (state, remaining): (UniverseState, _) =
        postcard::take_from_bytes(body).map_err(SnapshotError::Decode)?;
    state.rules.check().map_err(SnapshotError::InvalidRules)?;
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
    use crate::galaxy::generate_stage1;
    use crate::{CommanderId, SeasonConfig, TickInputs, step};

    fn universe() -> UniverseState {
        UniverseState::new(SeasonConfig::new(5, 1_793_923_200_000))
    }

    #[test]
    fn round_trip_is_exact() {
        let mut state = universe();
        step(&mut state, &TickInputs::default());
        let decoded = decode(&encode(&state).unwrap()).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn round_trip_with_galaxy_and_holdings_is_exact() {
        let commanders = [
            CommanderId(1),
            CommanderId(2),
            CommanderId(3),
            CommanderId(4),
        ];
        let mut state = generate_stage1(universe().season(), commanders).unwrap();
        step(&mut state, &TickInputs::default());
        let bytes = encode(&state).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded, state);
        // Canonical: re-encoding gives the same bytes.
        assert_eq!(encode(&decoded).unwrap(), bytes);
        assert_eq!(decoded.galaxy().sites().count(), 60);
        assert_eq!(decoded.holdings().count(), 4);
    }

    #[test]
    fn catch_up_mode_survives_a_round_trip() {
        let mut state = universe();
        step(
            &mut state,
            &TickInputs {
                operator: vec![crate::OperatorInput::CatchUpStart],
                commanders: vec![],
            },
        );
        let decoded = decode(&encode(&state).unwrap()).unwrap();
        assert!(decoded.catching_up());
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
        bytes[8..12].copy_from_slice(&1_u32.to_le_bytes());
        assert!(matches!(
            decode(&bytes),
            Err(SnapshotError::UnsupportedFormat { found: 1, .. })
        ));
        let mut bytes = encode(&universe()).unwrap();
        bytes.push(0);
        assert!(matches!(
            decode(&bytes),
            Err(SnapshotError::TrailingBytes(1))
        ));
    }

    #[test]
    fn a_universe_keeps_its_rules() {
        let mut rules = crate::Rules::embedded();
        rules.movement.formation_spacing_m = 200.0;
        let state = UniverseState::with_rules(universe().season(), rules.clone()).unwrap();
        let decoded = decode(&encode(&state).unwrap()).unwrap();
        assert_eq!(decoded.rules(), &rules);
        assert_ne!(decoded.rules(), universe().rules());
        assert!(
            UniverseState::with_rules(universe().season(), {
                rules.gates.jump_range_m = 0.0;
                rules
            })
            .is_err()
        );
    }

    #[test]
    fn rejects_snapshots_with_invalid_rules() {
        let mut state = universe();
        state.rules.ship_classes.clear();
        assert!(matches!(
            decode(&encode(&state).unwrap()),
            Err(SnapshotError::InvalidRules(_))
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
