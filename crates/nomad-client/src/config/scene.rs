//! `assets/layout/scene_config.json`, format `voxel-rts/scene-config@1` (phase-1 doc 03).
//!
//! Only the fields the client uses so far are declared; serde ignores the rest (shield, pylons,
//! elevation, lights policy), so later milestones can add them without breaking older files.

use std::path::Path;

use serde::Deserialize;

use super::{ConfigError, Rgb};

/// The one format version this build reads.
pub const SCENE_CONFIG_FORMAT: &str = "voxel-rts/scene-config@1";

/// The parsed file. `format` is checked separately before this is read (see [`Self::from_json`]).
#[derive(Clone, Debug, Deserialize)]
pub struct SceneConfig {
    /// Layout reference size in pixels: `[1920, 1080]` (phase-1 README ground rule 2).
    pub reference_resolution: [u32; 2],
    pub projection: ProjectionConfig,
    pub camera: CameraConfig,
    pub ops_grid: OpsGridConfig,
    pub background: BackgroundConfig,
}

/// Projection, doc 01 §1.
#[derive(Clone, Debug, Deserialize)]
pub struct ProjectionConfig {
    /// Yaw of views 0–3: `45 + n·90` degrees.
    pub yaw_deg: Vec<f32>,
    pub pitch_deg: f32,
    /// Scale of the world z axis; the mockup's voxels are slightly squat (0.8165).
    pub z_scale: f32,
    pub pixels_per_voxel_at_zoom_1: PixelsPerVoxel,
    /// Screen position of the world origin in view 0 at zoom 1 and the reference resolution.
    pub origin_screen_view0: [f32; 2],
    /// Render true cubes (z scale 1.0) instead of `z_scale`. Optional, default `false`
    /// (doc 01 §1 asks for this behind a config flag; phase-1 doc 07 item 8).
    #[serde(default)]
    pub true_cubes: bool,
}

impl ProjectionConfig {
    /// The z scale in effect: 1.0 with `true_cubes`, otherwise `z_scale`.
    pub fn effective_z_scale(&self) -> f32 {
        if self.true_cubes { 1.0 } else { self.z_scale }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "field names mirror the JSON keys"
)]
pub struct PixelsPerVoxel {
    pub screen_x_per_world_x: f32,
    pub screen_y_per_world_x: f32,
    pub screen_y_per_world_z: f32,
}

/// Camera, doc 02.
#[derive(Clone, Debug, Deserialize)]
pub struct CameraConfig {
    /// Rotation pivot and Home position: the deck centre `(-8.5, 9.5, 0)`.
    pub pivot_world: [f32; 3],
    /// Discrete zoom steps, ascending.
    pub zoom_levels: Vec<f32>,
}

/// Ops grid, doc 01 §4.
#[derive(Clone, Debug, Deserialize)]
pub struct OpsGridConfig {
    pub plane_z: f32,
    pub minor_spacing: f32,
    pub major_spacing: f32,
    pub minor_offset_xy: [f32; 2],
    pub major_offset_xy: [f32; 2],
    pub minor: GridLineStyle,
    pub major: GridLineStyle,
    /// Lines keep `alpha_near` within this distance of the camera pivot.
    pub near_radius_voxels: f32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct GridLineStyle {
    pub color: Rgb,
    pub width_px: f32,
    pub alpha_far: f32,
    pub alpha_near: f32,
}

/// Background, doc 01 §3. Only the two end colours are machine-readable; the remaining values
/// (mid stop, nebulae, stars, sun) are prose in the file and live in `render::background`.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct BackgroundConfig {
    pub color_center: Rgb,
    pub color_edge: Rgb,
}

#[derive(Deserialize)]
struct FormatProbe {
    format: String,
}

impl SceneConfig {
    /// Parses and validates the JSON text of a scene config. `path` is used in error messages.
    pub fn from_json(text: &str, path: &Path) -> Result<Self, ConfigError> {
        let parse_error = |source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        };
        // Check the version first so a file from another version is reported as such, not as a
        // confusing schema error.
        let probe: FormatProbe = serde_json::from_str(text).map_err(parse_error)?;
        if probe.format != SCENE_CONFIG_FORMAT {
            return Err(ConfigError::UnsupportedFormat {
                path: path.to_path_buf(),
                found: probe.format,
                expected: SCENE_CONFIG_FORMAT,
            });
        }
        let config: Self = serde_json::from_str(text).map_err(parse_error)?;
        config
            .validate()
            .map_err(|(field, reason)| ConfigError::Invalid {
                path: path.to_path_buf(),
                field,
                reason,
            })?;
        Ok(config)
    }

    /// Rejects values the renderer and camera cannot work with. Configuration is external input,
    /// so bad values must produce an error, never a panic (`CODING_STANDARDS.md` §5).
    fn validate(&self) -> Result<(), (&'static str, String)> {
        let [w, h] = self.reference_resolution;
        if w == 0 || h == 0 {
            return Err(("reference_resolution", "must be positive".into()));
        }

        let projection = &self.projection;
        let yaw = &projection.yaw_deg;
        let snaps_ok = yaw.len() == 4
            && yaw
                .iter()
                .zip([0.0_f32, 90.0, 180.0, 270.0])
                .all(|(&v, step)| (v - (yaw[0] + step)).abs() < 1e-3);
        if !snaps_ok {
            return Err((
                "projection.yaw_deg",
                "expected four angles 90° apart".into(),
            ));
        }
        // The camera derives its scale from screen_x_per_world_x / cos(yaw_0).
        if (yaw[0].to_radians().cos()).abs() < 0.1 {
            return Err((
                "projection.yaw_deg",
                "view 0 must not look along a world axis".into(),
            ));
        }
        if !(positive(projection.pitch_deg) && projection.pitch_deg < 90.0) {
            return Err(("projection.pitch_deg", "must be between 0 and 90".into()));
        }
        if !positive(projection.z_scale) {
            return Err(("projection.z_scale", "must be positive".into()));
        }
        if !positive(projection.pixels_per_voxel_at_zoom_1.screen_x_per_world_x) {
            return Err((
                "projection.pixels_per_voxel_at_zoom_1.screen_x_per_world_x",
                "must be positive".into(),
            ));
        }

        let camera = &self.camera;
        // Phase 1 has a single ops-grid plane at z = 0 (doc 01 §1, §4); the camera's ground-plane
        // maths and the grid pass rely on it.
        if camera.pivot_world[2] != 0.0 {
            return Err(("camera.pivot_world", "the pivot must lie on z = 0".into()));
        }
        if self.ops_grid.plane_z != 0.0 {
            return Err(("ops_grid.plane_z", "phase 1 supports only z = 0".into()));
        }
        let zooms = &camera.zoom_levels;
        if zooms.is_empty() || zooms.iter().any(|&z| !positive(z)) {
            return Err(("camera.zoom_levels", "must be positive".into()));
        }
        if zooms.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(("camera.zoom_levels", "must be strictly ascending".into()));
        }
        // Home resets to zoom 1 (doc 02).
        if !zooms.iter().any(|&z| (z - 1.0).abs() < 1e-6) {
            return Err(("camera.zoom_levels", "must contain 1.0 for Home".into()));
        }

        let grid = &self.ops_grid;
        if !(positive(grid.minor_spacing) && positive(grid.major_spacing)) {
            return Err(("ops_grid.*_spacing", "must be positive".into()));
        }
        if grid.near_radius_voxels.is_nan() || grid.near_radius_voxels < 0.0 {
            return Err(("ops_grid.near_radius_voxels", "must not be negative".into()));
        }
        for (field, style) in [
            ("ops_grid.minor", &grid.minor),
            ("ops_grid.major", &grid.major),
        ] {
            if !positive(style.width_px) {
                return Err((field, "width_px must be positive".into()));
            }
            let alpha_ok = |a: f32| positive(a) && a <= 1.0;
            // The grid fades towards alpha_far but never to zero (doc 01 §4).
            if !alpha_ok(style.alpha_near) || !alpha_ok(style.alpha_far) {
                return Err((field, "alpha_near and alpha_far must be in (0, 1]".into()));
            }
        }
        Ok(())
    }
}

/// False for zero, negatives and NaN.
fn positive(value: f32) -> bool {
    value > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_asset_root, load_scene_config, scene_config_path};

    fn real_config() -> SceneConfig {
        load_scene_config(&scene_config_path(&default_asset_root()))
            .expect("the shipped scene_config.json must load")
    }

    fn real_text() -> String {
        std::fs::read_to_string(scene_config_path(&default_asset_root()))
            .expect("the shipped scene_config.json must be readable")
    }

    fn approx(actual: &[f32], expected: &[f32]) -> bool {
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-6)
    }

    fn parse(text: &str) -> Result<SceneConfig, ConfigError> {
        SceneConfig::from_json(text, Path::new("test.json"))
    }

    fn edited(edit: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut value: serde_json::Value = serde_json::from_str(&real_text()).unwrap();
        edit(&mut value);
        value.to_string()
    }

    #[test]
    fn real_scene_config_parses() {
        let config = real_config();
        assert_eq!(config.reference_resolution, [1920, 1080]);
        assert!(approx(
            &config.projection.yaw_deg,
            &[45.0, 135.0, 225.0, 315.0]
        ));
        assert!((config.projection.pitch_deg - 30.0).abs() < 1e-6);
        assert!(!config.projection.true_cubes);
        assert!(approx(&config.camera.pivot_world, &[-8.5, 9.5, 0.0]));
        assert!(approx(
            &config.camera.zoom_levels,
            &[0.5, 0.75, 1.0, 1.5, 2.0]
        ));
        let grid = &config.ops_grid;
        assert!((grid.minor_spacing - 6.0).abs() < 1e-6);
        assert!((grid.major_spacing - 24.0).abs() < 1e-6);
        assert!(approx(&grid.minor_offset_xy, &[4.0, 0.0]));
        assert!(approx(&grid.major_offset_xy, &[22.0, 18.0]));
        assert_eq!(grid.minor.color, Rgb::new(0x6f, 0xd6, 0xff));
        assert_eq!(grid.major.color, Rgb::new(0x8f, 0xe2, 0xff));
        assert_eq!(config.background.color_center, Rgb::new(0x18, 0x24, 0x3a));
    }

    #[test]
    fn unknown_format_version_is_rejected() {
        let text = edited(|v| v["format"] = "voxel-rts/scene-config@2".into());
        match parse(&text) {
            Err(ConfigError::UnsupportedFormat {
                found, expected, ..
            }) => {
                assert_eq!(found, "voxel-rts/scene-config@2");
                assert_eq!(expected, SCENE_CONFIG_FORMAT);
            }
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
        // A different schema under an unknown version still reports the version, not the schema.
        let text = r#"{ "format": "voxel-rts/scene-config@9", "something": "else" }"#;
        assert!(matches!(
            parse(text),
            Err(ConfigError::UnsupportedFormat { .. })
        ));
    }

    #[test]
    fn missing_format_is_a_parse_error() {
        assert!(matches!(parse("{}"), Err(ConfigError::Parse { .. })));
        assert!(matches!(parse("not json"), Err(ConfigError::Parse { .. })));
    }

    #[test]
    fn malformed_colour_is_rejected() {
        let text = edited(|v| v["ops_grid"]["minor"]["color"] = "cyan".into());
        assert!(matches!(parse(&text), Err(ConfigError::Parse { .. })));
    }

    #[test]
    fn invalid_values_are_rejected() {
        type Edit = fn(&mut serde_json::Value);
        let cases: [(&str, Edit); 6] = [
            ("alpha_far zero", |v| {
                v["ops_grid"]["major"]["alpha_far"] = 0.0.into();
            }),
            ("zoom without 1.0", |v| {
                v["camera"]["zoom_levels"] = serde_json::json!([0.5, 2.0]);
            }),
            ("zoom unsorted", |v| {
                v["camera"]["zoom_levels"] = serde_json::json!([1.0, 0.5]);
            }),
            ("three yaw angles", |v| {
                v["projection"]["yaw_deg"] = serde_json::json!([45, 135, 225]);
            }),
            ("pitch 90", |v| v["projection"]["pitch_deg"] = 90.0.into()),
            ("spacing zero", |v| {
                v["ops_grid"]["minor_spacing"] = 0.into();
            }),
        ];
        for (name, edit) in cases {
            let text = edited(edit);
            assert!(
                matches!(parse(&text), Err(ConfigError::Invalid { .. })),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn true_cubes_flag_is_optional() {
        let text = edited(|v| v["projection"]["true_cubes"] = true.into());
        let config = parse(&text).unwrap();
        assert!(config.projection.true_cubes);
        assert!((config.projection.effective_z_scale() - 1.0).abs() < 1e-6);
        assert!((real_config().projection.effective_z_scale() - 0.8165).abs() < 1e-6);
    }
}
