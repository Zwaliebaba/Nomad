//! `assets/layout/hud_layout.json`, format `voxel-rts/hud-layout@1` (phase-1 docs 03 and 04).
//!
//! M3 reads only what the hover tag needs: the font family names and the `ELEV +n` tag style
//! (`markers.elevation_tag`). Panel rectangles, colours and the other marker styles follow with
//! the HUD in M6; serde ignores them for now. Every declared field is required.
//!
//! The tag's text is a template from this file, not a literal in render code (phase-1 README,
//! ground rule 3; `CODING_STANDARDS.md` §9): `{tier}` stands for the module's `elevation_tier`.

use std::path::Path;

use serde::Deserialize;

use super::{ConfigError, Rgb, check_format};

/// The one format version this build reads.
pub const HUD_LAYOUT_FORMAT: &str = "voxel-rts/hud-layout@1";
/// Placeholder for the elevation tier in [`ElevationTagStyle::text`].
pub const TIER_PLACEHOLDER: &str = "{tier}";
/// Longest tag template accepted, in bytes.
const MAX_TAG_TEXT_LEN: usize = 64;

/// The parsed file. `format` is checked separately before this is read.
#[derive(Clone, Debug, Deserialize)]
pub struct HudLayout {
    pub font_families: FontFamilies,
    pub markers: Markers,
}

/// Font family names as the font files declare them. The fonts are OFL and are not bundled
/// (phase-1 README, ground rule 4): they are loaded from `<asset root>/fonts/` when present.
#[derive(Clone, Debug, Deserialize)]
pub struct FontFamilies {
    /// `JetBrains Mono`: body text, values, tags (doc 04 "Common style").
    pub mono: String,
    /// Rajdhani: headers and button labels (doc 04 "Common style").
    pub display: String,
}

/// Which of the two HUD fonts a style uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontRole {
    Mono,
    Display,
}

impl FontFamilies {
    pub fn family(&self, role: FontRole) -> &str {
        match role {
            FontRole::Mono => &self.mono,
            FontRole::Display => &self.display,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Markers {
    pub elevation_tag: ElevationTagStyle,
}

/// The `ELEV +n` tag shown on hover (doc 01 §7: "10 px bold, only on hover or selection").
#[derive(Clone, Debug, Deserialize)]
pub struct ElevationTagStyle {
    /// Template of the tag text; [`TIER_PLACEHOLDER`] is replaced by the tier.
    pub text: String,
    pub font: FontRole,
    /// Font size in 1080p pixels; scales with the window, not with the zoom.
    pub size_px: f32,
    /// CSS-style weight, 100–900 (700 is bold).
    pub weight: u16,
    pub color: Rgb,
    /// Distance from the lowest screen point of the footprint down to the text baseline, in
    /// 1080p pixels.
    pub baseline_below_footprint_px: f32,
}

impl ElevationTagStyle {
    /// The tag text for a tier.
    pub fn text_for(&self, tier: u32) -> String {
        self.text.replace(TIER_PLACEHOLDER, &tier.to_string())
    }
}

/// A rejected field and the reason.
type Rejection = (&'static str, String);

impl HudLayout {
    /// Parses and validates the JSON text of a HUD layout. `path` is used in error messages.
    pub fn from_json(text: &str, path: &Path) -> Result<Self, ConfigError> {
        check_format(text, path, HUD_LAYOUT_FORMAT)?;
        let layout: Self = serde_json::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        layout
            .validate()
            .map_err(|(field, reason)| ConfigError::Invalid {
                path: path.to_path_buf(),
                field,
                reason,
            })?;
        Ok(layout)
    }

    fn validate(&self) -> Result<(), Rejection> {
        let families = &self.font_families;
        if families.mono.trim().is_empty() || families.display.trim().is_empty() {
            return Err(("font_families", "family names must not be empty".into()));
        }
        let tag = &self.markers.elevation_tag;
        if tag.text.is_empty() || tag.text.len() > MAX_TAG_TEXT_LEN {
            return Err((
                "markers.elevation_tag.text",
                format!("must be 1 to {MAX_TAG_TEXT_LEN} bytes long"),
            ));
        }
        if !tag.text.contains(TIER_PLACEHOLDER) {
            return Err((
                "markers.elevation_tag.text",
                format!("must contain {TIER_PLACEHOLDER}"),
            ));
        }
        // cosmic-text panics on a zero font size; keep sizes sane.
        if !(tag.size_px.is_finite() && (1.0..=200.0).contains(&tag.size_px)) {
            return Err(("markers.elevation_tag.size_px", "must be 1 to 200".into()));
        }
        if !(100..=900).contains(&tag.weight) {
            return Err(("markers.elevation_tag.weight", "must be 100 to 900".into()));
        }
        if !(tag.baseline_below_footprint_px.is_finite()
            && tag.baseline_below_footprint_px.abs() <= 1000.0)
        {
            return Err((
                "markers.elevation_tag.baseline_below_footprint_px",
                "must be finite and within ±1000".into(),
            ));
        }
        Ok(())
    }
}

/// Reads and validates `hud_layout.json`.
pub fn load_hud_layout(path: &Path) -> Result<HudLayout, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    HudLayout::from_json(&text, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{hud_layout_path, phase1_fixture_root};

    fn real_text() -> String {
        std::fs::read_to_string(hud_layout_path(&phase1_fixture_root())).unwrap()
    }

    fn parse(text: &str) -> Result<HudLayout, ConfigError> {
        HudLayout::from_json(text, Path::new("test.json"))
    }

    fn edited(edit: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut value: serde_json::Value = serde_json::from_str(&real_text()).unwrap();
        edit(&mut value);
        value.to_string()
    }

    /// Doc 01 §7: `ELEV +n`, 10 px bold; doc 04: `JetBrains Mono` and Rajdhani.
    #[test]
    fn real_hud_layout_parses() {
        let layout = load_hud_layout(&hud_layout_path(&phase1_fixture_root())).unwrap();
        assert_eq!(
            layout.font_families.family(FontRole::Mono),
            "JetBrains Mono"
        );
        assert_eq!(layout.font_families.family(FontRole::Display), "Rajdhani");
        let tag = &layout.markers.elevation_tag;
        assert_eq!(tag.text_for(8), "ELEV +8");
        assert_eq!(tag.text_for(4), "ELEV +4");
        assert_eq!(tag.font, FontRole::Mono);
        assert!((tag.size_px - 10.0).abs() < 1e-6);
        assert_eq!(tag.weight, 700);
    }

    #[test]
    fn bad_layouts_are_rejected() {
        type Edit = fn(&mut serde_json::Value);
        assert!(matches!(
            parse(&edited(|v| v["format"] = "voxel-rts/hud-layout@2".into())),
            Err(ConfigError::UnsupportedFormat { .. })
        ));
        assert!(matches!(
            parse(&edited(|v| {
                v["markers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("elevation_tag");
            })),
            Err(ConfigError::Parse { .. })
        ));
        assert!(matches!(
            parse(&edited(
                |v| v["markers"]["elevation_tag"]["font"] = "serif".into()
            )),
            Err(ConfigError::Parse { .. })
        ));
        let invalid: [(&str, Edit); 5] = [
            ("no placeholder", |v| {
                v["markers"]["elevation_tag"]["text"] = "ELEV".into();
            }),
            ("zero size", |v| {
                v["markers"]["elevation_tag"]["size_px"] = 0.into();
            }),
            ("weight out of range", |v| {
                v["markers"]["elevation_tag"]["weight"] = 1000.into();
            }),
            ("empty family", |v| v["font_families"]["mono"] = " ".into()),
            ("huge offset", |v| {
                v["markers"]["elevation_tag"]["baseline_below_footprint_px"] = 1e9.into();
            }),
        ];
        for (name, edit) in invalid {
            assert!(
                matches!(parse(&edited(edit)), Err(ConfigError::Invalid { .. })),
                "{name} should be rejected"
            );
        }
    }
}
