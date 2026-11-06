//! Infinite ops grid pass (phase-1 doc 01 §4, doc 05 "Grid"). The shader is `shaders/grid.wgsl`.
//!
//! # Fade (D-059)
//!
//! Line alpha fades linearly from `alpha_near` at `near_radius_voxels` to `alpha_far` at
//! `far_radius_voxels`, measured on the ground from the camera pivot, and stays at `alpha_far`
//! beyond (never zero). D-059 makes the reference render the target here, as an exception to
//! "the spec wins". The mockup behind `ops-grid-scene-no-hud.png` draws every line at `alpha_far`
//! and stacks near segments over it in six steps, clipped to a square around the origin. The
//! shipped values reproduce the result: `alpha_near` is the stacked effective alpha
//! (`far + near·(1 − far)` of the mockup's 0.22 and 0.40, so 0.26 and 0.45), and 75 → 114 voxels
//! is the least-squares fit of a linear ramp to the line contrast measured in the reference by
//! distance from the pivot. The radii are world distances: at zoom 1 the fade ends inside the
//! screen edge, as in the reference; zooming out shows more `alpha_far` grid, zooming in fills
//! the screen with near-alpha lines.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::context::TargetFormat;
use crate::camera::ViewTransform;
use crate::config::{GridLineStyle, OpsGridConfig};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GridUniforms {
    inv_view_proj: [[f32; 4]; 4],
    viewport: [f32; 4],
    pivot_fade: [f32; 4],
    minor_color: [f32; 4],
    minor_params: [f32; 4],
    minor_offset: [f32; 4],
    major_color: [f32; 4],
    major_params: [f32; 4],
    major_offset: [f32; 4],
}

impl GridUniforms {
    /// Uniforms for one frame (fade: see the module docs).
    pub fn new(grid: &OpsGridConfig, view: &ViewTransform) -> Self {
        // SceneConfig::validate guarantees far > near.
        let (near, far) = (grid.near_radius_voxels, grid.far_radius_voxels);
        // Widths are 1080p pixels and scale with the window like every spec value (phase-1
        // README, ground rule 2), but not with zoom: the lines stay hairlines.
        let style = |s: &GridLineStyle, spacing: f32, offset: [f32; 2]| {
            (
                s.color.to_vec4(s.width_px * view.layout_scale),
                [s.alpha_near, s.alpha_far, spacing, 0.0],
                [offset[0], offset[1], 0.0, 0.0],
            )
        };
        let (minor_color, minor_params, minor_offset) =
            style(&grid.minor, grid.minor_spacing, grid.minor_offset_xy);
        let (major_color, major_params, major_offset) =
            style(&grid.major, grid.major_spacing, grid.major_offset_xy);
        Self {
            inv_view_proj: view.view_proj().inverse().to_cols_array_2d(),
            viewport: [view.viewport_px.x, view.viewport_px.y, 0.0, 0.0],
            pivot_fade: [view.pivot.x, view.pivot.y, near, far],
            minor_color,
            minor_params,
            minor_offset,
            major_color,
            major_params,
            major_offset,
        }
    }

    #[cfg(test)]
    fn fade_radii(&self) -> (f32, f32) {
        (self.pivot_fade[2], self.pivot_fade[3])
    }
}

pub struct GridPass {
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl GridPass {
    pub fn new(device: &wgpu::Device, target: TargetFormat) -> Self {
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid uniforms"),
            contents: bytemuck::bytes_of(&GridUniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid"),
            entries: &[super::uniform_entry(
                wgpu::ShaderStages::FRAGMENT,
                size_of::<GridUniforms>(),
            )],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grid"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid.wgsl"),
            source: wgpu::ShaderSource::Wgsl(super::GRID_WGSL.into()),
        });
        // Drawn over the background with premultiplied alpha, testing nothing and writing no
        // depth. The opaque voxels come after it and cover it, so the grid always lies behind and
        // under the station, as in the reference (doc 05 frame order 2–3).
        let pipeline = super::fullscreen_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "fs_grid",
            target,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            "grid",
        );
        Self {
            uniform_buffer,
            bind_group,
            pipeline,
        }
    }

    pub fn prepare(&self, queue: &wgpu::Queue, uniforms: &GridUniforms) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.pipeline);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::camera::{Camera, Projection};
    use crate::config::{default_asset_root, load_scene_config, scene_config_path};

    /// The fade radii are world distances from the config, the same at every zoom and window
    /// size (D-059).
    #[test]
    fn fade_radii_come_from_the_config() {
        let config = load_scene_config(&scene_config_path(&default_asset_root())).unwrap();
        let grid = &config.ops_grid;
        let projection = Projection::from_config(&config);
        for size in [Vec2::new(1920.0, 1080.0), Vec2::new(40.0, 1080.0)] {
            let mut camera = Camera::new(projection.clone(), size);
            for zoom_index in 0..config.camera.zoom_levels.len() {
                camera.set_zoom_index(zoom_index);
                let (near, far) = GridUniforms::new(grid, &camera.transform()).fade_radii();
                assert!((near - grid.near_radius_voxels).abs() < 1e-6);
                assert!((far - grid.far_radius_voxels).abs() < 1e-6);
            }
        }
    }

    /// At zoom 1 the whole screen edge shows `alpha_far`, as in the reference render (doc 01 §4:
    /// "`alpha_far` at the screen edge and beyond").
    #[test]
    fn fade_ends_inside_the_screen_at_zoom_1() {
        let config = load_scene_config(&scene_config_path(&default_asset_root())).unwrap();
        let view =
            Camera::new(Projection::from_config(&config), Vec2::new(1920.0, 1080.0)).transform();
        // The nearest edge is the right one, 950 px from the pivot anchor.
        let edge_voxels = (1920.0 - view.anchor_px.x) / view.px_per_unit;
        let far = config.ops_grid.far_radius_voxels;
        assert!(far < edge_voxels, "far {far}, edge {edge_voxels}");
    }

    #[test]
    fn line_widths_scale_with_the_window_not_the_zoom() {
        let config = load_scene_config(&scene_config_path(&default_asset_root())).unwrap();
        let projection = Projection::from_config(&config);
        let width = |size: Vec2, zoom_index: usize| {
            let mut camera = Camera::new(projection.clone(), size);
            camera.set_zoom_index(zoom_index);
            let uniforms = GridUniforms::new(&config.ops_grid, &camera.transform());
            (uniforms.minor_color[3], uniforms.major_color[3])
        };
        let (minor, major) = width(Vec2::new(1920.0, 1080.0), 2);
        assert!((minor - 0.9).abs() < 1e-6 && (major - 1.2).abs() < 1e-6);
        let (minor_far, major_far) = width(Vec2::new(1920.0, 1080.0), 0);
        assert!((minor_far - minor).abs() < 1e-6 && (major_far - major).abs() < 1e-6);
        let (minor_4k, major_4k) = width(Vec2::new(3840.0, 2160.0), 2);
        assert!((minor_4k - 1.8).abs() < 1e-6 && (major_4k - 2.4).abs() < 1e-6);
    }

    #[test]
    fn uniform_layout_matches_the_shader() {
        // mat4x4 + 8 × vec4, no padding.
        assert_eq!(size_of::<GridUniforms>(), 64 + 8 * 16);
    }
}
