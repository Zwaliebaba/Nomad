//! `assets/layout/scene_config.json`, format `voxel-rts/scene-config@1` (phase-1 doc 03).
//!
//! Only the fields the client uses so far are declared; serde ignores the rest (pylons, the
//! elevation tier list, lights policy and the prose notes), so later milestones can add them
//! without breaking older files.
//!
//! Every declared field is required, including the tuning values D-063 moved out of the code
//! (pan speed, ops-area bounds, background and stars), the M3 looks (elevation cues, bloom,
//! hover) and the M4 shield (shape, centre height, vertical radius, below-grid opacity, lines and
//! hits): the file is the one source of truth for them, so there are no fallback constants in
//! the code that could silently disagree with it. The only optional field is
//! `projection.true_cubes` (default `false`).

use std::path::Path;

use serde::Deserialize;

use super::{ConfigError, Rgb, check_format};

/// The one format version this build reads.
pub const SCENE_CONFIG_FORMAT: &str = "voxel-rts/scene-config@1";

/// Most nebula ellipses the background shader draws.
pub const MAX_NEBULAE: usize = 4;
/// Upper bound on the star count, far above the ~200 of doc 01 §3 (D-061). It only guards
/// against a typo allocating millions of instances.
pub const MAX_STARS: u32 = 10_000;

/// The parsed file. `format` is checked separately before this is read (see [`Self::from_json`]).
#[derive(Clone, Debug, Deserialize)]
pub struct SceneConfig {
    /// Layout reference size in pixels: `[1920, 1080]` (phase-1 README, ground rule 2).
    pub reference_resolution: [u32; 2],
    pub projection: ProjectionConfig,
    pub camera: CameraConfig,
    pub ops_grid: OpsGridConfig,
    pub background: BackgroundConfig,
    pub elevation: ElevationConfig,
    pub bloom: BloomConfig,
    pub hover: HoverConfig,
    pub shield: ShieldConfig,
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
    /// (doc 01 §1 asks for this behind a config flag; phase-1 doc 07 item 8; D-063).
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
    /// Keyboard and edge-scroll pan speed in reference pixels per second (D-063: 800).
    pub pan_speed_ref_px_per_s: f32,
    /// Pan clamp: the pivot stays within this many voxels of the Home pivot on each ground axis,
    /// the phase-1 ops-area bounds (doc 02, D-046, D-063: 200).
    pub ops_area_half_extent_voxels: f32,
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
    /// Lines keep `alpha_near` within this ground distance of the camera pivot.
    pub near_radius_voxels: f32,
    /// Lines reach `alpha_far` at this ground distance from the camera pivot and keep it beyond;
    /// the fade between the two radii is linear (D-059).
    pub far_radius_voxels: f32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct GridLineStyle {
    pub color: Rgb,
    pub width_px: f32,
    pub alpha_far: f32,
    pub alpha_near: f32,
}

/// Background, doc 01 §3. Values doc 01 gives only in prose (nebula positions, blur, sun glow,
/// star colour) come from the mockup behind the reference renders
/// (`reference/mockup/ops-grid-proposal.html`). All positions and sizes are in reference pixels
/// on the 1920×1080 canvas.
#[derive(Clone, Debug, Deserialize)]
pub struct BackgroundConfig {
    pub color_center: Rgb,
    pub color_mid: Rgb,
    pub color_edge: Rgb,
    /// Centre of the radial gradient in canvas units (0..1 across the canvas).
    pub gradient_center: [f32; 2],
    /// Radius of the gradient in canvas units.
    pub gradient_radius: f32,
    /// Offset of `color_mid` along the gradient, in `(0, 1)`.
    pub gradient_mid_stop: f32,
    pub nebulae: NebulaeConfig,
    pub sun: SunConfig,
    pub stars: StarsConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NebulaeConfig {
    pub opacity: f32,
    pub rotation_deg: f32,
    pub blur_sigma_ref_px: f32,
    /// At most [`MAX_NEBULAE`].
    pub ellipses: Vec<NebulaEllipse>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct NebulaEllipse {
    pub center_ref_px: [f32; 2],
    pub radii_ref_px: [f32; 2],
    pub color: Rgb,
}

/// Soft sun glow: a radial gradient with three stops at offsets 0, `mid_stop` and 1.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct SunConfig {
    pub center_ref_px: [f32; 2],
    pub radius_ref_px: f32,
    /// Opacity of the whole glow, applied on top of the stop alphas.
    pub opacity: f32,
    pub mid_stop: f32,
    pub inner: ColorStop,
    pub mid: ColorStop,
    pub outer: ColorStop,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct ColorStop {
    pub color: Rgb,
    pub alpha: f32,
}

/// The star field (doc 01 §3, D-061: about 200 stars).
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct StarsConfig {
    /// At most [`MAX_STARS`].
    pub count: u32,
    /// Smallest and largest radius in reference pixels.
    pub radius_ref_px: [f32; 2],
    /// Smallest and largest opacity.
    pub alpha: [f32; 2],
    pub color: Rgb,
    /// Seed of the deterministic star scatter, so every run and screenshot shows the same sky.
    pub seed: u64,
}

/// Elevation cues, doc 01 §7: a dashed footprint on the ops grid under every module with
/// `elevation_tier > 0`, and dashed stalks from its visible bottom corners down to the grid. Doc 01
/// gives the footprint colour and alphas; widths, dash patterns and the stalk alpha are the
/// mockup's (`reference/mockup/ops-grid-proposal.html`).
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct ElevationConfig {
    pub footprint: FootprintStyle,
    pub stalk: CueLineStyle,
}

/// The footprint outline and its fill.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct FootprintStyle {
    #[serde(flatten)]
    pub line: CueLineStyle,
    /// Opacity of the fill inside the outline (doc 01 §7: 0.06).
    pub fill_alpha: f32,
}

/// A dashed overlay line. Widths and dashes are 1080p pixels and scale with the window, not with
/// the zoom, like the ops-grid lines (D-063).
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct CueLineStyle {
    pub color: Rgb,
    pub alpha: f32,
    pub width_ref_px: f32,
    /// Dash and gap length. A gap of 0 draws a solid line.
    pub dash_ref_px: [f32; 2],
}

/// Bloom on emissive palette entries, doc 01 §2: "small radius, ~6 px at zoom 1, intensity
/// modest".
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct BloomConfig {
    /// Glow radius in 1080p pixels at zoom 1. It scales with the zoom and the window size; the
    /// Gaussian sigma is half of it.
    pub radius_ref_px: f32,
    /// Factor on the blurred emissive image before it is added to the frame.
    pub intensity: f32,
    pub composite: BloomComposite,
}

/// How the glow is added to the frame. Both only ever add light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BloomComposite {
    /// `frame + glow`, clipped at 1.
    Additive,
    /// `frame + glow · (1 − frame)`: additive, scaled by the headroom left, so a pale emitter
    /// such as the pylon emitters' `#b8f6ff` keeps its tint instead of clipping to white.
    Screen,
}

/// Hover highlight (doc 06 M3: "hover a module → highlight").
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct HoverConfig {
    /// Non-emissive faces of the hovered module are multiplied by `1 + brightness_lift`.
    pub brightness_lift: f32,
}

/// The power shield (doc 01 §6, superseded in part by D-100; doc 06 M4): a sectored, translucent
/// ellipsoid around the station with a rim, a sector ring and scripted hit ripples. Sector
/// strengths are live data and come from the fixture (`selection.pylons`), not from here.
#[derive(Clone, Debug, Deserialize)]
pub struct ShieldConfig {
    pub shape: ShieldShape,
    /// Centre in world voxels. D-100: z is the ship plane (16); with [`ShieldShape::Dome`] it is
    /// the plane the cap rests on.
    pub center: [f32; 3],
    /// Horizontal radius: the equator, which is the 2D hit circle (doc 15 §5.1; D-119).
    pub radius_xy: f32,
    /// Vertical radius in world voxels, before the projection's z scale. Tuned at M4 so the
    /// solar array reads as outside the shield (D-100).
    pub radius_z: f32,
    /// Factor on the opacity of everything drawn below the ops grid (`z < 0`; D-100).
    pub below_grid_opacity: f32,
    /// Fill opacity of one surface layer where it faces the viewer (doc 01 §6: about 0.05).
    pub rest_alpha_fill: f32,
    /// Fill opacity at the silhouette, reached as `fresnel^fresnel_power`.
    pub edge_alpha_fill: f32,
    pub fresnel_power: f32,
    pub rim_color: Rgb,
    /// The silhouette line (doc 01 §6: alpha 0.75).
    pub rim_alpha: f32,
    pub rim_width_ref_px: f32,
    /// The silhouette over a weak sector, in `weak_color`.
    pub rim_weak: ShieldLine,
    /// A sector's arc of the ring at the equator (doc 01 §6: 2 px).
    pub ring: ShieldLine,
    /// The arc of a sector below `weak_threshold`, in `weak_color`.
    pub ring_weak: ShieldLine,
    /// The far half of the ring, seen through the globe.
    pub ring_far: ShieldLine,
    pub sectors: Vec<ShieldSector>,
    /// A sector below this strength is drawn in `weak_color` and dashed (doc 01 §6).
    pub weak_threshold: f32,
    pub weak_color: Rgb,
    pub hits: ShieldHits,
}

/// The shield's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShieldShape {
    /// A full ellipsoid centred on the ship plane (D-100).
    Globe,
    /// The phase-1 cap: only the part above `center` z (doc 01 §6, superseded; kept to compare).
    Dome,
}

/// A shield line style. Widths and dashes are 1080p pixels and scale with the window, like the
/// elevation cues.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct ShieldLine {
    pub alpha: f32,
    pub width_ref_px: f32,
    /// Dash and gap length. A gap of 0 draws a solid line.
    pub dash_ref_px: [f32; 2],
}

/// One shield sector: an arc of azimuths around the centre, in degrees from +x towards +y,
/// centred on its pylon (D-046).
#[derive(Clone, Debug, Deserialize)]
pub struct ShieldSector {
    /// The pylon feeding it, as in `station_scene.json` (`pylon_e`); its strength is the
    /// fixture's `selection.pylons` entry for the letter after `pylon_` (`E`).
    pub pylon: String,
    /// Start and end azimuth, start < end, at most 360° apart; may run past 360.
    pub az_deg: [f32; 2],
}

/// The scripted hit ripples of doc 06 M4 (doc 01 §6: hex ripple at the impact point, 600 ms,
/// radius 4–6 voxels, plus a hot flash).
#[derive(Clone, Debug, Deserialize)]
pub struct ShieldHits {
    /// The script repeats with this period.
    pub loop_ms: u32,
    /// How long one ripple lasts.
    pub ripple_ms: u32,
    /// How long the hot flash at the impact point lasts (0: no flash).
    pub flash_ms: u32,
    /// Width of one hex cell, in voxels along the surface.
    pub hex_cell_voxels: f32,
    pub hex_color: Rgb,
    pub hex_alpha: f32,
    /// The ripple's edge, in a healthy sector.
    pub outline_color: Rgb,
    /// The ripple's edge, in a sector below `weak_threshold`.
    pub outline_weak_color: Rgb,
    pub outline_alpha: f32,
    /// The flash's centre and edge colour (doc 01 §6).
    pub flash_core: Rgb,
    pub flash_edge: Rgb,
    pub flash_radius_voxels: f32,
    pub impacts: Vec<ShieldImpact>,
}

/// One scripted impact.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct ShieldImpact {
    /// Where the reference render shows it: pixels at the reference resolution in view 0 at zoom
    /// 1. The client puts the impact where this pixel's view ray meets the shield's front surface.
    pub reference_px: [f32; 2],
    /// The ripple's final radius along the surface.
    pub radius_voxels: f32,
    /// When in the loop it hits.
    pub delay_ms: u32,
}

/// Most scripted impacts the shield shader takes.
pub const MAX_SHIELD_IMPACTS: usize = 4;
/// Most shield sectors accepted.
pub const MAX_SHIELD_SECTORS: usize = 8;
/// Widest shield line accepted, in 1080p pixels.
pub const MAX_SHIELD_LINE_REF_PX: f32 = 16.0;

/// Largest bloom radius accepted, in 1080p pixels. The blur's tap count grows with the radius.
pub const MAX_BLOOM_RADIUS_REF_PX: f32 = 32.0;
/// Widest elevation-cue line accepted, in 1080p pixels.
pub const MAX_CUE_WIDTH_REF_PX: f32 = 16.0;

/// A rejected field and the reason.
type Rejection = (&'static str, String);

impl SceneConfig {
    /// Parses and validates the JSON text of a scene config. `path` is used in error messages.
    pub fn from_json(text: &str, path: &Path) -> Result<Self, ConfigError> {
        check_format(text, path, SCENE_CONFIG_FORMAT)?;
        let config: Self = serde_json::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
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
    fn validate(&self) -> Result<(), Rejection> {
        let [w, h] = self.reference_resolution;
        if w == 0 || h == 0 {
            return Err(("reference_resolution", "must be positive".into()));
        }
        self.validate_projection()?;
        self.validate_camera()?;
        self.validate_grid()?;
        self.background.validate()?;
        self.validate_looks()?;
        self.shield.validate()
    }

    /// Elevation cues, bloom and hover (M3).
    fn validate_looks(&self) -> Result<(), Rejection> {
        let line_ok = |style: &CueLineStyle| {
            unit(style.alpha)
                && finite_positive(style.width_ref_px)
                && style.width_ref_px <= MAX_CUE_WIDTH_REF_PX
                && finite_positive(style.dash_ref_px[0])
                && style.dash_ref_px[1].is_finite()
                && style.dash_ref_px[1] >= 0.0
        };
        let line_rule = format!(
            "alphas must be in [0, 1], width in (0, {MAX_CUE_WIDTH_REF_PX}], dash positive and \
             gap not negative"
        );
        let footprint = &self.elevation.footprint;
        if !(line_ok(&footprint.line) && unit(footprint.fill_alpha)) {
            return Err(("elevation.footprint", line_rule));
        }
        if !line_ok(&self.elevation.stalk) {
            return Err(("elevation.stalk", line_rule));
        }
        let bloom = &self.bloom;
        if !(bloom.radius_ref_px.is_finite()
            && (0.0..=MAX_BLOOM_RADIUS_REF_PX).contains(&bloom.radius_ref_px))
        {
            return Err((
                "bloom.radius_ref_px",
                format!("must be between 0 and {MAX_BLOOM_RADIUS_REF_PX}"),
            ));
        }
        if !(bloom.intensity.is_finite() && (0.0..=4.0).contains(&bloom.intensity)) {
            return Err(("bloom.intensity", "must be between 0 and 4".into()));
        }
        if !unit(self.hover.brightness_lift) {
            return Err(("hover.brightness_lift", "must be in [0, 1]".into()));
        }
        Ok(())
    }

    fn validate_projection(&self) -> Result<(), Rejection> {
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
        Ok(())
    }

    fn validate_camera(&self) -> Result<(), Rejection> {
        let camera = &self.camera;
        // Phase 1 has a single ops-grid plane at z = 0 (doc 01 §1, §4); the camera's ground-plane
        // maths and the grid pass rely on it.
        if camera.pivot_world[2] != 0.0 {
            return Err(("camera.pivot_world", "the pivot must lie on z = 0".into()));
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
        if !finite_positive(camera.pan_speed_ref_px_per_s) {
            return Err(("camera.pan_speed_ref_px_per_s", "must be positive".into()));
        }
        if !(camera.ops_area_half_extent_voxels.is_finite()
            && camera.ops_area_half_extent_voxels >= 0.0)
        {
            return Err((
                "camera.ops_area_half_extent_voxels",
                "must not be negative".into(),
            ));
        }
        Ok(())
    }

    fn validate_grid(&self) -> Result<(), Rejection> {
        let grid = &self.ops_grid;
        if grid.plane_z != 0.0 {
            return Err(("ops_grid.plane_z", "phase 1 supports only z = 0".into()));
        }
        if !(positive(grid.minor_spacing) && positive(grid.major_spacing)) {
            return Err(("ops_grid.*_spacing", "must be positive".into()));
        }
        if !(grid.near_radius_voxels.is_finite() && grid.near_radius_voxels >= 0.0) {
            return Err(("ops_grid.near_radius_voxels", "must not be negative".into()));
        }
        // The shader divides by far − near.
        if !(grid.far_radius_voxels.is_finite() && grid.far_radius_voxels > grid.near_radius_voxels)
        {
            return Err((
                "ops_grid.far_radius_voxels",
                "must be larger than near_radius_voxels".into(),
            ));
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

impl BackgroundConfig {
    fn validate(&self) -> Result<(), Rejection> {
        if !(finite_positive(self.gradient_radius)
            && self.gradient_center.iter().all(|v| v.is_finite()))
        {
            return Err((
                "background.gradient_*",
                "centre must be finite and radius positive".into(),
            ));
        }
        if !open_unit(self.gradient_mid_stop) {
            return Err(("background.gradient_mid_stop", "must be in (0, 1)".into()));
        }

        let nebulae = &self.nebulae;
        if nebulae.ellipses.len() > MAX_NEBULAE {
            return Err((
                "background.nebulae.ellipses",
                format!("at most {MAX_NEBULAE} are supported"),
            ));
        }
        if !(unit(nebulae.opacity)
            && nebulae.rotation_deg.is_finite()
            && finite_positive(nebulae.blur_sigma_ref_px))
        {
            return Err((
                "background.nebulae",
                "opacity must be in [0, 1], rotation finite and blur positive".into(),
            ));
        }
        let ellipse_ok = |e: &NebulaEllipse| {
            e.center_ref_px.iter().all(|v| v.is_finite())
                && e.radii_ref_px.iter().all(|&r| finite_positive(r))
        };
        if !nebulae.ellipses.iter().all(ellipse_ok) {
            return Err((
                "background.nebulae.ellipses",
                "centres must be finite and radii positive".into(),
            ));
        }

        let sun = &self.sun;
        let stops_ok = [sun.inner, sun.mid, sun.outer]
            .iter()
            .all(|stop| unit(stop.alpha));
        if !(sun.center_ref_px.iter().all(|v| v.is_finite())
            && finite_positive(sun.radius_ref_px)
            && unit(sun.opacity)
            && open_unit(sun.mid_stop)
            && stops_ok)
        {
            return Err((
                "background.sun",
                "radius must be positive, mid_stop in (0, 1), opacity and alphas in [0, 1]".into(),
            ));
        }

        let stars = &self.stars;
        if stars.count > MAX_STARS {
            return Err((
                "background.stars.count",
                format!("at most {MAX_STARS} are supported"),
            ));
        }
        let [r_min, r_max] = stars.radius_ref_px;
        if !(finite_positive(r_min) && r_max.is_finite() && r_min <= r_max) {
            return Err((
                "background.stars.radius_ref_px",
                "expected [min, max] with 0 < min <= max".into(),
            ));
        }
        let [a_min, a_max] = stars.alpha;
        if !(unit(a_min) && unit(a_max) && a_min <= a_max) {
            return Err((
                "background.stars.alpha",
                "expected [min, max] within [0, 1] with min <= max".into(),
            ));
        }
        Ok(())
    }
}

/// False for zero, negatives and NaN.
impl ShieldConfig {
    /// The shield (M4).
    fn validate(&self) -> Result<(), Rejection> {
        if !self.center.iter().all(|c| c.is_finite()) {
            return Err(("shield.center", "must be finite".into()));
        }
        if !(finite_positive(self.radius_xy) && finite_positive(self.radius_z)) {
            return Err(("shield.radius_xy / radius_z", "must be positive".into()));
        }
        for (field, value) in [
            ("shield.below_grid_opacity", self.below_grid_opacity),
            ("shield.rest_alpha_fill", self.rest_alpha_fill),
            ("shield.edge_alpha_fill", self.edge_alpha_fill),
            ("shield.rim_alpha", self.rim_alpha),
            ("shield.weak_threshold", self.weak_threshold),
            ("shield.hits.hex_alpha", self.hits.hex_alpha),
            ("shield.hits.outline_alpha", self.hits.outline_alpha),
        ] {
            if !unit(value) {
                return Err((field, "must be in [0, 1]".into()));
            }
        }
        if !(finite_positive(self.fresnel_power) && self.fresnel_power <= 64.0) {
            return Err(("shield.fresnel_power", "must be in (0, 64]".into()));
        }
        let width_ok = |w: f32| finite_positive(w) && w <= MAX_SHIELD_LINE_REF_PX;
        if !width_ok(self.rim_width_ref_px) {
            return Err((
                "shield.rim_width_ref_px",
                format!("must be in (0, {MAX_SHIELD_LINE_REF_PX}]"),
            ));
        }
        for (field, line) in [
            ("shield.rim_weak", &self.rim_weak),
            ("shield.ring", &self.ring),
            ("shield.ring_weak", &self.ring_weak),
            ("shield.ring_far", &self.ring_far),
        ] {
            let ok = unit(line.alpha)
                && width_ok(line.width_ref_px)
                && finite_positive(line.dash_ref_px[0])
                && line.dash_ref_px[1].is_finite()
                && line.dash_ref_px[1] >= 0.0;
            if !ok {
                return Err((
                    field,
                    format!(
                        "alpha must be in [0, 1], width in (0, {MAX_SHIELD_LINE_REF_PX}], dash \
                         positive and gap not negative"
                    ),
                ));
            }
        }
        if self.sectors.is_empty() || self.sectors.len() > MAX_SHIELD_SECTORS {
            return Err((
                "shield.sectors",
                format!("expected 1 to {MAX_SHIELD_SECTORS} sectors"),
            ));
        }
        for sector in &self.sectors {
            let [start, end] = sector.az_deg;
            if !(start.is_finite() && end.is_finite() && start < end && end - start <= 360.0) {
                return Err((
                    "shield.sectors.az_deg",
                    "start must be below end, at most 360° apart".into(),
                ));
            }
        }
        self.hits.validate()
    }
}

impl ShieldHits {
    fn validate(&self) -> Result<(), Rejection> {
        if self.loop_ms == 0 || self.ripple_ms == 0 {
            return Err(("shield.hits.loop_ms / ripple_ms", "must be positive".into()));
        }
        if !(finite_positive(self.hex_cell_voxels) && finite_positive(self.flash_radius_voxels)) {
            return Err((
                "shield.hits.hex_cell_voxels / flash_radius_voxels",
                "must be positive".into(),
            ));
        }
        if self.impacts.len() > MAX_SHIELD_IMPACTS {
            return Err((
                "shield.hits.impacts",
                format!("at most {MAX_SHIELD_IMPACTS} impacts"),
            ));
        }
        for impact in &self.impacts {
            if !(impact.reference_px.iter().all(|c| c.is_finite())
                && finite_positive(impact.radius_voxels))
            {
                return Err((
                    "shield.hits.impacts",
                    "reference_px must be finite and radius_voxels positive".into(),
                ));
            }
        }
        Ok(())
    }
}

fn positive(value: f32) -> bool {
    value > 0.0
}

/// Positive and not infinite.
fn finite_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

/// Within `[0, 1]`.
fn unit(value: f32) -> bool {
    (0.0..=1.0).contains(&value)
}

/// Within `(0, 1)`.
fn open_unit(value: f32) -> bool {
    value > 0.0 && value < 1.0
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

    /// M3 looks: the doc 01 §7 footprint colour and alphas, the mockup's dashes and the doc 01 §2
    /// bloom radius.
    #[test]
    fn m3_looks_come_from_the_file() {
        let config = real_config();
        let footprint = &config.elevation.footprint;
        assert_eq!(footprint.line.color, Rgb::new(0x38, 0xd6, 0xff));
        assert!((footprint.line.alpha - 0.7).abs() < 1e-6);
        assert!((footprint.fill_alpha - 0.06).abs() < 1e-6);
        assert!(approx(&footprint.line.dash_ref_px, &[4.0, 4.0]));
        let stalk = &config.elevation.stalk;
        assert_eq!(stalk.color, Rgb::new(0x38, 0xd6, 0xff));
        assert!(approx(&stalk.dash_ref_px, &[2.0, 3.0]));
        assert!((config.bloom.radius_ref_px - 6.0).abs() < 1e-6);
        assert!(config.bloom.intensity > 0.0 && config.bloom.intensity <= 1.0);
        assert_eq!(config.bloom.composite, BloomComposite::Screen);
        assert!(config.hover.brightness_lift > 0.0 && config.hover.brightness_lift < 0.5);
    }

    /// D-063 values: pan speed 800 px/s, ±200-voxel ops area, and the M1 background (D-061:
    /// about 200 stars).
    #[test]
    fn tuning_values_come_from_the_file() {
        let config = real_config();
        assert!((config.camera.pan_speed_ref_px_per_s - 800.0).abs() < 1e-6);
        assert!((config.camera.ops_area_half_extent_voxels - 200.0).abs() < 1e-6);
        let background = &config.background;
        assert_eq!(background.color_mid, Rgb::new(0x0c, 0x13, 0x22));
        assert_eq!(background.nebulae.ellipses.len(), 3);
        assert!((background.nebulae.opacity - 0.35).abs() < 1e-6);
        assert!((background.nebulae.rotation_deg + 15.0).abs() < 1e-6);
        let stars = &background.stars;
        assert_eq!(stars.count, 200);
        assert!(approx(&stars.radius_ref_px, &[0.5, 1.1]));
        assert!(approx(&stars.alpha, &[0.15, 0.6]));
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

    /// The tuning fields are required, not defaulted (module docs).
    #[test]
    fn missing_tuning_values_are_rejected() {
        type Edit = fn(&mut serde_json::Value);
        let cases: [(&str, Edit); 10] = [
            ("shield shape", |v| {
                v["shield"].as_object_mut().unwrap().remove("shape");
            }),
            ("shield vertical radius", |v| {
                v["shield"].as_object_mut().unwrap().remove("radius_z");
            }),
            ("shield below-grid opacity", |v| {
                v["shield"]
                    .as_object_mut()
                    .unwrap()
                    .remove("below_grid_opacity");
            }),
            ("shield hits", |v| {
                v["shield"].as_object_mut().unwrap().remove("hits");
            }),
            ("pan speed", |v| {
                v["camera"]
                    .as_object_mut()
                    .unwrap()
                    .remove("pan_speed_ref_px_per_s");
            }),
            ("ops area", |v| {
                v["camera"]
                    .as_object_mut()
                    .unwrap()
                    .remove("ops_area_half_extent_voxels");
            }),
            ("stars", |v| {
                v["background"].as_object_mut().unwrap().remove("stars");
            }),
            ("far radius", |v| {
                v["ops_grid"]
                    .as_object_mut()
                    .unwrap()
                    .remove("far_radius_voxels");
            }),
            ("bloom", |v| {
                v.as_object_mut().unwrap().remove("bloom");
            }),
            ("footprint fill", |v| {
                v["elevation"]["footprint"]
                    .as_object_mut()
                    .unwrap()
                    .remove("fill_alpha");
            }),
        ];
        for (name, edit) in cases {
            assert!(
                matches!(parse(&edited(edit)), Err(ConfigError::Parse { .. })),
                "missing {name} should be rejected"
            );
        }
    }

    #[test]
    fn invalid_values_are_rejected() {
        type Edit = fn(&mut serde_json::Value);
        let cases: [(&str, Edit); 25] = [
            ("shield centre not finite", |v| {
                v["shield"]["center"] = serde_json::json!([0, 0, 1e40]);
            }),
            ("shield vertical radius zero", |v| {
                v["shield"]["radius_z"] = 0.into();
            }),
            ("below-grid opacity above 1", |v| {
                v["shield"]["below_grid_opacity"] = 1.5.into();
            }),
            ("ring width huge", |v| {
                v["shield"]["ring"]["width_ref_px"] = 100.into();
            }),
            ("sector arc reversed", |v| {
                v["shield"]["sectors"][0]["az_deg"] = serde_json::json!([270, 180]);
            }),
            ("too many impacts", |v| {
                let one = v["shield"]["hits"]["impacts"][0].clone();
                v["shield"]["hits"]["impacts"] =
                    serde_json::Value::Array(vec![one; MAX_SHIELD_IMPACTS + 1]);
            }),
            ("ripple of zero length", |v| {
                v["shield"]["hits"]["ripple_ms"] = 0.into();
            }),
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
            ("far radius inside near radius", |v| {
                v["ops_grid"]["far_radius_voxels"] = 10.into();
            }),
            ("pan speed zero", |v| {
                v["camera"]["pan_speed_ref_px_per_s"] = 0.into();
            }),
            ("negative ops area", |v| {
                v["camera"]["ops_area_half_extent_voxels"] = (-1).into();
            }),
            ("too many stars", |v| {
                v["background"]["stars"]["count"] = (MAX_STARS + 1).into();
            }),
            ("star radius range reversed", |v| {
                v["background"]["stars"]["radius_ref_px"] = serde_json::json!([1.1, 0.5]);
            }),
            ("too many nebulae", |v| {
                let one = v["background"]["nebulae"]["ellipses"][0].clone();
                v["background"]["nebulae"]["ellipses"] =
                    serde_json::Value::Array(vec![one; MAX_NEBULAE + 1]);
            }),
            ("sun mid stop at 1", |v| {
                v["background"]["sun"]["mid_stop"] = 1.0.into();
            }),
            ("footprint alpha above 1", |v| {
                v["elevation"]["footprint"]["alpha"] = 1.5.into();
            }),
            ("stalk dash zero", |v| {
                v["elevation"]["stalk"]["dash_ref_px"] = serde_json::json!([0, 3]);
            }),
            ("bloom radius huge", |v| {
                v["bloom"]["radius_ref_px"] = 1000.into();
            }),
            ("bloom intensity negative", |v| {
                v["bloom"]["intensity"] = (-0.5).into();
            }),
            ("hover lift above 1", |v| {
                v["hover"]["brightness_lift"] = 2.into();
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

    /// D-100: a globe centred on the ship plane, its equator the 36-voxel hit circle (D-119).
    #[test]
    fn shield_is_a_globe_on_the_ship_plane() {
        let shield = real_config().shield;
        assert_eq!(shield.shape, ShieldShape::Globe);
        assert!(approx(&shield.center, &[-8.6, 9.4, 16.0]));
        assert!((shield.radius_xy - 36.0).abs() < 1e-6);
        assert!(
            shield.radius_z > shield.center[2],
            "part of the globe lies below the grid"
        );
        assert_eq!(shield.sectors.len(), 4);
        assert_eq!(shield.hits.impacts.len(), 2);
        let dome = edited(|v| v["shield"]["shape"] = "dome".into());
        assert_eq!(parse(&dome).unwrap().shield.shape, ShieldShape::Dome);
        let cube = edited(|v| v["shield"]["shape"] = "cube".into());
        assert!(matches!(parse(&cube), Err(ConfigError::Parse { .. })));
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
