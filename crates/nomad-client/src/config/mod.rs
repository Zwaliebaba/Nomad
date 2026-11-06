//! Configuration and asset loading (phase-1 doc 03).
//!
//! Every file is parsed with `serde`. A file whose `format` version this build does not know is
//! rejected with a clear error before the rest of it is read (`CODING_STANDARDS.md` §9). The voxel
//! assets (`crate::assets`) share the asset root and the error type defined here.

mod hud;
mod scene;

use std::fmt;
use std::path::{Path, PathBuf};

pub use hud::{ElevationTagStyle, FontRole, HudLayout, load_hud_layout};
pub use scene::{
    BackgroundConfig, BloomComposite, BloomConfig, ColorStop, CueLineStyle, ElevationConfig,
    GridLineStyle, MAX_NEBULAE, OpsGridConfig, SceneConfig, StarsConfig,
};

/// Environment variable that overrides the asset root.
pub const ASSETS_ENV_VAR: &str = "NOMAD_ASSETS";

/// The asset root: `$NOMAD_ASSETS` when set and non-empty, otherwise the phase-1 package in this
/// workspace (`Design/phase1-visual-slice/assets`).
pub fn asset_root() -> PathBuf {
    match std::env::var_os(ASSETS_ENV_VAR) {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => default_asset_root(),
    }
}

/// `Design/phase1-visual-slice/assets`, resolved from this crate's manifest directory at build time.
pub fn default_asset_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("Design")
        .join("phase1-visual-slice")
        .join("assets")
}

/// Location of `scene_config.json` under an asset root.
pub fn scene_config_path(asset_root: &Path) -> PathBuf {
    asset_root.join("layout").join("scene_config.json")
}

/// Location of `station_scene.json` under an asset root.
pub fn station_scene_path(asset_root: &Path) -> PathBuf {
    asset_root.join("voxels").join("station_scene.json")
}

/// Location of `hud_layout.json` under an asset root.
pub fn hud_layout_path(asset_root: &Path) -> PathBuf {
    asset_root.join("layout").join("hud_layout.json")
}

/// The folder the OFL fonts are downloaded into (phase-1 README, ground rule 4). It is
/// git-ignored: the fonts are not bundled.
pub fn fonts_dir(asset_root: &Path) -> PathBuf {
    asset_root.join("fonts")
}

/// Reads and validates `scene_config.json`.
pub fn load_scene_config(path: &Path) -> Result<SceneConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    SceneConfig::from_json(&text, path)
}

/// Rejects a file whose `format` is not `expected`, before the rest of it is parsed, so a file from
/// another version is reported as such and not as a confusing schema error.
pub fn check_format(text: &str, path: &Path, expected: &'static str) -> Result<(), ConfigError> {
    #[derive(serde::Deserialize)]
    struct FormatProbe {
        format: String,
    }
    let probe: FormatProbe = serde_json::from_str(text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    if probe.format == expected {
        Ok(())
    } else {
        Err(ConfigError::UnsupportedFormat {
            path: path.to_path_buf(),
            found: probe.format,
            expected,
        })
    }
}

/// Why a configuration or asset file was rejected.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{}: unsupported format {found:?}; this build reads {expected:?}", path.display())]
    UnsupportedFormat {
        path: PathBuf,
        found: String,
        expected: &'static str,
    },
    #[error("{}: invalid `{field}`: {reason}", path.display())]
    Invalid {
        path: PathBuf,
        field: &'static str,
        reason: String,
    },
}

/// An sRGB colour written as `"#rrggbb"` in the JSON files.
///
/// The renderer works on sRGB-encoded values directly (see `render::context`), so the channels are
/// passed to shaders as they are, only scaled to `0.0..=1.0`.
#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Channels scaled to `0.0..=1.0`, still sRGB-encoded.
    pub fn to_unit(self) -> [f32; 3] {
        [
            f32::from(self.r) / 255.0,
            f32::from(self.g) / 255.0,
            f32::from(self.b) / 255.0,
        ]
    }

    /// `[r, g, b, w]` for a `vec4<f32>` uniform field.
    pub fn to_vec4(self, w: f32) -> [f32; 4] {
        let [r, g, b] = self.to_unit();
        [r, g, b, w]
    }
}

impl fmt::Debug for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::try_from(text.as_str())
    }
}

impl TryFrom<&str> for Rgb {
    type Error = String;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let malformed = || format!("expected a colour like \"#6fd6ff\", got {text:?}");
        let hex = text.strip_prefix('#').ok_or_else(malformed)?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(malformed());
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| malformed());
        Ok(Self::new(channel(0)?, channel(2)?, channel(4)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(Rgb::try_from("#6fd6ff"), Ok(Rgb::new(0x6f, 0xd6, 0xff)));
        assert_eq!(Rgb::try_from("#8FE2FF"), Ok(Rgb::new(0x8f, 0xe2, 0xff)));
    }

    #[test]
    fn rejects_malformed_colours() {
        for text in [
            "6fd6ff", "#6fd6f", "#6fd6ffa", "#6fd6fg", "#", "", "#+fd6ff",
        ] {
            assert!(Rgb::try_from(text).is_err(), "{text:?} should be rejected");
        }
    }

    #[test]
    fn default_asset_root_holds_the_scene_files() {
        assert!(scene_config_path(&default_asset_root()).is_file());
        assert!(station_scene_path(&default_asset_root()).is_file());
        assert!(hud_layout_path(&default_asset_root()).is_file());
    }
}
