//! Camera: orthographic 2:1 dimetric view with snap yaw, zoom steps and pan
//! (phase-1 doc 01 §1, doc 02).
//!
//! # How the camera reproduces the doc 01 formula
//!
//! Doc 01 §1 fixes view 0 at zoom 1 and 1920×1080 as
//! `sx = 1060 + 5(x − y)`, `sy = 490 + 2.5(x + y) − 5z`. As a 3D camera:
//!
//! - Screen right on the ground is `R(ψ) = (cos ψ, −sin ψ)` and screen up on the ground is
//!   `H(ψ) = (−sin ψ, −cos ψ)` for yaw `ψ`. At ψ = 45° this gives `R = (1, −1)/√2` and
//!   `H = (−1, −1)/√2`, so grid north (azimuth 225°, D-046) is screen-up in view 0. At ψ = 135°
//!   the solar array lands right of the pivot and the asteroids left of it, as in
//!   `reference/renders/station-view-1.png`.
//! - The view is orthographic, tilted down by the pitch θ = 30°. A ground offset `d` moves
//!   `k·(R·d)` pixels right and `k·sin θ·(H·d)` pixels up; a height `z` moves `k·cos θ·s_z·z`
//!   pixels up, where `s_z` is the z scale (0.8165) applied in the world → view matrix.
//! - `k`, pixels per world unit, follows from `screen_x_per_world_x = k·cos 45°`: `k = 5√2`.
//!   Then `k·sin 45°·sin 30° = 2.5` and `k·cos 30°·0.8165 = 5.0000`, matching the formula
//!   (0.8165 ≈ √(2/3), and cos 30°·√(2/3) = 1/√2 exactly).
//! - The pivot `(−8.5, 9.5, 0)` projects to (970, 492.5), not to the screen centre. The camera
//!   therefore keeps the pivot at a fixed screen *anchor*, the centre plus (10, −47.5) reference
//!   pixels, derived from `origin_screen_view0`. Rotation and zoom happen about that anchor, which
//!   keeps the formula exact in view 0 (phase-1 doc 07, item 12).
//!
//! All lengths scale uniformly with the window by `min(w / 1920, h / 1080)` (phase-1 README,
//! ground rule 2).

use std::collections::VecDeque;

use glam::{Mat4, Vec2, Vec3, Vec4};

use crate::config::SceneConfig;

/// Duration of a 90° snap turn (doc 02).
pub const TURN_DURATION_S: f32 = 0.25;
/// Duration of a zoom step (doc 02).
pub const ZOOM_DURATION_S: f32 = 0.15;
/// Pan clamp: ±200 voxels around the Home pivot, the phase-1 ops-area bounds (doc 02, D-046).
/// `scene_config.json` names the clamp but has no number for it.
pub const OPS_AREA_HALF_EXTENT_VOXELS: f32 = 200.0;
/// Half the depth range of the orthographic volume, in world units along the view direction.
/// Generous: the station is about 35 voxels tall and the visible ground spans a few hundred.
const DEPTH_HALF_RANGE_VOXELS: f32 = 2048.0;
/// How close the effective zoom must be to an integer for pixel snapping.
const SNAP_EPSILON: f32 = 1e-4;

/// A 90° snap turn: Q is −90°, E is +90° (doc 02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    Minus90,
    Plus90,
}

/// One zoom step on the wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomStep {
    In,
    Out,
}

/// Session-constant projection parameters derived from `scene_config.json`.
#[derive(Clone, Debug)]
pub struct Projection {
    base_yaw_deg: f32,
    sin_pitch: f32,
    cos_pitch: f32,
    z_scale: f32,
    /// Pixels per world unit at zoom 1 and the reference resolution (`5√2`).
    px_per_unit: f32,
    reference_size: Vec2,
    /// Screen position of the pivot relative to the screen centre, in reference pixels.
    anchor_offset_ref_px: Vec2,
    home_pivot: Vec2,
    zoom_levels: Vec<f32>,
    home_zoom_index: usize,
}

impl Projection {
    pub fn from_config(config: &SceneConfig) -> Self {
        let projection = &config.projection;
        let base_yaw_deg = *projection
            .yaw_deg
            .first()
            .expect("invariant: SceneConfig::validate checks four yaw angles");
        let (sin_pitch, cos_pitch) = projection.pitch_deg.to_radians().sin_cos();
        let px_per_unit = projection.pixels_per_voxel_at_zoom_1.screen_x_per_world_x
            / base_yaw_deg.to_radians().cos();
        let [ref_w, ref_h] = config.reference_resolution;
        let reference_size = Vec2::new(ref_w as f32, ref_h as f32);
        let [px, py, _] = config.camera.pivot_world;
        let home_pivot = Vec2::new(px, py);
        let home_zoom_index = config
            .camera
            .zoom_levels
            .iter()
            .position(|&z| (z - 1.0).abs() < 1e-6)
            .expect("invariant: SceneConfig::validate checks that zoom 1.0 exists");

        let mut this = Self {
            base_yaw_deg,
            sin_pitch,
            cos_pitch,
            z_scale: projection.effective_z_scale(),
            px_per_unit,
            reference_size,
            anchor_offset_ref_px: Vec2::ZERO,
            home_pivot,
            zoom_levels: config.camera.zoom_levels.clone(),
            home_zoom_index,
        };
        // Where the pivot lands in view 0 at zoom 1, by the doc 01 formula.
        let (right, up_ground) = basis(base_yaw_deg);
        let origin = Vec2::from(projection.origin_screen_view0);
        let pivot_screen = origin
            + Vec2::new(
                right.dot(home_pivot),
                -sin_pitch * up_ground.dot(home_pivot),
            ) * px_per_unit;
        this.anchor_offset_ref_px = pivot_screen - reference_size * 0.5;

        for problem in this.consistency_problems(config) {
            tracing::warn!(%problem, "scene_config projection values disagree");
        }
        this
    }

    /// Checks that the pixel ratios in `scene_config.json` agree with its pitch and z scale. The
    /// camera uses the angles, so a mismatch would silently break the doc 01 formula.
    fn consistency_problems(&self, config: &SceneConfig) -> Vec<String> {
        let ratios = &config.projection.pixels_per_voxel_at_zoom_1;
        let (right, up_ground) = basis(self.base_yaw_deg);
        let implied_y_per_x = -self.sin_pitch * up_ground.x * self.px_per_unit;
        let implied_y_per_z = -self.cos_pitch * config.projection.z_scale * self.px_per_unit;
        let implied_x_per_x = right.x * self.px_per_unit;
        [
            (
                "screen_x_per_world_x",
                ratios.screen_x_per_world_x,
                implied_x_per_x,
            ),
            (
                "screen_y_per_world_x",
                ratios.screen_y_per_world_x,
                implied_y_per_x,
            ),
            (
                "screen_y_per_world_z",
                ratios.screen_y_per_world_z,
                implied_y_per_z,
            ),
        ]
        .into_iter()
        .filter(|(_, stated, implied)| (stated - implied).abs() > 1e-3 * stated.abs().max(1.0))
        .map(|(name, stated, implied)| {
            format!("{name} is {stated} but pitch and z_scale imply {implied}")
        })
        .collect()
    }

    /// Index of a zoom level, if `zoom` is one of the configured steps.
    pub fn zoom_index_of(&self, zoom: f32) -> Option<usize> {
        self.zoom_levels
            .iter()
            .position(|&level| (level - zoom).abs() < 1e-6)
    }

    pub fn zoom_levels(&self) -> &[f32] {
        &self.zoom_levels
    }
}

/// Screen-right and screen-up directions on the ground for a yaw in degrees.
fn basis(yaw_deg: f32) -> (Vec2, Vec2) {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    (Vec2::new(c, -s), Vec2::new(-s, -c))
}

/// Cubic ease-in-out (smoothstep) on `0..=1`.
fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, Debug)]
struct Tween {
    from: f32,
    to: f32,
    elapsed_s: f32,
}

impl Tween {
    fn value(&self, duration_s: f32) -> f32 {
        self.from + (self.to - self.from) * ease_in_out(self.elapsed_s / duration_s)
    }
}

#[derive(Clone, Copy, Debug)]
struct ZoomTween {
    tween: Tween,
    /// Ground point kept under the cursor while zooming.
    focus: Vec2,
}

/// The view camera. Owns view state only; it never touches Universe state.
#[derive(Clone, Debug)]
pub struct Camera {
    projection: Projection,
    viewport_px: Vec2,
    /// Ground point at the screen anchor; rotation is about the vertical axis through it.
    pivot: Vec2,
    /// Snap steps of the current or last turn target; yaw = base + 90°·steps.
    view_steps: i32,
    yaw_deg: f32,
    turn: Option<Tween>,
    /// Turns requested during a turn run after it, in order (doc 02: queued, not dropped).
    queued_turns: VecDeque<Turn>,
    zoom_index: usize,
    zoom: f32,
    zoom_tween: Option<ZoomTween>,
}

impl Camera {
    pub fn new(projection: Projection, viewport_px: Vec2) -> Self {
        let mut camera = Self {
            pivot: projection.home_pivot,
            view_steps: 0,
            yaw_deg: projection.base_yaw_deg,
            turn: None,
            queued_turns: VecDeque::new(),
            zoom_index: projection.home_zoom_index,
            zoom: 1.0,
            zoom_tween: None,
            viewport_px: Vec2::ONE,
            projection,
        };
        camera.set_viewport(viewport_px);
        camera
    }

    pub fn set_viewport(&mut self, viewport_px: Vec2) {
        // A minimised window reports 0×0; keep the maths finite.
        self.viewport_px = viewport_px.max(Vec2::ONE);
    }

    /// Home: view 0, zoom 1, pivot on the deck centre (doc 02). Cancels animations and queued turns.
    pub fn reset_home(&mut self) {
        self.pivot = self.projection.home_pivot;
        self.view_steps = 0;
        self.yaw_deg = self.target_yaw_deg();
        self.turn = None;
        self.queued_turns.clear();
        self.zoom_index = self.projection.home_zoom_index;
        self.zoom = self.projection.zoom_levels[self.zoom_index];
        self.zoom_tween = None;
    }

    /// Jumps to a view without animating.
    pub fn set_view(&mut self, view: u8) {
        self.view_steps = i32::from(view % 4);
        self.yaw_deg = self.target_yaw_deg();
        self.turn = None;
        self.queued_turns.clear();
    }

    /// Jumps to a zoom level without animating. Out-of-range indices are ignored.
    pub fn set_zoom_index(&mut self, index: usize) {
        if let Some(&zoom) = self.projection.zoom_levels.get(index) {
            self.zoom_index = index;
            self.zoom = zoom;
            self.zoom_tween = None;
        }
    }

    /// Starts a snap turn, or queues it behind the turn in progress.
    pub fn rotate(&mut self, turn: Turn) {
        if self.turn.is_some() {
            self.queued_turns.push_back(turn);
        } else {
            self.start_turn(turn);
        }
    }

    fn start_turn(&mut self, turn: Turn) {
        self.view_steps += match turn {
            Turn::Minus90 => -1,
            Turn::Plus90 => 1,
        };
        self.turn = Some(Tween {
            from: self.yaw_deg,
            to: self.target_yaw_deg(),
            elapsed_s: 0.0,
        });
    }

    fn target_yaw_deg(&self) -> f32 {
        self.projection.base_yaw_deg + 90.0 * self.view_steps as f32
    }

    /// Steps the zoom one level towards `step`, easing toward the cursor (doc 02). Returns false
    /// when already at that end of the zoom range.
    pub fn zoom_step(&mut self, step: ZoomStep, cursor_px: Option<Vec2>) -> bool {
        let last = self.projection.zoom_levels.len().saturating_sub(1);
        let target = match step {
            ZoomStep::In => (self.zoom_index + 1).min(last),
            ZoomStep::Out => self.zoom_index.saturating_sub(1),
        };
        if target == self.zoom_index {
            return false;
        }
        let transform = self.transform();
        let focus = transform.screen_to_ground(cursor_px.unwrap_or(transform.anchor_px));
        self.zoom_index = target;
        self.zoom_tween = Some(ZoomTween {
            tween: Tween {
                from: self.zoom,
                to: self.projection.zoom_levels[target],
                elapsed_s: 0.0,
            },
            focus,
        });
        true
    }

    /// Moves the camera by a screen-space offset in pixels: positive x pans right, positive y
    /// pans down. Clamped to the ops-area bounds.
    pub fn pan_screen_px(&mut self, delta_px: Vec2) {
        let transform = self.transform();
        let k = transform.px_per_unit;
        let world = transform.right * (delta_px.x / k)
            - transform.up_ground * (delta_px.y / (k * transform.sin_pitch));
        self.pivot += world;
        if let Some(zoom) = &mut self.zoom_tween {
            // Keep zooming about the same screen point while panning.
            zoom.focus += world;
        }
        self.clamp_pivot();
    }

    /// Drags the ground with the cursor (middle mouse): the scene follows the pointer.
    pub fn drag_px(&mut self, cursor_delta_px: Vec2) {
        self.pan_screen_px(-cursor_delta_px);
    }

    /// Advances animations by `dt_s` seconds.
    pub fn update(&mut self, dt_s: f32) {
        let dt_s = dt_s.max(0.0);
        self.update_turn(dt_s);
        self.update_zoom(dt_s);
        self.clamp_pivot();
    }

    fn update_turn(&mut self, dt_s: f32) {
        let mut remaining_s = dt_s;
        while let Some(mut tween) = self.turn {
            tween.elapsed_s += remaining_s;
            if tween.elapsed_s < TURN_DURATION_S {
                self.yaw_deg = tween.value(TURN_DURATION_S);
                self.turn = Some(tween);
                return;
            }
            // Carry the left-over time into the next queued turn so queued turns keep pace.
            remaining_s = tween.elapsed_s - TURN_DURATION_S;
            self.view_steps = self.view_steps.rem_euclid(4);
            self.yaw_deg = self.target_yaw_deg();
            self.turn = None;
            if let Some(next) = self.queued_turns.pop_front() {
                self.start_turn(next);
            }
        }
    }

    fn update_zoom(&mut self, dt_s: f32) {
        let Some(mut zoom) = self.zoom_tween else {
            return;
        };
        zoom.tween.elapsed_s += dt_s;
        let done = zoom.tween.elapsed_s >= ZOOM_DURATION_S;
        let new_zoom = if done {
            zoom.tween.to
        } else {
            zoom.tween.value(ZOOM_DURATION_S)
        };
        // Scaling the focus→pivot offset by old/new zoom keeps the focus at the same screen point.
        self.pivot = zoom.focus + (self.pivot - zoom.focus) * (self.zoom / new_zoom);
        self.zoom = new_zoom;
        self.zoom_tween = (!done).then_some(zoom);
    }

    fn clamp_pivot(&mut self) {
        let home = self.projection.home_pivot;
        let extent = Vec2::splat(OPS_AREA_HALF_EXTENT_VOXELS);
        self.pivot = self.pivot.clamp(home - extent, home + extent);
    }

    /// View index 0–3 of the current turn target, or of the settled view.
    pub fn view_index(&self) -> u8 {
        // rem_euclid(4) is always in 0..4.
        u8::try_from(self.view_steps.rem_euclid(4)).unwrap_or_default()
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    #[cfg(test)]
    fn is_turning(&self) -> bool {
        self.turn.is_some()
    }

    #[cfg(test)]
    fn is_zooming(&self) -> bool {
        self.zoom_tween.is_some()
    }

    #[cfg(test)]
    fn yaw_deg(&self) -> f32 {
        self.yaw_deg
    }

    #[cfg(test)]
    fn pivot(&self) -> Vec2 {
        self.pivot
    }

    #[cfg(test)]
    fn projection(&self) -> &Projection {
        &self.projection
    }

    /// Uniform layout scale for the current window: `min(w / 1920, h / 1080)`.
    pub fn layout_scale(&self) -> f32 {
        let scale = self.viewport_px / self.projection.reference_size;
        scale.x.min(scale.y)
    }

    /// Snap translation to whole pixels at integer effective zoom, so voxel edges stay crisp while
    /// panning (doc 02; doc 05 "Pixel crispness"). Off while a turn or zoom animates.
    fn pixel_snap_active(&self) -> bool {
        if self.turn.is_some() || self.zoom_tween.is_some() {
            return false;
        }
        let effective = self.zoom * self.layout_scale();
        effective >= 1.0 - SNAP_EPSILON && (effective - effective.round()).abs() < SNAP_EPSILON
    }

    /// The current world ↔ screen mapping.
    pub fn transform(&self) -> ViewTransform {
        let projection = &self.projection;
        let (right, up_ground) = basis(self.yaw_deg);
        let layout = self.layout_scale();
        let mut transform = ViewTransform {
            viewport_px: self.viewport_px,
            pivot: self.pivot,
            right,
            up_ground,
            sin_pitch: projection.sin_pitch,
            cos_pitch: projection.cos_pitch,
            z_scale: projection.z_scale,
            px_per_unit: projection.px_per_unit * self.zoom * layout,
            layout_scale: layout,
            anchor_px: self.viewport_px * 0.5 + projection.anchor_offset_ref_px * layout,
        };
        if self.pixel_snap_active() {
            let origin = transform.world_to_screen(Vec3::ZERO);
            transform.anchor_px += origin.round() - origin;
        }
        transform
    }
}

/// A frozen world ↔ screen mapping. Screen coordinates are pixels from the top-left corner, y down.
#[derive(Clone, Copy, Debug)]
pub struct ViewTransform {
    pub viewport_px: Vec2,
    pub pivot: Vec2,
    /// Screen-right direction on the ground.
    pub right: Vec2,
    /// Screen-up direction on the ground (away from the viewer).
    pub up_ground: Vec2,
    pub sin_pitch: f32,
    pub cos_pitch: f32,
    pub z_scale: f32,
    /// Window pixels per reference pixel: `min(w / 1920, h / 1080)`.
    pub layout_scale: f32,
    /// Pixels per world unit along screen x, including zoom and layout scale.
    pub px_per_unit: f32,
    /// Screen position of the pivot.
    pub anchor_px: Vec2,
}

impl ViewTransform {
    /// Projects a world point to screen pixels.
    pub fn world_to_screen(&self, world: Vec3) -> Vec2 {
        let d = world.truncate() - self.pivot;
        let right = d.dot(self.right);
        let up = self.sin_pitch * d.dot(self.up_ground) + self.cos_pitch * self.z_scale * world.z;
        self.anchor_px + Vec2::new(right, -up) * self.px_per_unit
    }

    /// The point on the ops-grid plane `z = 0` under a screen position.
    pub fn screen_to_ground(&self, screen_px: Vec2) -> Vec2 {
        let offset = (screen_px - self.anchor_px) / self.px_per_unit;
        self.pivot + self.right * offset.x - self.up_ground * (offset.y / self.sin_pitch)
    }

    /// World → clip-space matrix (wgpu conventions: NDC y up, depth 0..1, nearer is smaller).
    /// The z scale lives here, so meshes keep integer voxel coordinates (doc 05).
    pub fn view_proj(&self) -> Mat4 {
        let Vec2 {
            x: width,
            y: height,
        } = self.viewport_px;
        let scale = self.px_per_unit;
        let (right, up, pivot) = (self.right, self.up_ground, self.pivot);
        let up_z = self.cos_pitch * self.z_scale;
        // ndc_x = 2·sx/w − 1, with sx = anchor_x + k·R·(p − pivot).
        let row_x = Vec4::new(
            2.0 * scale * right.x / width,
            2.0 * scale * right.y / width,
            0.0,
            2.0 * (self.anchor_px.x - scale * right.dot(pivot)) / width - 1.0,
        );
        // ndc_y = 1 − 2·sy/h, with sy = anchor_y − k·(sin θ·H·(p − pivot) + cos θ·s_z·z).
        let row_y = Vec4::new(
            2.0 * scale * self.sin_pitch * up.x / height,
            2.0 * scale * self.sin_pitch * up.y / height,
            2.0 * scale * up_z / height,
            1.0 - 2.0 * (self.anchor_px.y + scale * self.sin_pitch * up.dot(pivot)) / height,
        );
        // Depth grows into the screen along (cos θ·H, −sin θ·s_z), centred on the pivot.
        let depth = 0.5 / DEPTH_HALF_RANGE_VOXELS;
        let row_z = Vec4::new(
            self.cos_pitch * up.x * depth,
            self.cos_pitch * up.y * depth,
            -self.sin_pitch * self.z_scale * depth,
            0.5 - self.cos_pitch * up.dot(pivot) * depth,
        );
        Mat4::from_cols(row_x, row_y, row_z, Vec4::W).transpose()
    }

    /// Ground distance from the pivot to the nearest screen edge. The grid reaches `alpha_far`
    /// there (doc 01 §4).
    pub fn screen_edge_distance_voxels(&self) -> f32 {
        let a = self.anchor_px;
        let v = self.viewport_px;
        let across = a.x.min(v.x - a.x) / self.px_per_unit;
        let along = a.y.min(v.y - a.y) / (self.px_per_unit * self.sin_pitch);
        across.min(along).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_asset_root, load_scene_config, scene_config_path};

    const REFERENCE: Vec2 = Vec2::new(1920.0, 1080.0);

    fn config() -> SceneConfig {
        load_scene_config(&scene_config_path(&default_asset_root())).unwrap()
    }

    fn camera_at(viewport: Vec2) -> Camera {
        Camera::new(Projection::from_config(&config()), viewport)
    }

    fn camera() -> Camera {
        camera_at(REFERENCE)
    }

    /// Doc 01 §1, view 0, zoom 1, 1920×1080.
    fn doc01_formula(p: Vec3) -> Vec2 {
        Vec2::new(
            1060.0 + 5.0 * (p.x - p.y),
            490.0 + 2.5 * (p.x + p.y) - 5.0 * p.z,
        )
    }

    fn via_matrix(t: &ViewTransform, p: Vec3) -> Vec2 {
        let clip = t.view_proj() * p.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        Vec2::new(
            (ndc.x + 1.0) * 0.5 * t.viewport_px.x,
            (1.0 - ndc.y) * 0.5 * t.viewport_px.y,
        )
    }

    fn assert_close(actual: Vec2, expected: Vec2, tolerance: f32, what: &str) {
        assert!(
            (actual - expected).abs().max_element() <= tolerance,
            "{what}: got {actual}, expected {expected}"
        );
    }

    const SAMPLE_POINTS: [Vec3; 7] = [
        Vec3::ZERO,
        Vec3::new(10.0, -5.0, 3.0),
        Vec3::new(-8.5, 9.5, 0.0),
        Vec3::new(-62.0, -42.0, 0.0),
        Vec3::new(40.0, 48.0, 15.0),
        Vec3::new(13.25, -7.5, 33.0),
        Vec3::new(-150.0, 120.0, 16.0),
    ];

    #[test]
    fn view0_matches_the_doc01_formula() {
        let t = camera().transform();
        for p in SAMPLE_POINTS {
            let expected = doc01_formula(p);
            assert_close(t.world_to_screen(p), expected, 0.01, "direct");
            assert_close(via_matrix(&t, p), expected, 0.01, "view_proj");
        }
    }

    #[test]
    fn pivot_sits_at_the_anchor_not_the_centre() {
        let t = camera().transform();
        assert_close(t.anchor_px, Vec2::new(970.0, 492.5), 1e-3, "anchor");
        assert_close(
            t.world_to_screen(Vec3::new(-8.5, 9.5, 0.0)),
            t.anchor_px,
            1e-3,
            "pivot",
        );
    }

    #[test]
    fn screen_to_ground_round_trips() {
        let cam = camera();
        for view in 0..4 {
            let mut cam = cam.clone();
            cam.set_view(view);
            let t = cam.transform();
            let inverse = t.view_proj().inverse();
            for s in [
                Vec2::ZERO,
                Vec2::new(1919.0, 1079.0),
                Vec2::new(960.0, 540.0),
                Vec2::new(123.4, 987.6),
            ] {
                let ground = t.screen_to_ground(s);
                assert_close(t.world_to_screen(ground.extend(0.0)), s, 1e-3, "round trip");
                // The GPU path: unproject two depths and intersect z = 0.
                let ndc = Vec2::new(2.0 * s.x / 1920.0 - 1.0, 1.0 - 2.0 * s.y / 1080.0);
                let a = inverse.project_point3(ndc.extend(0.0));
                let b = inverse.project_point3(ndc.extend(1.0));
                let hit = a.lerp(b, a.z / (a.z - b.z));
                assert_close(hit.truncate(), ground, 2e-3, "inverse view_proj");
            }
        }
    }

    #[test]
    fn view1_matches_the_reference_orientation() {
        let mut cam = camera();
        cam.set_view(1);
        let t = cam.transform();
        let pivot_x = t.anchor_px.x;
        // station-view-1.png: solar array on the right, asteroid field on the left.
        assert!(t.world_to_screen(Vec3::new(-42.5, -22.5, 0.0)).x > pivot_x + 100.0);
        assert!(t.world_to_screen(Vec3::new(54.0, 7.5, 0.0)).x < pivot_x - 100.0);
    }

    #[test]
    fn scene_config_ratios_are_consistent() {
        let config = config();
        let projection = Projection::from_config(&config);
        assert!(projection.consistency_problems(&config).is_empty());
    }

    #[test]
    fn true_cubes_change_only_the_height_scale() {
        let mut config = config();
        config.projection.true_cubes = true;
        let t = Camera::new(Projection::from_config(&config), REFERENCE).transform();
        let ground = Vec3::new(10.0, -5.0, 0.0);
        assert_close(
            t.world_to_screen(ground),
            doc01_formula(ground),
            0.01,
            "ground",
        );
        let up = t.world_to_screen(Vec3::Z) - t.world_to_screen(Vec3::ZERO);
        let expected = -5.0 * 2.0_f32.sqrt() * 30.0_f32.to_radians().cos();
        assert!((up.y - expected).abs() < 1e-3, "true cube height {}", up.y);
    }

    #[test]
    fn turn_takes_250_ms_with_ease_in_out() {
        let mut cam = camera();
        cam.rotate(Turn::Plus90);
        assert!(cam.is_turning());
        assert_eq!(cam.view_index(), 1);
        cam.update(0.125);
        assert!(
            (cam.yaw_deg() - 90.0).abs() < 1e-4,
            "halfway yaw {}",
            cam.yaw_deg()
        );
        cam.update(0.0625);
        assert!(cam.is_turning());
        cam.update(0.0625);
        assert!(!cam.is_turning());
        assert!((cam.yaw_deg() - 135.0).abs() < 1e-4);
    }

    #[test]
    fn snap_rotation_cycles_both_ways() {
        let mut cam = camera();
        for expected in [1, 2, 3, 0] {
            cam.rotate(Turn::Plus90);
            cam.update(TURN_DURATION_S);
            assert_eq!(cam.view_index(), expected);
            assert!(!cam.is_turning());
        }
        assert!((cam.yaw_deg() - 45.0).abs() < 1e-4);
        for expected in [3, 2, 1, 0] {
            cam.rotate(Turn::Minus90);
            cam.update(TURN_DURATION_S);
            assert_eq!(cam.view_index(), expected);
        }
        assert!((cam.yaw_deg() - 45.0).abs() < 1e-4);
    }

    #[test]
    fn yaw_is_continuous_across_view3_to_view0() {
        let mut cam = camera();
        cam.set_view(3);
        cam.rotate(Turn::Plus90);
        cam.update(0.125);
        // Halfway from 315° to 405°, not back through 180°.
        assert!(
            (cam.yaw_deg() - 360.0).abs() < 1e-3,
            "yaw {}",
            cam.yaw_deg()
        );
        cam.update(0.125);
        assert_eq!(cam.view_index(), 0);
        assert!((cam.yaw_deg() - 45.0).abs() < 1e-4);
    }

    #[test]
    fn input_during_a_turn_is_queued_and_applied_after_it() {
        let mut cam = camera();
        cam.rotate(Turn::Plus90);
        cam.update(0.0625);
        cam.rotate(Turn::Plus90);
        cam.rotate(Turn::Minus90);
        cam.rotate(Turn::Plus90);
        // The first turn is unaffected.
        assert_eq!(cam.view_index(), 1);
        cam.update(0.1875);
        // First turn done at exactly 250 ms; the second has started.
        assert_eq!(cam.view_index(), 2);
        assert!(cam.is_turning());
        assert!((cam.yaw_deg() - 135.0).abs() < 1e-4);
        cam.update(TURN_DURATION_S); // second done, Q starts
        assert_eq!(cam.view_index(), 1);
        cam.update(TURN_DURATION_S); // Q done, last E starts
        assert_eq!(cam.view_index(), 2);
        assert!(cam.is_turning());
        cam.update(TURN_DURATION_S);
        assert_eq!(cam.view_index(), 2);
        assert!(!cam.is_turning());
        assert!((cam.yaw_deg() - 225.0).abs() < 1e-4);
    }

    #[test]
    fn one_long_frame_runs_queued_turns_in_order() {
        let mut cam = camera();
        cam.rotate(Turn::Plus90);
        cam.rotate(Turn::Plus90);
        cam.update(0.375);
        assert_eq!(cam.view_index(), 2);
        assert!(
            (cam.yaw_deg() - 180.0).abs() < 1e-3,
            "yaw {}",
            cam.yaw_deg()
        );
        cam.update(1.0);
        assert!(!cam.is_turning());
        assert!((cam.yaw_deg() - 225.0).abs() < 1e-4);
    }

    #[test]
    fn zoom_steps_clamp_at_both_ends() {
        let mut cam = camera();
        let levels = cam.projection().zoom_levels().to_vec();
        assert!(cam.zoom_step(ZoomStep::Out, None));
        assert!(cam.zoom_step(ZoomStep::Out, None));
        assert!(!cam.zoom_step(ZoomStep::Out, None));
        cam.update(ZOOM_DURATION_S);
        assert!((cam.zoom() - levels[0]).abs() < 1e-6);
        for _ in 0..4 {
            assert!(cam.zoom_step(ZoomStep::In, None));
        }
        assert!(!cam.zoom_step(ZoomStep::In, None));
        cam.update(ZOOM_DURATION_S);
        assert!((cam.zoom() - levels[levels.len() - 1]).abs() < 1e-6);
        assert!(!cam.is_zooming());
    }

    #[test]
    fn zoom_eases_toward_the_cursor() {
        let mut cam = camera();
        cam.set_view(2);
        let cursor = Vec2::new(1500.0, 300.0);
        let focus = cam.transform().screen_to_ground(cursor);
        assert!(cam.zoom_step(ZoomStep::In, Some(cursor)));
        cam.update(0.075);
        assert!(
            cam.zoom() > 1.0 && cam.zoom() < 1.5,
            "mid zoom {}",
            cam.zoom()
        );
        assert_close(cam.transform().screen_to_ground(cursor), focus, 1e-3, "mid");
        cam.update(0.075);
        assert!((cam.zoom() - 1.5).abs() < 1e-6);
        assert_close(cam.transform().screen_to_ground(cursor), focus, 1e-3, "end");
    }

    #[test]
    fn pan_clamps_to_the_ops_area_bounds() {
        let home = Vec2::new(-8.5, 9.5);
        for delta in [
            Vec2::new(1e7, 0.0),
            Vec2::new(-1e7, 0.0),
            Vec2::new(0.0, 1e7),
            Vec2::new(0.0, -1e7),
            Vec2::new(1e7, 1e7),
        ] {
            let mut cam = camera();
            cam.pan_screen_px(delta);
            let offset = (cam.pivot() - home).abs();
            assert!(offset.max_element() <= OPS_AREA_HALF_EXTENT_VOXELS + 1e-3);
            assert!((offset.max_element() - OPS_AREA_HALF_EXTENT_VOXELS).abs() < 1e-3);
        }
        // Zooming toward a far cursor cannot escape the bounds either.
        let mut cam = camera();
        cam.pan_screen_px(Vec2::new(1e7, 0.0));
        cam.zoom_step(ZoomStep::In, Some(Vec2::new(1919.0, 540.0)));
        cam.update(ZOOM_DURATION_S);
        assert!((cam.pivot() - home).abs().max_element() <= OPS_AREA_HALF_EXTENT_VOXELS + 1e-3);
    }

    #[test]
    fn pan_directions_follow_the_screen() {
        let mut cam = camera();
        cam.set_view(3);
        cam.set_zoom_index(3); // 1.5: no pixel snapping, so moves are exact
        let before = cam.transform().world_to_screen(Vec3::ZERO);
        cam.pan_screen_px(Vec2::new(30.0, -20.0));
        // Panning right/up moves the scene left/down by the same number of pixels.
        let after = cam.transform().world_to_screen(Vec3::ZERO);
        assert_close(after - before, Vec2::new(-30.0, 20.0), 1e-3, "pan");
    }

    #[test]
    fn drag_keeps_the_grabbed_point_under_the_cursor() {
        let mut cam = camera();
        cam.set_view(1);
        cam.set_zoom_index(1); // 0.75
        let grab = Vec2::new(700.0, 400.0);
        let point = cam.transform().screen_to_ground(grab);
        let delta = Vec2::new(-55.5, 31.25);
        cam.drag_px(delta);
        assert_close(
            cam.transform().screen_to_ground(grab + delta),
            point,
            1e-3,
            "drag",
        );
    }

    #[test]
    fn translation_snaps_to_whole_pixels_at_integer_zoom() {
        let mut cam = camera();
        cam.set_zoom_index(3); // 1.5
        cam.pan_screen_px(Vec2::new(0.3, 0.2));
        let origin = cam.transform().world_to_screen(Vec3::ZERO);
        assert!(
            (origin - origin.round()).abs().max_element() > 0.05,
            "zoom 1.5 is not snapped"
        );

        cam.set_zoom_index(2); // 1.0
        let origin = cam.transform().world_to_screen(Vec3::ZERO);
        assert_close(origin, origin.round(), 1e-3, "zoom 1 snaps");

        // At 4K, zoom 0.5 is one screen pixel per reference pixel: snapped too.
        let mut cam = camera_at(REFERENCE * 2.0);
        cam.set_zoom_index(0);
        cam.pan_screen_px(Vec2::new(0.3, 0.7));
        let origin = cam.transform().world_to_screen(Vec3::ZERO);
        assert_close(origin, origin.round(), 1e-3, "4K zoom 0.5 snaps");

        // No snapping mid-turn.
        let mut cam = camera();
        cam.pan_screen_px(Vec2::new(0.3, 0.2));
        cam.rotate(Turn::Plus90);
        cam.update(0.1);
        assert!(!cam.pixel_snap_active());
    }

    #[test]
    fn home_resets_view_zoom_and_pivot() {
        let mut cam = camera();
        cam.rotate(Turn::Plus90);
        cam.rotate(Turn::Plus90);
        cam.zoom_step(ZoomStep::Out, Some(Vec2::new(10.0, 10.0)));
        cam.pan_screen_px(Vec2::new(400.0, 250.0));
        cam.update(0.1);
        cam.reset_home();
        assert_eq!(cam.view_index(), 0);
        assert!(!cam.is_turning() && !cam.is_zooming());
        assert!((cam.yaw_deg() - 45.0).abs() < 1e-6);
        assert!((cam.zoom() - 1.0).abs() < 1e-6);
        assert_close(cam.pivot(), Vec2::new(-8.5, 9.5), 1e-6, "pivot");
        // Nothing queued survives Home.
        cam.update(1.0);
        assert_eq!(cam.view_index(), 0);
    }

    #[test]
    fn layout_scales_uniformly_with_the_window() {
        let cam = camera_at(Vec2::new(3840.0, 2160.0));
        let t = cam.transform();
        assert!((cam.layout_scale() - 2.0).abs() < 1e-6);
        assert_close(t.anchor_px, Vec2::new(1940.0, 985.0), 1e-3, "4K anchor");
        // A wide window scales by height.
        assert!((camera_at(Vec2::new(2560.0, 1080.0)).layout_scale() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn screen_edge_distance_at_home() {
        let t = camera().transform();
        // Nearest edge is the right one: 950 px at 5√2 px per voxel.
        assert!((t.screen_edge_distance_voxels() - 950.0 / (5.0 * 2.0_f32.sqrt())).abs() < 1e-2);
    }
}
