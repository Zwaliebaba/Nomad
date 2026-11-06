//! Station geometry on the ship plane: obstacles and the launch point (D-105; S3 plan §4.3,
//! §4.4).
//!
//! Station modules are not simulated yet (they arrive with S4), so the geometry comes from
//! the rules' mother-station layout: the phase-1 station's module bounds in grid voxels
//! (D-003, D-067). Two derived steps place it in the system:
//!
//! - **Grid to system:** 1 voxel ≈ 35 m and grid north is system north, at azimuth 225° in
//!   grid xy (doc 10 §8, D-046). Grid east is azimuth 315°, so a grid point `g` is
//!   `(g·east, g·north)` in the system frame, relative to the site (the grid origin is the
//!   core, phase 1).
//! - **Obstacles:** a module whose height range contains the ship plane (z = 16) is a
//!   circular obstacle with its footprint radius, the circle around its xy bounds (D-105).
//!   With the phase-1 station that is the core spindle, the four pylons and FA-1, as the S3
//!   plan expects; the deck, the solar array and the dry dock are lower.
//!
//! Only mother stations have this layout. Other holdings have no modules modelled yet, so
//! they have no obstacles; they launch from the same offset.

use nomad_types::PositionMetres;

use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::rules::{ModuleShape, StationLayout};
use crate::state::UniverseState;

/// A circular obstacle on the ship plane, in the system frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    /// Its centre.
    pub centre: Vec2,
    /// Its footprint radius.
    pub radius_m: f64,
}

/// A point in grid voxels relative to the core, as a displacement in the system frame.
pub(crate) fn grid_to_system(layout: &StationLayout, grid: [f64; 2]) -> Vec2 {
    let azimuth = layout.grid_north_azimuth_deg.to_radians();
    let north = Vec2::from_angle(azimuth);
    let east = Vec2::new(-north.y, north.x);
    let g = Vec2::new(grid[0], grid[1]);
    Vec2::new(g.dot(east), g.dot(north)) * layout.voxel_m
}

/// Whether a module's height range crosses the ship plane (D-105).
pub(crate) fn crosses_ship_plane(layout: &StationLayout, module: &ModuleShape) -> bool {
    (module.min_voxels[2]..=module.max_voxels[2]).contains(&layout.ship_plane_z_voxels)
}

/// The obstacles of a mother station whose core is at `site`.
pub(crate) fn layout_obstacles(layout: &StationLayout, site: PositionMetres) -> Vec<Obstacle> {
    layout
        .modules
        .iter()
        .filter(|module| crosses_ship_plane(layout, module))
        .map(|module| {
            let axis = |i: usize| {
                let (lo, hi) = (
                    f64::from(module.min_voxels[i]),
                    f64::from(module.max_voxels[i]),
                );
                // Voxel i covers [i - 0.5, i + 0.5]; the phase-1 core is centred on 0.
                (f64::midpoint(lo, hi), (hi - lo + 1.0) * 0.5)
            };
            let ((cx, hx), (cy, hy)) = (axis(0), axis(1));
            Obstacle {
                centre: Vec2::from(site) + grid_to_system(layout, [cx, cy]),
                radius_m: libm::sqrt(hx * hx + hy * hy) * layout.voxel_m,
            }
        })
        .collect()
}

/// Where ships launched from `holding` appear: its site plus the layout's launch offset.
pub(crate) fn launch_point(state: &UniverseState, holding: HoldingId) -> Option<PositionMetres> {
    let site = state.galaxy.site(state.holdings.get(&holding)?.site)?;
    let layout = &state.rules.mother_station;
    Some((Vec2::from(site.position) + grid_to_system(layout, layout.launch_point_voxels)).into())
}

/// Every obstacle in `system`, in holding order.
pub(crate) fn obstacles_in(state: &UniverseState, system: SystemId) -> Vec<Obstacle> {
    let layout = &state.rules.mother_station;
    state
        .holdings
        .values()
        .filter(|h| h.mother_station && h.core_complete)
        .filter_map(|h| state.galaxy.site(h.site))
        .filter(|site| site.system == system)
        .flat_map(|site| layout_obstacles(layout, site.position))
        .collect()
}

impl UniverseState {
    /// The obstacles on the ship plane in `system`: station modules that cross it (D-105).
    pub fn obstacles(&self, system: SystemId) -> Vec<Obstacle> {
        obstacles_in(self, system)
    }

    /// Where ships launched from `holding` appear.
    pub fn launch_point(&self, holding: HoldingId) -> Option<PositionMetres> {
        launch_point(self, holding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rules;

    #[test]
    fn grid_north_is_system_north() {
        let layout = Rules::embedded().mother_station;
        // Azimuth 225° in grid xy is north, 315° is east (D-046).
        let north = grid_to_system(&layout, [-1.0, -1.0]);
        assert!(north.x.abs() < 1e-9 && north.y > 0.0);
        let east = grid_to_system(&layout, [1.0, -1.0]);
        assert!(east.x > 0.0 && east.y.abs() < 1e-9);
        // One voxel is 35 m.
        let unit = grid_to_system(&layout, [1.0, 0.0]);
        assert!((unit.length() - 35.0).abs() < 1e-9);
    }

    #[test]
    fn phase1_station_obstacles_are_the_core_pylons_and_fa1() {
        let layout = Rules::embedded().mother_station;
        let crossing: Vec<&str> = layout
            .modules
            .iter()
            .filter(|m| crosses_ship_plane(&layout, m))
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(
            crossing,
            [
                "core_spindle",
                "pylon_n",
                "pylon_e",
                "pylon_s",
                "pylon_w",
                "factory_fa1"
            ]
        );
        let obstacles = layout_obstacles(&layout, PositionMetres::ORIGIN);
        // The core spindle's 23 × 23 voxel footprint: radius 11.5 × √2 voxels ≈ 569 m.
        assert!(obstacles[0].centre.length() < 1e-9);
        assert!((obstacles[0].radius_m - 11.5 * 2_f64.sqrt() * 35.0).abs() < 1e-9);
        // Pylon N is due north of the dome centre, about 26 voxels out (D-046).
        let pylon_n = obstacles[1].centre;
        let dome_centre = grid_to_system(&layout, [-8.6, 9.4]);
        let rel = pylon_n - dome_centre;
        assert!(rel.x.abs() < 1.0 && rel.y > 25.0 * 35.0);
    }

    #[test]
    fn the_launch_point_is_clear_of_every_obstacle() {
        let layout = Rules::embedded().mother_station;
        let launch = grid_to_system(&layout, layout.launch_point_voxels);
        for obstacle in layout_obstacles(&layout, PositionMetres::ORIGIN) {
            // Clear of the footprint by more than a 3 × 2 formation's half-diagonal.
            assert!((launch - obstacle.centre).length() > obstacle.radius_m + 200.0);
        }
    }
}
