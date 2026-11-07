//! The power shield (phase-1 doc 05 `render/shield.rs`, frame order step 6; doc 01 §6 with
//! D-100; doc 06 M4). The surface shader is `shaders/shield.wgsl`; the lines use
//! `render/lines.rs`.
//!
//! # Shape
//!
//! D-100: a full **globe** centred on the ship plane (`center` z = 16), an ellipsoid with the
//! horizontal radius of the 2D hit circle (doc 15 §5.1; 36 voxels, D-119) and a tuned vertical
//! radius. Everything below the ops grid (`z < 0`) is drawn at `below_grid_opacity`. The phase-1
//! **dome** (the cap above `center` z) stays available as `shape: "dome"` for comparison.
//!
//! # Draw order
//!
//! Within the scene pass, after the elevation overlays and before the bloom (doc 05 steps 5, 6
//! and 8), depth-tested against the voxels and writing no depth:
//!
//! 1. the surface's **back faces** (fill and hit ripples);
//! 2. the **far half of the sector ring**, so the front surface tints it;
//! 3. the surface's **front faces**;
//! 4. the **near half of the sector ring** and the **rim** (the silhouette).
//!
//! The rim and the ring are rebuilt on the CPU every frame (a few hundred vertices), so they
//! follow the view through turns. The ring lies on the equator, one arc per sector; a sector
//! whose pylon is below `weak_threshold` is drawn in `weak_color` and dashed, and so is the rim
//! over it (doc 01 §6). The strengths come from the fixture (`selection.pylons`).
//!
//! # Projection
//!
//! The camera is orthographic over a world whose z axis is scaled by the projection's z scale
//! (`camera::ViewTransform::view_proj`). In that z-scaled space the view is a rigid orthographic
//! camera looking along `(cos θ·H, −sin θ)`, so silhouettes, facing and fresnel are computed there.
//!
//! # Hits
//!
//! Doc 06 M4: ripples on a scripted loop at the two impact points of the reference render. Each
//! impact is given as a reference pixel of view 0 at zoom 1; it lies where that pixel's view ray
//! first meets the surface. A ripple grows over `ripple_ms` and fades; a hot flash (doc 01 §6)
//! lasts `flash_ms` and feeds the bloom.

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use wgpu::util::DeviceExt;

use super::context::TargetFormat;
use super::lines::{LineBatch, LineVertex, QUAD_VERTICES, Stroke, push_stroke};
use crate::camera::{Camera, Projection, ViewTransform};
use crate::config::{
    Fixture, MAX_SHIELD_IMPACTS, Rgb, SceneConfig, ShieldConfig, ShieldLine, ShieldShape,
};

/// Segments of the rim (the silhouette loop).
pub const RIM_SEGMENTS: usize = 192;
/// Segments of the sector ring around the equator.
pub const RING_SEGMENTS: usize = 192;
/// Latitude and longitude bands of the surface mesh.
const MESH_LAT: u32 = 48;
const MESH_LON: u32 = 96;

/// The shield's ellipsoid in world voxels, with the projection's z scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipsoid {
    pub center: Vec3,
    /// Semi-axes: `radius_xy`, `radius_xy`, `radius_z`.
    pub radii: Vec3,
    pub z_scale: f32,
}

impl Ellipsoid {
    pub fn new(shield: &ShieldConfig, z_scale: f32) -> Self {
        Self {
            center: Vec3::from(shield.center),
            radii: Vec3::new(shield.radius_xy, shield.radius_xy, shield.radius_z),
            z_scale,
        }
    }

    /// The surface point in the direction of the unit vector `unit` (the mesh parameterisation).
    pub fn point(&self, unit: Vec3) -> Vec3 {
        self.center + self.radii * unit
    }

    /// The outward surface normal at `point`, in world space (unit length).
    pub fn normal(&self, point: Vec3) -> Vec3 {
        ((point - self.center) / (self.radii * self.radii)).normalize()
    }

    /// The view direction into the screen, in z-scaled space (module docs, "Projection").
    pub fn view_dir_scaled(view: &ViewTransform) -> Vec3 {
        Vec3::new(
            view.cos_pitch * view.up_ground.x,
            view.cos_pitch * view.up_ground.y,
            -view.sin_pitch,
        )
    }

    /// The silhouette for `view` as a closed loop of `segments` world points: where the surface
    /// normal (in z-scaled space) is perpendicular to the view direction.
    pub fn silhouette(&self, view: &ViewTransform, segments: usize) -> Vec<Vec3> {
        let v = Self::view_dir_scaled(view);
        let a = self.radii.x;
        let c = self.radii.z * self.z_scale;
        // On the unit sphere the silhouette is the great circle perpendicular to w.
        let w = Vec3::new(v.x / a, v.y / a, v.z / c).normalize();
        let helper = if w.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
        let e1 = w.cross(helper).normalize();
        let e2 = w.cross(e1);
        (0..segments)
            .map(|i| {
                let phi = std::f32::consts::TAU * i as f32 / segments as f32;
                self.point(e1 * phi.cos() + e2 * phi.sin())
            })
            .collect()
    }

    /// Where the view ray through the screen pixel `px` first meets the surface, if it does.
    pub fn front_hit(&self, view: &ViewTransform, px: Vec2) -> Option<Vec3> {
        let ground = view.screen_to_ground(px);
        let origin = ground.extend(0.0);
        // World direction into the screen that keeps the screen position (z-scaled space:
        // `view_dir_scaled`).
        let dir = Vec3::new(
            view.cos_pitch * view.up_ground.x,
            view.cos_pitch * view.up_ground.y,
            -view.sin_pitch / view.z_scale,
        );
        let o = (origin - self.center) / self.radii;
        let d = dir / self.radii;
        let qa = d.dot(d);
        let qb = 2.0 * o.dot(d);
        let qc = o.dot(o) - 1.0;
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 || qa <= 0.0 {
            return None;
        }
        // Nearer to the viewer is smaller t.
        let t = (-qb - disc.sqrt()) / (2.0 * qa);
        Some(origin + dir * t)
    }

    /// The azimuth of `point` around the centre, degrees in `[0, 360)` from +x towards +y.
    pub fn azimuth_deg(&self, point: Vec3) -> f32 {
        let d = point - self.center;
        d.y.atan2(d.x).to_degrees().rem_euclid(360.0)
    }
}

/// The shield's sectors with their state (doc 01 §6, D-046).
#[derive(Clone, Debug, PartialEq)]
pub struct Sectors {
    /// Start and end azimuth (degrees) and whether the sector is weak.
    arcs: Vec<(f32, f32, bool)>,
}

impl Sectors {
    /// The sectors of `shield`, weak where the fixture's pylon strength is below the threshold.
    /// A sector without a strength counts as healthy (the asset loader rejects that case).
    pub fn new(shield: &ShieldConfig, fixture: &Fixture) -> Self {
        let arcs = shield
            .sectors
            .iter()
            .map(|sector| {
                let strength = fixture.pylon_strength(&sector.pylon).unwrap_or(1.0);
                (
                    sector.az_deg[0],
                    sector.az_deg[1],
                    strength < shield.weak_threshold,
                )
            })
            .collect();
        Self { arcs }
    }

    /// Whether the sector at `azimuth_deg` is weak; `false` where no sector covers it.
    pub fn weak_at(&self, azimuth_deg: f32) -> bool {
        self.arcs.iter().any(|&(start, end, weak)| {
            weak && (azimuth_deg - start).rem_euclid(360.0) <= end - start
        })
    }

    /// The arcs that are weak, as `(start, end)` degrees.
    pub fn weak_arcs(&self) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.arcs
            .iter()
            .filter(|a| a.2)
            .map(|&(start, end, _)| (start, end))
    }
}

/// One scripted impact, placed on the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub point: Vec3,
    pub normal: Vec3,
    pub radius_voxels: f32,
    pub delay_ms: u32,
    pub weak: bool,
}

/// Places the configured impacts on the surface (module docs, "Hits"). An impact whose reference
/// pixel misses the shield is logged and skipped.
pub fn place_impacts(
    config: &SceneConfig,
    ellipsoid: &Ellipsoid,
    sectors: &Sectors,
) -> Vec<Impact> {
    let [w, h] = config.reference_resolution;
    let reference = Camera::new(
        Projection::from_config(config),
        Vec2::new(w as f32, h as f32),
    )
    .transform();
    config
        .shield
        .hits
        .impacts
        .iter()
        .take(MAX_SHIELD_IMPACTS)
        .filter_map(|impact| {
            let px = Vec2::from(impact.reference_px);
            let Some(point) = ellipsoid.front_hit(&reference, px) else {
                tracing::warn!(
                    reference_px = ?impact.reference_px,
                    "shield impact misses the shield; skipped"
                );
                return None;
            };
            if config.shield.shape == ShieldShape::Dome && point.z < ellipsoid.center.z {
                tracing::warn!(reference_px = ?impact.reference_px, "impact below the dome; skipped");
                return None;
            }
            Some(Impact {
                point,
                normal: ellipsoid.normal(point),
                radius_voxels: impact.radius_voxels,
                delay_ms: impact.delay_ms,
                weak: sectors.weak_at(ellipsoid.azimuth_deg(point)),
            })
        })
        .collect()
}

/// The state of one ripple at a moment of the script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RippleState {
    /// The ripple's radius now (0: not running).
    pub radius: f32,
    /// Its opacity factor, 1 at the start, 0 at the end.
    pub fade: f32,
    /// The flash's intensity, 1 at the impact, 0 after `flash_ms`.
    pub flash: f32,
}

/// The ripple of an impact `elapsed_ms` into the script (doc 01 §6: it grows to its radius over
/// `ripple_ms`, easing out, and fades; the flash is brief).
pub fn ripple_state(
    impact_radius: f32,
    delay_ms: u32,
    loop_ms: u32,
    ripple_ms: u32,
    flash_ms: u32,
    elapsed_ms: u64,
) -> RippleState {
    let loop_ms = u64::from(loop_ms.max(1));
    let local = (elapsed_ms % loop_ms + loop_ms - u64::from(delay_ms) % loop_ms) % loop_ms;
    let local = local as f32;
    let p = local / ripple_ms.max(1) as f32;
    let (radius, fade) = if p < 1.0 {
        let ease = 1.0 - (1.0 - p) * (1.0 - p);
        (impact_radius * (0.3 + 0.7 * ease), (1.0 - p).powf(1.5))
    } else {
        (0.0, 0.0)
    };
    let flash = if flash_ms > 0 && local < flash_ms as f32 {
        1.0 - local / flash_ms as f32
    } else {
        0.0
    };
    RippleState {
        radius,
        fade,
        flash,
    }
}

/// The opacity of one surface layer such that the back and front layers together have opacity
/// `total`: the configured fill opacities are those of the whole shield as seen (doc 01 §6:
/// about 0.05), and every line of sight crosses the surface twice.
pub fn per_layer_alpha(total: f32) -> f32 {
    1.0 - (1.0 - total.clamp(0.0, 1.0)).sqrt()
}

/// Window-pixel stroke of a shield line style.
fn stroke(line: &ShieldLine, color: Rgb, layout_scale: f32) -> Stroke {
    Stroke {
        color: color.to_vec4(line.alpha),
        dash_px: line.dash_ref_px[0] * layout_scale,
        gap_px: line.dash_ref_px[1] * layout_scale,
        width_px: line.width_ref_px * layout_scale,
    }
}

/// The rim and ring vertices for one frame: `far` is drawn before the front faces, `near` after.
#[derive(Default)]
pub struct ShieldLines {
    pub far: Vec<LineVertex>,
    pub near: Vec<LineVertex>,
}

/// A run of segments drawn with one style, so the dashes continue across segment joints.
struct Run<'a> {
    out: &'a mut Vec<LineVertex>,
    key: Option<(bool, bool)>,
    phase_px: f32,
}

/// Builds the rim and the sector ring for `view` (module docs, "Draw order").
pub fn build_lines(
    out: &mut ShieldLines,
    shield: &ShieldConfig,
    ellipsoid: &Ellipsoid,
    sectors: &Sectors,
    view: &ViewTransform,
) {
    out.far.clear();
    out.near.clear();
    let scale = view.layout_scale;
    let rim = Stroke {
        color: shield.rim_color.to_vec4(shield.rim_alpha),
        dash_px: 1.0,
        gap_px: 0.0,
        width_px: shield.rim_width_ref_px * scale,
    };
    let rim_weak = stroke(&shield.rim_weak, shield.weak_color, scale);
    let ring = stroke(&shield.ring, shield.rim_color, scale);
    let ring_weak = stroke(&shield.ring_weak, shield.weak_color, scale);
    let ring_far = stroke(&shield.ring_far, shield.rim_color, scale);
    let ring_far_weak = stroke(&shield.ring_far, shield.weak_color, scale);
    let below = |z: f32| {
        if z < 0.0 {
            shield.below_grid_opacity
        } else {
            1.0
        }
    };
    let dome = shield.shape == ShieldShape::Dome;

    // The sector ring on the equator (the dome's base).
    let h = view.up_ground;
    let point_at = |az: f32| {
        ellipsoid.center
            + Vec3::new(
                az.cos() * ellipsoid.radii.x,
                az.sin() * ellipsoid.radii.y,
                0.0,
            )
    };
    let mut far_run = Run {
        out: &mut out.far,
        key: None,
        phase_px: 0.0,
    };
    let mut near_ring = Vec::new();
    let mut near_run = Run {
        out: &mut near_ring,
        key: None,
        phase_px: 0.0,
    };
    for i in 0..RING_SEGMENTS {
        let a0 = std::f32::consts::TAU * i as f32 / RING_SEGMENTS as f32;
        let a1 = std::f32::consts::TAU * (i + 1) as f32 / RING_SEGMENTS as f32;
        let mid = 0.5 * (a0 + a1);
        let weak = sectors.weak_at(mid.to_degrees().rem_euclid(360.0));
        // The far half faces away from the viewer: its outward direction points along H.
        let far = Vec2::new(mid.cos(), mid.sin()).dot(h) > 0.0;
        let (p0, p1) = (point_at(a0), point_at(a1));
        let opacity = below(ellipsoid.center.z);
        let (run, style) = match (far, weak) {
            (true, false) => (&mut far_run, &ring_far),
            (true, true) => (&mut far_run, &ring_far_weak),
            (false, false) => (&mut near_run, &ring),
            (false, true) => (&mut near_run, &ring_weak),
        };
        push_run(run, (far, weak), p0, p1, style, opacity, view);
    }

    // The rim: the silhouette loop, coloured by the sector under each segment.
    let loop_points = ellipsoid.silhouette(view, RIM_SEGMENTS);
    let mut rim_run = Run {
        out: &mut out.near,
        key: None,
        phase_px: 0.0,
    };
    for i in 0..loop_points.len() {
        let (p0, p1) = (loop_points[i], loop_points[(i + 1) % loop_points.len()]);
        let mid = 0.5 * (p0 + p1);
        if dome && mid.z < ellipsoid.center.z {
            rim_run.key = None;
            continue;
        }
        let weak = sectors.weak_at(ellipsoid.azimuth_deg(mid));
        let style = if weak { &rim_weak } else { &rim };
        push_run(
            &mut rim_run,
            (false, weak),
            p0,
            p1,
            style,
            below(mid.z),
            view,
        );
    }
    out.near.extend(near_ring);
}

fn push_run(
    run: &mut Run<'_>,
    key: (bool, bool),
    p0: Vec3,
    p1: Vec3,
    style: &Stroke,
    opacity: f32,
    view: &ViewTransform,
) {
    if run.key != Some(key) {
        run.key = Some(key);
        run.phase_px = 0.0;
    }
    push_stroke(run.out, p0, p1, style, run.phase_px, opacity);
    run.phase_px += (view.world_to_screen(p1) - view.world_to_screen(p0)).length();
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ShieldUniforms {
    view_proj: [[f32; 4]; 4],
    center: [f32; 4],
    radii: [f32; 4],
    view_dir: [f32; 4],
    fill: [f32; 4],
    fill_color: [f32; 4],
    hex_color: [f32; 4],
    outline: [f32; 4],
    outline_weak: [f32; 4],
    flash_core: [f32; 4],
    flash_edge: [f32; 4],
    hit_pos: [[f32; 4]; MAX_SHIELD_IMPACTS],
    hit_normal: [[f32; 4]; MAX_SHIELD_IMPACTS],
    hit_info: [[f32; 4]; MAX_SHIELD_IMPACTS],
}

pub struct ShieldPass {
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    back: wgpu::RenderPipeline,
    front: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    lines: LineBatch,
    /// Reused every frame.
    scratch: ShieldLines,
    /// Vertices of `scratch.far` in this frame's upload.
    far_count: u32,
    config: ShieldConfig,
    ellipsoid: Ellipsoid,
    sectors: Sectors,
    impacts: Vec<Impact>,
}

impl ShieldPass {
    pub fn new(
        device: &wgpu::Device,
        target: TargetFormat,
        config: &SceneConfig,
        fixture: &Fixture,
    ) -> Self {
        let shield = config.shield.clone();
        let ellipsoid = Ellipsoid::new(&shield, config.projection.effective_z_scale());
        let sectors = Sectors::new(&shield, fixture);
        let impacts = place_impacts(config, &ellipsoid, &sectors);
        tracing::info!(
            impacts = impacts.len(),
            weak_sectors = sectors.weak_arcs().count(),
            "shield"
        );
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shield uniforms"),
            contents: bytemuck::bytes_of(&ShieldUniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shield"),
            entries: &[super::uniform_entry(
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                size_of::<ShieldUniforms>(),
            )],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shield"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let (vertices, indices) = unit_sphere(MESH_LAT, MESH_LON);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shield vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shield indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shield"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shield.wgsl"),
            source: wgpu::ShaderSource::Wgsl(super::SHIELD_WGSL.into()),
        });
        let pipeline = |entry: &str, label: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_shield"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<[f32; 3]>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                    })],
                },
                // Facing is decided in the shader from the normal, so no culling.
                primitive: wgpu::PrimitiveState::default(),
                // Doc 05 step 6: alpha blend, depth test on, depth write off.
                depth_stencil: Some(super::context::depth_stencil(
                    false,
                    Some(wgpu::CompareFunction::LessEqual),
                )),
                multisample: target.multisample(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &super::scene_targets(
                        target,
                        Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    ),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let line_capacity = (RIM_SEGMENTS + RING_SEGMENTS) * QUAD_VERTICES;
        Self {
            uniform_buffer,
            bind_group,
            back: pipeline("fs_shield_back", "shield back faces"),
            front: pipeline("fs_shield_front", "shield front faces"),
            vertex_buffer,
            index_buffer,
            index_count: u32::try_from(indices.len())
                .expect("invariant: the sphere mesh has a few thousand indices"),
            lines: LineBatch::new(device, target, line_capacity, "shield lines"),
            scratch: ShieldLines::default(),
            far_count: 0,
            config: shield,
            ellipsoid,
            sectors,
            impacts,
        }
    }

    /// Updates the uniforms and lines for `view` at `elapsed_ms` into the hit script.
    pub fn prepare(&mut self, queue: &wgpu::Queue, view: &ViewTransform, elapsed_ms: u64) {
        let shield = &self.config;
        let hits = &shield.hits;
        let mut uniforms = ShieldUniforms::zeroed();
        uniforms.view_proj = view.view_proj().to_cols_array_2d();
        let dome = if shield.shape == ShieldShape::Dome {
            1.0
        } else {
            0.0
        };
        uniforms.center = self.ellipsoid.center.extend(dome).to_array();
        uniforms.radii = self
            .ellipsoid
            .radii
            .extend(self.ellipsoid.z_scale)
            .to_array();
        uniforms.view_dir = Ellipsoid::view_dir_scaled(view)
            .extend(shield.below_grid_opacity)
            .to_array();
        uniforms.fill = [
            per_layer_alpha(shield.rest_alpha_fill),
            per_layer_alpha(shield.edge_alpha_fill),
            shield.fresnel_power,
            hits.hex_cell_voxels,
        ];
        uniforms.fill_color = shield.rim_color.to_vec4(self.impacts.len() as f32);
        uniforms.hex_color = hits.hex_color.to_vec4(hits.hex_alpha);
        uniforms.outline = hits.outline_color.to_vec4(hits.outline_alpha);
        uniforms.outline_weak = hits.outline_weak_color.to_vec4(0.0);
        uniforms.flash_core = hits.flash_core.to_vec4(hits.flash_radius_voxels);
        uniforms.flash_edge = hits.flash_edge.to_vec4(1.0 / view.px_per_unit.max(1e-6));
        for (i, impact) in self.impacts.iter().enumerate() {
            let state = ripple_state(
                impact.radius_voxels,
                impact.delay_ms,
                hits.loop_ms,
                hits.ripple_ms,
                hits.flash_ms,
                elapsed_ms,
            );
            uniforms.hit_pos[i] = impact.point.extend(state.radius).to_array();
            uniforms.hit_normal[i] = impact.normal.extend(state.fade).to_array();
            uniforms.hit_info[i] = [if impact.weak { 1.0 } else { 0.0 }, state.flash, 0.0, 0.0];
        }
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        build_lines(
            &mut self.scratch,
            &self.config,
            &self.ellipsoid,
            &self.sectors,
            view,
        );
        self.far_count =
            u32::try_from(self.scratch.far.len()).expect("invariant: a few hundred line vertices");
        let mut all = std::mem::take(&mut self.scratch.far);
        all.extend_from_slice(&self.scratch.near);
        self.lines.upload(queue, view, &all);
        all.truncate(self.far_count as usize);
        self.scratch.far = all;
    }

    /// Draws the shield in the order of the module docs.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_pipeline(&self.back);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
        self.lines.draw(pass, 0..self.far_count);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_pipeline(&self.front);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
        self.lines.draw(pass, self.far_count..self.lines.len());
    }
}

/// A unit sphere with `lat` latitude and `lon` longitude bands: positions and triangle indices.
fn unit_sphere(lat: u32, lon: u32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(((lat + 1) * (lon + 1)) as usize);
    for i in 0..=lat {
        let phi = std::f32::consts::PI * (i as f32 / lat as f32 - 0.5);
        for j in 0..=lon {
            let theta = std::f32::consts::TAU * j as f32 / lon as f32;
            vertices.push([phi.cos() * theta.cos(), phi.cos() * theta.sin(), phi.sin()]);
        }
    }
    let mut indices = Vec::with_capacity((lat * lon * 6) as usize);
    for i in 0..lat {
        for j in 0..lon {
            let a = i * (lon + 1) + j;
            let b = a + lon + 1;
            indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        default_asset_root, fixture_path, load_fixture, load_scene_config, scene_config_path,
    };

    fn config() -> SceneConfig {
        load_scene_config(&scene_config_path(&default_asset_root())).unwrap()
    }

    fn fixture() -> Fixture {
        load_fixture(&fixture_path(&default_asset_root())).unwrap()
    }

    fn ellipsoid() -> Ellipsoid {
        let config = config();
        Ellipsoid::new(&config.shield, config.projection.effective_z_scale())
    }

    fn camera() -> Camera {
        Camera::new(
            Projection::from_config(&config()),
            Vec2::new(1920.0, 1080.0),
        )
    }

    /// Every silhouette point lies on the surface, its normal is perpendicular to the view
    /// direction in z-scaled space, and no sampled surface point projects outside the loop.
    #[test]
    fn silhouette_bounds_the_projected_surface() {
        let globe = ellipsoid();
        let mut camera = camera();
        let (vertices, _) = unit_sphere(24, 48);
        for view in 0..4 {
            camera.set_view(view);
            let transform = camera.transform();
            let view_dir = Ellipsoid::view_dir_scaled(&transform);
            let rim = globe.silhouette(&transform, RIM_SEGMENTS);
            let scaled_z = globe.radii.z * globe.z_scale;
            for point in &rim {
                let unit = (*point - globe.center) / globe.radii;
                assert!((unit.length() - 1.0).abs() < 1e-4);
                let normal = Vec3::new(
                    unit.x / globe.radii.x,
                    unit.y / globe.radii.y,
                    unit.z / scaled_z,
                )
                .normalize();
                assert!(normal.dot(view_dir).abs() < 1e-4, "view {view}");
            }
            let outline: Vec<Vec2> = rim
                .iter()
                .map(|point| transform.world_to_screen(*point))
                .collect();
            let centre = transform.world_to_screen(globe.center);
            for unit in &vertices {
                let screen = transform.world_to_screen(globe.point(Vec3::from(*unit)));
                assert!(
                    inside_or_on(&outline, centre, screen),
                    "view {view}: {screen} outside"
                );
            }
        }
    }

    /// Whether `p` lies inside the star-shaped polygon `outline` around `centre` (with a little
    /// slack for the polygon's chords).
    fn inside_or_on(outline: &[Vec2], centre: Vec2, point: Vec2) -> bool {
        let offset = point - centre;
        let angle = offset.y.atan2(offset.x);
        let count = outline.len();
        (0..count).any(|i| {
            let start = outline[i] - centre;
            let end = outline[(i + 1) % count] - centre;
            let start_angle = start.y.atan2(start.x);
            let span = (end.y.atan2(end.x) - start_angle).rem_euclid(std::f32::consts::TAU);
            let along = (angle - start_angle).rem_euclid(std::f32::consts::TAU);
            if along > span + 1e-6 {
                return false;
            }
            let fraction = if span > 0.0 { along / span } else { 0.0 };
            offset.length() <= start.lerp(end, fraction).length() + 1.0
        })
    }

    /// The impacts lie on the surface, on the side facing the camera of view 0, and project back
    /// to their reference pixels; the one in the weak E sector is marked weak.
    #[test]
    fn impacts_sit_where_the_reference_shows_them() {
        let config = config();
        let globe = ellipsoid();
        let sectors = Sectors::new(&config.shield, &fixture());
        let impacts = place_impacts(&config, &globe, &sectors);
        assert_eq!(impacts.len(), 2);
        let transform = camera().transform();
        let view_dir = Ellipsoid::view_dir_scaled(&transform);
        for (impact, given) in impacts.iter().zip(&config.shield.hits.impacts) {
            let unit = (impact.point - globe.center) / globe.radii;
            assert!((unit.length() - 1.0).abs() < 1e-3);
            let screen = transform.world_to_screen(impact.point);
            assert!(
                screen.distance(Vec2::from(given.reference_px)) < 0.5,
                "{screen}"
            );
            let scaled_normal = Vec3::new(
                unit.x / globe.radii.x,
                unit.y / globe.radii.y,
                unit.z / (globe.radii.z * globe.z_scale),
            );
            assert!(scaled_normal.dot(view_dir) < 0.0, "front surface");
        }
        assert!(impacts[0].weak, "the first impact is in the 38% E sector");
    }

    #[test]
    fn sectors_follow_the_fixture() {
        let config = config();
        let sectors = Sectors::new(&config.shield, &fixture());
        // E is 270–360° and at 38% (D-046; fixture), the others are above 50%.
        assert!(sectors.weak_at(300.0));
        assert!(sectors.weak_at(359.9));
        assert!(!sectors.weak_at(0.5));
        assert!(!sectors.weak_at(200.0));
        assert_eq!(
            sectors.weak_arcs().collect::<Vec<_>>(),
            vec![(270.0, 360.0)]
        );
    }

    #[test]
    fn ripples_grow_fade_and_repeat() {
        let at = |ms| ripple_state(5.0, 300, 2000, 600, 150, ms);
        // Before its delay a ripple is off; at the impact it starts small with a full flash.
        assert!(at(100).radius.abs() < 1e-9);
        let start = at(300);
        assert!((start.radius - 1.5).abs() < 1e-5 && (start.flash - 1.0).abs() < 1e-6);
        let mid = at(600);
        assert!(mid.radius > start.radius && mid.fade < 1.0 && mid.flash.abs() < 1e-9);
        assert!(at(900).radius.abs() < 1e-9);
        // The script repeats every loop.
        assert_eq!(at(2600), mid);
    }

    #[test]
    fn lines_split_far_and_near_and_dash_the_weak_sector() {
        let config = config();
        let e = ellipsoid();
        let sectors = Sectors::new(&config.shield, &fixture());
        let t = camera().transform();
        let mut lines = ShieldLines::default();
        build_lines(&mut lines, &config.shield, &e, &sectors, &t);
        assert_eq!(
            lines.far.len() + lines.near.len(),
            (RIM_SEGMENTS + RING_SEGMENTS) * QUAD_VERTICES
        );
        // Half of the ring is far, half near.
        assert_eq!(lines.far.len(), RING_SEGMENTS / 2 * QUAD_VERTICES);
        let weak = config.shield.weak_color.to_unit();
        let is_weak = |v: &LineVertex| (0..3).all(|c| (v.color[c] - weak[c]).abs() < 1e-6);
        // Weak strokes are dashed; the healthy near ring and the healthy rim are solid.
        assert!(
            lines
                .near
                .iter()
                .filter(|v| is_weak(v))
                .all(|v| v.dash[1] > 0.0)
        );
        assert!(lines.near.iter().any(|v| !is_weak(v) && v.dash[1] == 0.0));
        assert!(lines.near.iter().any(is_weak));
        // The far half is dashed throughout.
        assert!(lines.far.iter().all(|v| v.dash[1] > 0.0));
        // Rim parts below the ops grid are fainter.
        let rim = &lines.near[..RIM_SEGMENTS * QUAD_VERTICES];
        let mut below = rim.iter().filter(|v| v.start[2] < 0.0 && v.end[2] < 0.0);
        assert!(below.clone().count() > 0);
        assert!(below.all(
            |v| v.color[3] <= config.shield.rim_alpha * config.shield.below_grid_opacity + 1e-6
        ));
    }

    #[test]
    fn mesh_is_a_closed_sphere() {
        let (vertices, indices) = unit_sphere(MESH_LAT, MESH_LON);
        assert!(
            vertices
                .iter()
                .all(|v| (Vec3::from(*v).length() - 1.0).abs() < 1e-5)
        );
        assert_eq!(indices.len(), (MESH_LAT * MESH_LON * 6) as usize);
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
    }

    #[test]
    fn uniform_layout_matches_the_shader() {
        // 1 mat4 + 10 vec4 + 3 arrays of 4 vec4, all 16-byte aligned.
        assert_eq!(size_of::<ShieldUniforms>(), 64 + 10 * 16 + 3 * 4 * 16);
    }
}
