//! Background pass (phase-1 doc 01 §3): radial gradient, three blurred nebula ellipses, a soft sun
//! glow and about 200 stars, all in screen space.
//!
//! `scene_config.json` only has the gradient's centre and edge colours as numbers; the rest of
//! doc 01 §3 is prose there. Those values are constants below, citing doc 01 §3 or, where the doc
//! gives no number, the mockup (`reference/mockup/ops-grid-proposal.html`) that produced the
//! reference renders.

use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use wgpu::util::DeviceExt;

use super::context::TargetFormat;
use crate::config::{Rgb, SceneConfig};

/// Gradient centre (35%, 25%) and middle stop 55% `#0c1322` (doc 01 §3). Radius 0.95 in canvas
/// units from the mockup's `#dbg` gradient.
const GRADIENT_CENTER: [f32; 2] = [0.35, 0.25];
const GRADIENT_RADIUS: f32 = 0.95;
const GRADIENT_MID_STOP: f32 = 0.55;
const GRADIENT_MID_COLOR: Rgb = Rgb::new(0x0c, 0x13, 0x22);

struct Nebula {
    center_ref_px: [f32; 2],
    radii_ref_px: [f32; 2],
    color: Rgb,
}

/// Colours, opacity and rotation from doc 01 §3; positions, radii and blur from the mockup.
const NEBULAE: [Nebula; 3] = [
    Nebula {
        center_ref_px: [1350.0, 240.0],
        radii_ref_px: [480.0, 150.0],
        color: Rgb::new(0x2a, 0x4a, 0x5a),
    },
    Nebula {
        center_ref_px: [420.0, 180.0],
        radii_ref_px: [420.0, 130.0],
        color: Rgb::new(0x28, 0x3a, 0x55),
    },
    Nebula {
        center_ref_px: [1500.0, 900.0],
        radii_ref_px: [380.0, 140.0],
        color: Rgb::new(0x1e, 0x2f, 0x45),
    },
];
const NEBULA_OPACITY: f32 = 0.35;
const NEBULA_ROTATION_DEG: f32 = -15.0;
const NEBULA_BLUR_SIGMA_REF_PX: f32 = 80.0;

/// Sun glow, top-left (doc 01 §3); geometry and stops from the mockup's `#dsun`.
const SUN_CENTER_REF_PX: [f32; 2] = [40.0, 20.0];
const SUN_RADIUS_REF_PX: f32 = 380.0;
const SUN_OPACITY: f32 = 0.6;
const SUN_INNER: (Rgb, f32) = (Rgb::new(0xff, 0xf1, 0xd6), 0.55);
const SUN_MID: (Rgb, f32) = (Rgb::new(0xff, 0xd9, 0xa0), 0.16);
const SUN_MID_STOP: f32 = 0.25;
const SUN_OUTER: (Rgb, f32) = (Rgb::new(0xff, 0xd9, 0xa0), 0.0);

/// About 200 stars, radius 0.5–1.1 px, alpha 0.15–0.6 (doc 01 §3, `scene_config.json`).
/// Colour from the mockup.
pub const STAR_COUNT: usize = 200;
const STAR_RADIUS_REF_PX: (f32, f32) = (0.5, 1.1);
const STAR_ALPHA: (f32, f32) = (0.15, 0.6);
const STAR_COLOR: Rgb = Rgb::new(0xdf, 0xe9, 0xff);
/// Fixed seed so the star field is the same on every run and in screenshots.
const STAR_SEED: u64 = 0x4e4f_4d41_4453_5441;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BackgroundUniforms {
    viewport_offset: [f32; 4],
    canvas: [f32; 4],
    gradient: [f32; 4],
    color_center: [f32; 4],
    color_mid: [f32; 4],
    color_edge: [f32; 4],
    nebula_shape: [[f32; 4]; 3],
    nebula_color: [[f32; 4]; 3],
    nebula_params: [f32; 4],
    sun_shape: [f32; 4],
    sun_inner: [f32; 4],
    sun_mid: [f32; 4],
    sun_outer: [f32; 4],
    sun_mid_stop: [f32; 4],
    star_color: [f32; 4],
}

/// One star: centre and radius in reference pixels, and alpha.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Star {
    pub center_ref_px: [f32; 2],
    pub radius_ref_px: f32,
    pub alpha: f32,
}

/// `SplitMix64`: tiny, deterministic, and enough for scattering stars.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`, from the top 24 bits.
    fn next_unit(&mut self) -> f32 {
        let bits = u32::try_from(self.next_u64() >> 40).unwrap_or_default();
        bits as f32 / (1u32 << 24) as f32
    }
}

/// The star field on the reference canvas.
pub fn generate_stars(reference_size: Vec2) -> Vec<Star> {
    let mut rng = SplitMix64(STAR_SEED);
    let lerp = |(lo, hi): (f32, f32), t: f32| lo + (hi - lo) * t;
    (0..STAR_COUNT)
        .map(|_| Star {
            center_ref_px: [
                rng.next_unit() * reference_size.x,
                rng.next_unit() * reference_size.y,
            ],
            radius_ref_px: lerp(STAR_RADIUS_REF_PX, rng.next_unit()),
            alpha: lerp(STAR_ALPHA, rng.next_unit()),
        })
        .collect()
}

/// Window pixels per reference pixel and the window position of the canvas origin, for a canvas
/// scaled to cover the whole window.
pub fn cover_fit(viewport_px: Vec2, reference_size: Vec2) -> (f32, Vec2) {
    let scale = (viewport_px / reference_size).max_element();
    (scale, (viewport_px - reference_size * scale) * 0.5)
}

pub struct BackgroundPass {
    uniforms: BackgroundUniforms,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    gradient_pipeline: wgpu::RenderPipeline,
    star_pipeline: wgpu::RenderPipeline,
    star_buffer: wgpu::Buffer,
    star_count: u32,
    reference_size: Vec2,
}

impl BackgroundUniforms {
    /// Everything but the viewport, which [`BackgroundPass::prepare`] fills in per frame.
    fn new(config: &SceneConfig, reference_size: Vec2) -> Self {
        let (sin_rot, cos_rot) = NEBULA_ROTATION_DEG.to_radians().sin_cos();
        let stop = |(color, alpha): (Rgb, f32)| color.to_vec4(alpha);
        Self {
            viewport_offset: [reference_size.x, reference_size.y, 0.0, 0.0],
            canvas: [1.0, reference_size.x, reference_size.y, 0.0],
            gradient: [
                GRADIENT_CENTER[0],
                GRADIENT_CENTER[1],
                GRADIENT_RADIUS,
                GRADIENT_MID_STOP,
            ],
            color_center: config.background.color_center.to_vec4(1.0),
            color_mid: GRADIENT_MID_COLOR.to_vec4(1.0),
            color_edge: config.background.color_edge.to_vec4(1.0),
            nebula_shape: NEBULAE.map(|n| {
                [
                    n.center_ref_px[0],
                    n.center_ref_px[1],
                    n.radii_ref_px[0],
                    n.radii_ref_px[1],
                ]
            }),
            nebula_color: NEBULAE.map(|n| n.color.to_vec4(NEBULA_OPACITY)),
            nebula_params: [cos_rot, sin_rot, NEBULA_BLUR_SIGMA_REF_PX, 0.0],
            sun_shape: [
                SUN_CENTER_REF_PX[0],
                SUN_CENTER_REF_PX[1],
                SUN_RADIUS_REF_PX,
                SUN_OPACITY,
            ],
            sun_inner: stop(SUN_INNER),
            sun_mid: stop(SUN_MID),
            sun_outer: stop(SUN_OUTER),
            sun_mid_stop: [SUN_MID_STOP, 0.0, 0.0, 0.0],
            star_color: STAR_COLOR.to_vec4(1.0),
        }
    }
}

impl BackgroundPass {
    pub fn new(device: &wgpu::Device, target: TargetFormat, config: &SceneConfig) -> Self {
        let [ref_w, ref_h] = config.reference_resolution;
        let reference_size = Vec2::new(ref_w as f32, ref_h as f32);
        let uniforms = BackgroundUniforms::new(config, reference_size);
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("background uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("background"),
            entries: &[super::uniform_entry(
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                size_of::<BackgroundUniforms>(),
            )],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("background"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("background"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("background.wgsl"),
            source: wgpu::ShaderSource::Wgsl(super::BACKGROUND_WGSL.into()),
        });

        let gradient_pipeline = super::fullscreen_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "fs_background",
            target,
            None,
            "background",
        );

        let star_pipeline = star_pipeline(device, &pipeline_layout, &shader, target);
        let stars = generate_stars(reference_size);
        let star_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stars"),
            contents: bytemuck::cast_slice(&stars),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            uniforms,
            uniform_buffer,
            bind_group,
            gradient_pipeline,
            star_pipeline,
            star_buffer,
            star_count: u32::try_from(stars.len()).unwrap_or(0),
            reference_size,
        }
    }

    /// Updates the uniforms for a viewport size.
    pub fn prepare(&mut self, queue: &wgpu::Queue, viewport_px: Vec2) {
        let (scale, offset) = cover_fit(viewport_px, self.reference_size);
        self.uniforms.viewport_offset = [viewport_px.x, viewport_px.y, offset.x, offset.y];
        self.uniforms.canvas[0] = scale;
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&self.uniforms));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.gradient_pipeline);
        pass.draw(0..3, 0..1);
        // Stars come after the sun glow here, the mockup draws them before it. The glow is at
        // most 33% opaque and covers only a corner, so the difference is not visible.
        pass.set_pipeline(&self.star_pipeline);
        pass.set_vertex_buffer(0, self.star_buffer.slice(..));
        pass.draw(0..6, 0..self.star_count);
    }
}

/// Instanced star quads, alpha-blended over the gradient.
fn star_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    target: TargetFormat,
) -> wgpu::RenderPipeline {
    let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Float32];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("stars"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_star"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Star>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(super::context::depth_stencil(false, None)),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_star"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target.color,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: Vec2 = Vec2::new(1920.0, 1080.0);

    #[test]
    fn stars_follow_the_spec_and_are_deterministic() {
        let stars = generate_stars(REFERENCE);
        assert_eq!(stars.len(), STAR_COUNT);
        for star in &stars {
            let [x, y] = star.center_ref_px;
            assert!((0.0..1920.0).contains(&x) && (0.0..1080.0).contains(&y));
            assert!((0.5..=1.1).contains(&star.radius_ref_px));
            assert!((0.15..=0.6).contains(&star.alpha));
        }
        assert_eq!(stars, generate_stars(REFERENCE));
        // Not all piled up: every quarter of the canvas gets some.
        for (qx, qy) in [(0.0, 0.0), (960.0, 0.0), (0.0, 540.0), (960.0, 540.0)] {
            let count = stars
                .iter()
                .filter(|s| {
                    let [x, y] = s.center_ref_px;
                    (qx..qx + 960.0).contains(&x) && (qy..qy + 540.0).contains(&y)
                })
                .count();
            assert!(count > 25, "quarter ({qx}, {qy}) has {count} stars");
        }
    }

    #[test]
    fn cover_fit_fills_the_window() {
        let (scale, offset) = cover_fit(REFERENCE, REFERENCE);
        assert!((scale - 1.0).abs() < 1e-6 && offset.abs().max_element() < 1e-6);
        // Ultra-wide: scaled by width, cropped top and bottom.
        let (scale, offset) = cover_fit(Vec2::new(3840.0, 1080.0), REFERENCE);
        assert!((scale - 2.0).abs() < 1e-6);
        assert!((offset - Vec2::new(0.0, -540.0)).abs().max_element() < 1e-4);
    }

    #[test]
    fn uniform_layout_is_a_whole_number_of_vec4s() {
        assert_eq!(size_of::<BackgroundUniforms>() % 16, 0);
        assert_eq!(size_of::<BackgroundUniforms>(), 19 * 16);
    }
}
