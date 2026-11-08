//! Hover picking: which station module is under the cursor (phase-1 doc 06, M3: "hover a module
//! → highlight + ELEV tag").
//!
//! The camera is orthographic, so every screen pixel looks along the same world direction. The
//! pixel's view ray goes through the point on the ops grid under it
//! ([`ViewTransform::screen_to_ground`]) and runs along `(H, −tan θ / s_z)`, where `H` is the
//! screen-up direction on the ground, `θ` the pitch and `s_z` the z scale: moving that way changes
//! neither screen coordinate (`camera` module docs). Each module's dense chunk is walked cell by
//! cell along the ray (Amanatides–Woo), from where the ray enters its bounds; the module whose
//! first solid cell is nearest the camera wins. This is exact, needs no GPU read-back, and costs
//! at most a few hundred cell steps per module, once per frame.

use glam::{Vec2, Vec3};

use crate::assets::voxel::{Chunk, StationScene};
use crate::camera::ViewTransform;

/// Upper bound on cell steps through one chunk, a guard against a degenerate ray.
const MAX_STEPS: usize = 4096;

/// The module under a screen position, if any: its index in `scene.modules`.
pub fn module_at(scene: &StationScene, view: &ViewTransform, cursor_px: Vec2) -> Option<usize> {
    let (origin, direction) = view_ray(view, cursor_px);
    scene
        .modules
        .iter()
        .enumerate()
        .filter_map(|(index, module)| {
            first_hit(&module.chunk, origin, direction).map(|t| (t, index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index)
}

/// The view ray of a screen position: a point on it (on the ops grid) and its direction into the
/// screen. Smaller ray parameters are nearer the camera.
pub fn view_ray(view: &ViewTransform, screen_px: Vec2) -> (Vec3, Vec3) {
    let ground = view.screen_to_ground(screen_px);
    let tan_pitch = view.sin_pitch / view.cos_pitch;
    let direction = Vec3::new(
        view.up_ground.x,
        view.up_ground.y,
        -tan_pitch / view.z_scale,
    );
    (ground.extend(0.0), direction)
}

/// The ray parameter where the ray enters the first solid cell of `chunk`, if it meets one.
pub fn first_hit(chunk: &Chunk, origin: Vec3, direction: Vec3) -> Option<f32> {
    let lo = Vec3::from(chunk.origin().map(|v| v as f32));
    let size = chunk.size();
    let hi = lo + Vec3::from(size.map(|s| s as f32));
    let (t_enter, t_exit) = ray_box(origin, direction, lo, hi)?;

    // The cell containing the entry point. On the entry face the coordinate may round to the far
    // side, so clamp into the chunk.
    let entry = origin + direction * t_enter - lo;
    let mut cell = [0_usize; 3];
    let mut step = [0_isize; 3];
    let mut t_next = [f32::INFINITY; 3];
    let mut t_delta = [f32::INFINITY; 3];
    for axis in 0..3 {
        let last = size[axis].saturating_sub(1);
        cell[axis] = whole_cell(entry[axis]).min(last);
        let d = direction[axis];
        if d != 0.0 {
            step[axis] = if d > 0.0 { 1 } else { -1 };
            t_delta[axis] = 1.0 / d.abs();
            let boundary = lo[axis] + cell[axis] as f32 + if d > 0.0 { 1.0 } else { 0.0 };
            t_next[axis] = (boundary - origin[axis]) / d;
        }
    }

    let mut t = t_enter;
    for _ in 0..MAX_STEPS {
        if chunk.get(cell) != 0 {
            return Some(t);
        }
        // Step across the nearest cell boundary.
        let axis = (0..3)
            .min_by(|&a, &b| t_next[a].total_cmp(&t_next[b]))
            .unwrap_or(0);
        t = t_next[axis];
        if t > t_exit {
            return None;
        }
        cell[axis] = cell[axis].checked_add_signed(step[axis])?;
        if cell[axis] >= size[axis] {
            return None;
        }
        t_next[axis] += t_delta[axis];
    }
    None
}

/// Slab test of a ray against the box `[lo, hi)` over all ray parameters. Returns the entry and
/// exit parameters.
pub fn ray_box(origin: Vec3, direction: Vec3, lo: Vec3, hi: Vec3) -> Option<(f32, f32)> {
    let mut t_enter = f32::NEG_INFINITY;
    let mut t_exit = f32::INFINITY;
    for axis in 0..3 {
        let (o, d) = (origin[axis], direction[axis]);
        if d == 0.0 {
            if o < lo[axis] || o >= hi[axis] {
                return None;
            }
        } else {
            let a = (lo[axis] - o) / d;
            let b = (hi[axis] - o) / d;
            t_enter = t_enter.max(a.min(b));
            t_exit = t_exit.min(a.max(b));
        }
    }
    (t_enter < t_exit).then_some((t_enter, t_exit))
}

/// `floor(value)` as a cell index, 0 for negative values.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to non-negative; callers clamp to the chunk size"
)]
fn whole_cell(value: f32) -> usize {
    value.max(0.0).floor() as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::voxel::load_station_scene;
    use crate::camera::{Camera, Projection, Turn};
    use crate::config::{
        load_scene_config, phase1_fixture_root, scene_config_path, station_scene_path,
    };

    fn station() -> StationScene {
        load_station_scene(&station_scene_path(&phase1_fixture_root())).unwrap()
    }

    fn camera() -> Camera {
        let config = load_scene_config(&scene_config_path(&phase1_fixture_root())).unwrap();
        Camera::new(Projection::from_config(&config), Vec2::new(1920.0, 1080.0))
    }

    fn index_of(scene: &StationScene, id: &str) -> usize {
        scene.modules.iter().position(|m| m.id == id).unwrap()
    }

    /// The reference answer: every voxel of every module as its own box.
    /// Each module's bounds and its voxels as unit boxes (minimum corners), listed once.
    fn voxel_boxes(scene: &StationScene) -> Vec<((Vec3, Vec3), Vec<Vec3>)> {
        scene
            .modules
            .iter()
            .map(|module| {
                let lo = Vec3::from(module.chunk.origin().map(|v| v as f32));
                let hi = lo + Vec3::from(module.chunk.size().map(|s| s as f32));
                let voxels = module
                    .chunk
                    .voxels()
                    .map(|(p, _)| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
                    .collect();
                ((lo, hi), voxels)
            })
            .collect()
    }

    fn brute_force(
        boxes: &[((Vec3, Vec3), Vec<Vec3>)],
        origin: Vec3,
        direction: Vec3,
    ) -> Option<(f32, usize)> {
        let mut best: Option<(f32, usize)> = None;
        for (index, ((lo, hi), voxels)) in boxes.iter().enumerate() {
            // Only a shortcut: a ray that misses the bounds misses every voxel inside them.
            if ray_box(origin, direction, *lo, *hi).is_none() {
                continue;
            }
            for &lo in voxels {
                if let Some((t, _)) = ray_box(origin, direction, lo, lo + Vec3::ONE)
                    && best.is_none_or(|(b, _)| t < b)
                {
                    best = Some((t, index));
                }
            }
        }
        best
    }

    /// The ray really is the pixel's line of sight: every point on it projects to the pixel.
    #[test]
    fn view_ray_projects_to_its_pixel() {
        let mut camera = camera();
        for view in 0..4 {
            camera.set_view(view);
            let t = camera.transform();
            let pixel = Vec2::new(812.3, 377.9);
            let (origin, direction) = view_ray(&t, pixel);
            for s in [-40.0, -3.5, 0.0, 12.0] {
                let p = t.world_to_screen(origin + direction * s);
                assert!((p - pixel).length() < 1e-2, "view {view}, s {s}: {p}");
            }
        }
    }

    /// The cell walk agrees with testing every voxel, on a grid of pixels over the station in
    /// every view and mid-turn (where the ray runs along a world axis at one instant).
    #[test]
    fn picking_matches_a_brute_force_search() {
        let scene = station();
        let boxes = voxel_boxes(&scene);
        let mut camera = camera();
        let mut hits = 0;
        for (view, mid_turn) in [(0, false), (1, false), (2, false), (3, true)] {
            camera.set_view(view);
            if mid_turn {
                camera.rotate(Turn::Plus90);
                camera.update(0.125);
            }
            let t = camera.transform();
            // Odd offsets keep rays off exact voxel edges, where both answers are equally right.
            for i in 0..30 {
                for j in 0..18 {
                    let pixel = Vec2::new(560.37 + 28.13 * i as f32, 230.61 + 28.07 * j as f32);
                    let (origin, direction) = view_ray(&t, pixel);
                    let expected = brute_force(&boxes, origin, direction).map(|(_, m)| m);
                    assert_eq!(
                        module_at(&scene, &t, pixel),
                        expected,
                        "view {view} mid-turn {mid_turn} at {pixel}"
                    );
                    hits += usize::from(expected.is_some());
                }
            }
        }
        assert!(hits > 500, "only {hits} pixels hit the station");
    }

    /// In view 0, the top of FA-1's container rack is FA-1; empty space and the grid under
    /// nothing pick nothing.
    #[test]
    fn picks_lifted_modules_and_misses_empty_space() {
        let scene = station();
        let t = camera().transform();
        let fa1 = index_of(&scene, "factory_fa1");
        // The highest voxel of FA-1, seen from above, is FA-1.
        let top = scene.modules[fa1]
            .chunk
            .voxels()
            .max_by_key(|(p, _)| p[2])
            .map(|(p, _)| p)
            .unwrap();
        let top_centre = Vec3::new(
            top[0] as f32 + 0.5,
            top[1] as f32 + 0.5,
            top[2] as f32 + 1.0,
        );
        assert_eq!(
            module_at(&scene, &t, t.world_to_screen(top_centre)),
            Some(fa1)
        );
        assert_eq!(module_at(&scene, &t, Vec2::new(40.0, 1040.0)), None);
        assert_eq!(
            module_at(&scene, &t, t.world_to_screen(Vec3::new(-120.0, 60.0, 0.0))),
            None
        );
    }
}
