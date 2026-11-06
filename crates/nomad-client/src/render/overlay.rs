//! World-space overlays (phase-1 doc 05 `render/overlay.rs`, frame order step 5): the elevation
//! cues of doc 01 §7. The shader is `shaders/overlay.wgsl`.
//!
//! For every module with `elevation_tier > 0` (FA-1 at +8 and FB-2 at +4 in phase 1):
//!
//! - a **footprint** on the ops grid: the module's bounds projected straight down to `z = 0`, a
//!   dashed outline (`#38d6ff`, alpha 0.7) over a fill (alpha 0.06);
//! - dashed **stalks** from the three visible bottom corners of the bounds (at `bounds.min.z`, doc
//!   03) down to the grid.
//!
//! The overlays are depth-tested against the voxels but write no depth, so the module and anything
//! else in front hide them, as doc 05 asks. (The mockup draws them on top of everything, as the
//! reference render with the HUD shows; phase-1 doc 07 item 21.) They are drawn whether or not
//! the HUD is shown (doc 07 item 20).
//!
//! # Which corners are visible
//!
//! The camera looks along `H`, the screen-up direction on the ground, so the bottom corner of the
//! bounds farthest along `H` sits behind the module and the other three are visible. The geometry
//! is rebuilt on the CPU every frame (a few dozen vertices), so this follows the view, including
//! during the 250 ms turn. Halfway through a turn the two back corners swap; around that instant
//! (within [`CORNER_CROSSFADE_DEG`] of it) the two candidates cross-fade instead of popping.

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use wgpu::util::DeviceExt;

use super::context::TargetFormat;
use crate::assets::voxel::StationScene;
use crate::camera::ViewTransform;
use crate::config::{CueLineStyle, ElevationConfig};

/// Half-width of the yaw range, around the instant two corners are equally far back, over which
/// their stalks cross-fade.
pub const CORNER_CROSSFADE_DEG: f32 = 15.0;
/// Vertices per stroke or fill quad (two triangles, not indexed).
const QUAD_VERTICES: usize = 6;
/// Quads per elevated module: one fill, four outline edges, at most four stalks.
const QUADS_PER_MODULE: usize = 9;

/// A module with `elevation_tier > 0` and what its cues need.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElevatedModule {
    /// Index in `StationScene::modules`.
    pub index: usize,
    pub tier: u32,
    /// Footprint on the ground: the bounds' minimum and maximum corner (exclusive) in x and y.
    pub footprint_min: Vec2,
    pub footprint_max: Vec2,
    /// Bottom of the bounds, where the stalks start.
    pub bottom_z: f32,
}

impl ElevatedModule {
    /// The four footprint corners, counter-clockwise from the minimum corner.
    pub fn corners(&self) -> [Vec2; 4] {
        let (lo, hi) = (self.footprint_min, self.footprint_max);
        [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)]
    }

    pub fn centre(&self) -> Vec2 {
        (self.footprint_min + self.footprint_max) * 0.5
    }

    /// Where the elevation tag hangs: the screen point below the footprint's centre at the height
    /// of its lowest corner on screen. Continuous through turns, unlike the front corner itself.
    pub fn tag_anchor(&self, view: &ViewTransform) -> Vec2 {
        let lowest = self
            .corners()
            .iter()
            .map(|c| view.world_to_screen(c.extend(0.0)).y)
            .fold(f32::NEG_INFINITY, f32::max);
        let centre = view.world_to_screen(self.centre().extend(0.0));
        Vec2::new(centre.x, lowest)
    }
}

/// The modules that get elevation cues. A module with a tier above 0 whose bounds do not start
/// above the grid has no stalks to draw; it is logged and skipped.
pub fn elevated_modules(scene: &StationScene) -> Vec<ElevatedModule> {
    scene
        .modules
        .iter()
        .enumerate()
        .filter(|(_, module)| module.elevation_tier > 0)
        .filter_map(|(index, module)| {
            let origin = module.chunk.origin();
            let size = module.chunk.size();
            let corner = |axis: usize| origin[axis] as f32;
            let extent = |axis: usize| size[axis] as f32;
            if origin[2] <= 0 {
                tracing::warn!(
                    module = %module.id,
                    tier = module.elevation_tier,
                    bottom_z = origin[2],
                    "lifted module does not start above the ops grid; no elevation cues"
                );
                return None;
            }
            Some(ElevatedModule {
                index,
                tier: module.elevation_tier,
                footprint_min: Vec2::new(corner(0), corner(1)),
                footprint_max: Vec2::new(corner(0) + extent(0), corner(1) + extent(1)),
                bottom_z: corner(2),
            })
        })
        .collect()
}

/// Opacity factor of the stalk at each of `corners` for a camera whose screen-up direction on the
/// ground is `up_ground`: 1 for visible corners, 0 for the hidden back corner, and in between
/// while two corners swap places mid-turn (module docs).
pub fn stalk_visibility(corners: &[Vec2; 4], up_ground: Vec2) -> [f32; 4] {
    let depth = corners.map(|c| c.dot(up_ground));
    let mut order = [0, 1, 2, 3];
    order.sort_by(|&a, &b| depth[b].total_cmp(&depth[a]));
    let (back, next) = (order[0], order[1]);
    let edge = corners[back] - corners[next];
    // 0 when both are equally far back, 1 once the back corner leads by the cross-fade angle.
    let lead = (depth[back] - depth[next])
        / (edge.length().max(f32::EPSILON) * CORNER_CROSSFADE_DEG.to_radians().sin());
    let t = lead.clamp(0.0, 1.0);
    let mut visibility = [1.0; 4];
    visibility[back] = 0.5 * (1.0 - t);
    visibility[next] = 0.5 * (1.0 + t);
    visibility
}

/// One overlay vertex (`OverlayIn` in the shader). 64 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct OverlayVertex {
    start: [f32; 3],
    end: [f32; 3],
    corner: [f32; 2],
    color: [f32; 4],
    /// Dash, gap, phase and stroke width in window pixels; width 0 marks a fill.
    dash: [f32; 4],
}

/// A dashed stroke style in window pixels.
#[derive(Clone, Copy, Debug)]
struct Stroke {
    color: [f32; 4],
    dash_px: f32,
    gap_px: f32,
    width_px: f32,
}

impl Stroke {
    fn new(style: &CueLineStyle, layout_scale: f32) -> Self {
        Self {
            color: style.color.to_vec4(style.alpha),
            dash_px: style.dash_ref_px[0] * layout_scale,
            gap_px: style.dash_ref_px[1] * layout_scale,
            width_px: style.width_ref_px * layout_scale,
        }
    }
}

fn push_stroke(
    out: &mut Vec<OverlayVertex>,
    start: Vec3,
    end: Vec3,
    stroke: &Stroke,
    phase_px: f32,
    opacity: f32,
) {
    let mut color = stroke.color;
    color[3] *= opacity;
    let vertex = |along: f32, side: f32| OverlayVertex {
        start: start.to_array(),
        end: end.to_array(),
        corner: [along, side],
        color,
        dash: [stroke.dash_px, stroke.gap_px, phase_px, stroke.width_px],
    };
    let [a, b, c, d] = [
        vertex(0.0, -1.0),
        vertex(1.0, -1.0),
        vertex(1.0, 1.0),
        vertex(0.0, 1.0),
    ];
    out.extend([a, b, c, a, c, d]);
}

fn push_fill(out: &mut Vec<OverlayVertex>, quad: [Vec3; 4], color: [f32; 4]) {
    let vertex = |p: Vec3| OverlayVertex {
        start: p.to_array(),
        end: p.to_array(),
        corner: [0.0, 0.0],
        color,
        dash: [0.0; 4],
    };
    let [a, b, c, d] = quad.map(vertex);
    out.extend([a, b, c, a, c, d]);
}

/// Appends the cues of `module` for `view` to `out`.
pub fn build_cues(
    out: &mut Vec<OverlayVertex>,
    module: &ElevatedModule,
    style: &ElevationConfig,
    view: &ViewTransform,
) {
    let footprint = Stroke::new(&style.footprint.line, view.layout_scale);
    let stalk = Stroke::new(&style.stalk, view.layout_scale);
    let corners = module.corners();
    let ground = corners.map(|c| c.extend(0.0));

    push_fill(
        out,
        ground,
        style
            .footprint
            .line
            .color
            .to_vec4(style.footprint.fill_alpha),
    );
    // One dashed path around the footprint: each edge continues the previous edge's dashes.
    let mut phase_px = 0.0;
    for i in 0..4 {
        let (a, b) = (ground[i], ground[(i + 1) % 4]);
        push_stroke(out, a, b, &footprint, phase_px, 1.0);
        phase_px += (view.world_to_screen(b) - view.world_to_screen(a)).length();
    }
    // Stalks run from the module down, so their dashes start at the module.
    for (corner, visibility) in corners
        .iter()
        .zip(stalk_visibility(&corners, view.up_ground))
    {
        if visibility > 0.0 {
            let top = corner.extend(module.bottom_z);
            push_stroke(out, top, corner.extend(0.0), &stalk, 0.0, visibility);
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct OverlayUniforms {
    view_proj: [[f32; 4]; 4],
    viewport: [f32; 4],
}

pub struct OverlayPass {
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    vertex_count: u32,
    /// Reused every frame, so building the cues allocates nothing.
    scratch: Vec<OverlayVertex>,
    modules: Vec<ElevatedModule>,
    style: ElevationConfig,
}

impl OverlayPass {
    pub fn new(
        device: &wgpu::Device,
        target: TargetFormat,
        modules: Vec<ElevatedModule>,
        style: ElevationConfig,
    ) -> Self {
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlay uniforms"),
            contents: bytemuck::bytes_of(&OverlayUniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay"),
            entries: &[super::uniform_entry(
                wgpu::ShaderStages::VERTEX,
                size_of::<OverlayUniforms>(),
            )],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let vertex_capacity = (modules.len() * QUADS_PER_MODULE * QUAD_VERTICES).max(1);
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay vertices"),
            size: (vertex_capacity * size_of::<OverlayVertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            uniform_buffer,
            bind_group,
            pipeline: overlay_pipeline(device, &layout, target),
            vertex_buffer,
            vertex_capacity,
            vertex_count: 0,
            scratch: Vec::with_capacity(vertex_capacity),
            modules,
            style,
        }
    }

    /// Rebuilds the cues for this frame's view and uploads them.
    pub fn prepare(&mut self, queue: &wgpu::Queue, view: &ViewTransform) {
        let uniforms = OverlayUniforms {
            view_proj: view.view_proj().to_cols_array_2d(),
            viewport: [view.viewport_px.x, view.viewport_px.y, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        self.scratch.clear();
        for module in &self.modules {
            build_cues(&mut self.scratch, module, &self.style, view);
        }
        debug_assert!(self.scratch.len() <= self.vertex_capacity);
        self.scratch.truncate(self.vertex_capacity);
        self.vertex_count = u32::try_from(self.scratch.len())
            .expect("invariant: a few dozen vertices per elevated module");
        if !self.scratch.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.scratch));
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.vertex_count == 0 {
            return;
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}

fn overlay_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    target: TargetFormat,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("overlay"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("overlay.wgsl"),
        source: wgpu::ShaderSource::Wgsl(super::OVERLAY_WGSL.into()),
    });
    let attributes = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x4,
        4 => Float32x4,
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("overlay"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_overlay"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<OverlayVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        // Doc 05 step 5: depth test on, depth write off. Footprints lie exactly on the plane of
        // the deck's bottom faces, so equal depths pass.
        depth_stencil: Some(super::context::depth_stencil(
            false,
            Some(wgpu::CompareFunction::LessEqual),
        )),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_overlay"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &super::scene_targets(
                target,
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            ),
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::voxel::load_station_scene;
    use crate::camera::{Camera, Projection, Turn};
    use crate::config::{
        default_asset_root, load_scene_config, scene_config_path, station_scene_path,
    };

    fn camera() -> Camera {
        let config = load_scene_config(&scene_config_path(&default_asset_root())).unwrap();
        Camera::new(Projection::from_config(&config), Vec2::new(1920.0, 1080.0))
    }

    fn modules() -> Vec<ElevatedModule> {
        elevated_modules(&load_station_scene(&station_scene_path(&default_asset_root())).unwrap())
    }

    /// Doc 01 §5 and doc 03: FA-1 at +8 and FB-2 at +4, footprints from their bounds (inclusive
    /// voxel coordinates, so the far edge is max + 1) and stalks from bounds.min.z. The bounds
    /// are those of the spread layout (D-111).
    #[test]
    fn footprints_come_from_the_bounds() {
        let modules = modules();
        assert_eq!(modules.len(), 2);
        let fa1 = modules[0];
        assert_eq!(fa1.tier, 8);
        assert_eq!(fa1.footprint_min, Vec2::new(-59.0, 13.0));
        assert_eq!(fa1.footprint_max, Vec2::new(-39.0, 31.0));
        assert!((fa1.bottom_z - 8.0).abs() < 1e-6);
        let fb2 = modules[1];
        assert_eq!(fb2.tier, 4);
        assert_eq!(fb2.footprint_min, Vec2::new(27.0, 31.0));
        assert_eq!(fb2.footprint_max, Vec2::new(45.0, 49.0));
        assert!((fb2.bottom_z - 4.0).abs() < 1e-6);
    }

    /// In every settled view exactly the back corner (farthest along screen-up) is hidden, and the
    /// three visible ones are the left, right and front corners on screen.
    #[test]
    fn three_corners_are_visible_in_every_view() {
        let mut camera = camera();
        for module in modules() {
            let corners = module.corners();
            for view in 0..4 {
                camera.set_view(view);
                let t = camera.transform();
                let visibility = stalk_visibility(&corners, t.up_ground);
                assert_eq!(
                    visibility
                        .iter()
                        .filter(|&&v| (v - 1.0).abs() < 1e-6)
                        .count(),
                    3
                );
                let hidden = visibility.iter().position(|&v| v.abs() < 1e-6).unwrap();
                // The hidden corner is the topmost on screen (smallest y at z = 0).
                let screen_y = corners.map(|c| t.world_to_screen(c.extend(0.0)).y);
                let topmost = (0..4)
                    .min_by(|&a, &b| screen_y[a].total_cmp(&screen_y[b]))
                    .unwrap();
                assert_eq!(hidden, topmost, "view {view}");
            }
        }
    }

    /// Through a whole turn the visibility changes continuously (no pop), and halfway through the
    /// two back corners are half visible each.
    #[test]
    fn corner_visibility_is_continuous_through_a_turn() {
        let mut camera = camera();
        let module = modules()[0];
        let corners = module.corners();
        camera.rotate(Turn::Plus90);
        let mut previous = stalk_visibility(&corners, camera.transform().up_ground);
        let steps = 250;
        for _ in 0..steps {
            camera.update(0.25 / steps as f32);
            let visibility = stalk_visibility(&corners, camera.transform().up_ground);
            for (a, b) in previous.iter().zip(&visibility) {
                assert!((a - b).abs() < 0.1, "{previous:?} → {visibility:?}");
            }
            assert!((visibility.iter().sum::<f32>() - 3.0).abs() < 1e-4);
            previous = visibility;
        }
        // Halfway, at yaw 90°, screen-up on the ground is H = (−1, 0): the two corners with the
        // smallest x (indices 0 and 3) are equally far back.
        let halfway = stalk_visibility(&corners, Vec2::new(-1.0, 0.0));
        for (actual, expected) in halfway.iter().zip([0.5, 1.0, 1.0, 0.5]) {
            assert!((actual - expected).abs() < 1e-6, "{halfway:?}");
        }
    }

    #[test]
    fn cues_have_a_fill_four_edges_and_three_stalks() {
        let config = load_scene_config(&scene_config_path(&default_asset_root())).unwrap();
        let t = camera().transform();
        let mut vertices = Vec::new();
        build_cues(&mut vertices, &modules()[0], &config.elevation, &t);
        assert_eq!(vertices.len(), (1 + 4 + 3) * QUAD_VERTICES);
        // The fill sits on the grid, with the fill alpha.
        assert!(
            vertices[..6]
                .iter()
                .all(|v| v.start[2] == 0.0 && v.dash[3] == 0.0)
        );
        assert!((vertices[0].color[3] - 0.06).abs() < 1e-6);
        // Outline edges continue each other's dashes: each phase is the screen length so far.
        let phases: Vec<f32> = (0..4).map(|i| vertices[6 + i * 6].dash[2]).collect();
        assert!(phases[0] == 0.0 && phases.windows(2).all(|w| w[1] > w[0]));
        // Stalks run from the module bottom (z = 8) down to the grid.
        for stalk in vertices[30..].chunks_exact(6) {
            assert!((stalk[0].start[2] - 8.0).abs() < 1e-6 && stalk[0].end[2] == 0.0);
            assert!((stalk[0].color[3] - 0.6).abs() < 1e-6);
        }
        assert!(QUADS_PER_MODULE * QUAD_VERTICES >= vertices.len());
    }

    /// The tag hangs below the footprint: under its centre, level with its lowest corner.
    #[test]
    fn tag_anchor_is_below_the_footprint() {
        let t = camera().transform();
        let module = modules()[1];
        let anchor = module.tag_anchor(&t);
        let corners = module.corners().map(|c| t.world_to_screen(c.extend(0.0)));
        let lowest = corners.iter().map(|c| c.y).fold(f32::MIN, f32::max);
        assert!((anchor.y - lowest).abs() < 1e-3);
        let centre = t.world_to_screen(module.centre().extend(0.0));
        assert!((anchor.x - centre.x).abs() < 1e-3);
    }

    #[test]
    fn vertex_layout_matches_the_shader() {
        assert_eq!(size_of::<OverlayVertex>(), 64);
        assert_eq!(size_of::<OverlayUniforms>(), 80);
    }
}
