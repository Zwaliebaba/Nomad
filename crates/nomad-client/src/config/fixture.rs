//! `assets/fixtures/siege_kr114.json`, format `voxel-rts/fixture@1` (phase-1 doc 03): the phase-1
//! data feed that stands in for the live view-model (D-028).
//!
//! Only what the world layer reads so far is declared: the shield pylons' strengths
//! (`selection.pylons`), which colour the shield sectors (doc 06 M4). The HUD (M6) reads the
//! rest; serde ignores it until then.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::{ConfigError, check_format};

/// The one format version this build reads.
pub const FIXTURE_FORMAT: &str = "voxel-rts/fixture@1";

/// The parsed fixture.
#[derive(Clone, Debug, Deserialize)]
pub struct Fixture {
    pub selection: Selection,
}

/// The selected object's panel data (doc 04).
#[derive(Clone, Debug, Deserialize)]
pub struct Selection {
    /// Strength of each shield pylon in `[0, 1]`, keyed by its letter (`N`, `E`, `S`, `W`).
    pub pylons: BTreeMap<String, f32>,
}

impl Fixture {
    /// Parses and validates the JSON text of a fixture. `path` is used in error messages.
    pub fn from_json(text: &str, path: &Path) -> Result<Self, ConfigError> {
        check_format(text, path, FIXTURE_FORMAT)?;
        let fixture: Self = serde_json::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        if let Some((pylon, _)) = fixture
            .selection
            .pylons
            .iter()
            .find(|(_, s)| !(0.0..=1.0).contains(*s))
        {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                field: "selection.pylons",
                reason: format!("strength of pylon {pylon} must be in [0, 1]"),
            });
        }
        Ok(fixture)
    }

    /// The strength of the pylon with the station id `pylon_id` (`pylon_e` → the fixture's `E`).
    pub fn pylon_strength(&self, pylon_id: &str) -> Option<f32> {
        let letter = pylon_id.strip_prefix("pylon_")?.to_ascii_uppercase();
        self.selection.pylons.get(&letter).copied()
    }
}

/// Reads and validates a fixture.
pub fn load_fixture(path: &Path) -> Result<Fixture, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Fixture::from_json(&text, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{fixture_path, phase1_fixture_root};

    #[test]
    fn real_fixture_has_the_pylon_strengths() {
        let fixture = load_fixture(&fixture_path(&phase1_fixture_root())).unwrap();
        assert_eq!(fixture.pylon_strength("pylon_n"), Some(1.0));
        assert_eq!(fixture.pylon_strength("pylon_e"), Some(0.38));
        assert_eq!(fixture.pylon_strength("pylon_s"), Some(0.88));
        assert_eq!(fixture.pylon_strength("pylon_w"), Some(0.92));
        assert_eq!(fixture.pylon_strength("pylon_x"), None);
        assert_eq!(fixture.pylon_strength("core"), None);
    }

    #[test]
    fn rejects_other_formats_and_bad_strengths() {
        let path = Path::new("test.json");
        let other = r#"{"format": "voxel-rts/fixture@2", "selection": {"pylons": {}}}"#;
        assert!(matches!(
            Fixture::from_json(other, path),
            Err(ConfigError::UnsupportedFormat { .. })
        ));
        let bad = r#"{"format": "voxel-rts/fixture@1", "selection": {"pylons": {"E": 1.5}}}"#;
        assert!(matches!(
            Fixture::from_json(bad, path),
            Err(ConfigError::Invalid { .. })
        ));
        let missing = r#"{"format": "voxel-rts/fixture@1"}"#;
        assert!(matches!(
            Fixture::from_json(missing, path),
            Err(ConfigError::Parse { .. })
        ));
    }
}
