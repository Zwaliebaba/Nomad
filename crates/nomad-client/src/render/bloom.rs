//! Bloom on emissive surfaces (phase-1 doc 01 §2; doc 05 `render/bloom.rs`, frame order step 8).
//! The shader is `shaders/bloom.wgsl`.
//!
//! Doc 01 §2: emissive palette entries "feed a bloom/glow pass (small radius, ~6 px at zoom 1,
//! intensity modest)". The scene pass writes emissive colours into their own target
//! (`render::context`, "Emissive target"), with black wherever anything else is in front, so that
//! image is the bright pass and nothing that is not emissive can glow. Then:
//!
//! 1. **Downsample** it to half resolution (a 2×2 mean).
//! 2. **Blur** it with a separable Gaussian, horizontally and then vertically. The radius is
//!    `bloom.radius_ref_px` (6) in 1080p pixels at zoom 1 and scales with the zoom and the window,
//!    like the voxels; the Gaussian sigma is half the radius, so the glow fades out by about the
//!    radius. Taps reach three sigma.
//! 3. **Composite**: add the blurred image times `bloom.intensity` to the resolved frame, either
//!    plainly (`additive`) or scaled by the headroom left in the frame (`screen`,
//!    `frame + glow · (1 − frame)`). The shipped config uses `screen`: the pylon emitters'
//!    `#b8f6ff` is so pale that a plain add clips it to white even at a low intensity, while the
//!    reference renders keep it cyan with a halo around it (phase-1 doc 07 item 22). Both only
//!    ever add light.
//!
//! Pixels with no emissive surface within reach of the kernel get exactly zero added, so the
//! colours of non-emissive voxels are unchanged away from the emitters (checked by
//! `bloom_brightens_only_around_emitters`). Later milestones add their FX glow to the same
//! emissive target ("Bloom on emissive + FX").

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::context::EMISSIVE_FORMAT;
use crate::camera::ViewTransform;
use crate::config::{BloomComposite, BloomConfig};

/// Most taps on each side of the centre in one blur pass. The default radius needs 18 at zoom 2
/// on a 4K window.
pub const MAX_BLUR_TAPS: u32 = 48;

/// The Gaussian of one frame, in half-resolution texels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurKernel {
    pub sigma_texels: f32,
    pub taps: u32,
}

impl BlurKernel {
    /// Kernel for a view: radius `radius_ref_px` at zoom 1 and 1080p, scaled by zoom and window.
    pub fn new(config: BloomConfig, view: &ViewTransform) -> Self {
        let radius_px = config.radius_ref_px * view.zoom * view.layout_scale;
        // Sigma is half the radius, and the blur runs at half resolution.
        let sigma_texels = radius_px * 0.5 * 0.5;
        let taps = (3.0 * sigma_texels).ceil().clamp(0.0, MAX_BLUR_TAPS as f32);
        Self {
            sigma_texels,
            taps: whole_taps(taps),
        }
    }
}

/// A tap count already clamped to `0..=MAX_BLUR_TAPS`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=MAX_BLUR_TAPS"
)]
fn whole_taps(taps: f32) -> u32 {
    taps as u32
}

/// The composite's blend: the glow is added to the frame, plainly or scaled by `1 − frame`
/// (module docs). The frame's alpha stays as it is (opaque).
fn glow_blend(composite: BloomComposite) -> wgpu::BlendState {
    let src_factor = match composite {
        BloomComposite::Additive => wgpu::BlendFactor::One,
        BloomComposite::Screen => wgpu::BlendFactor::OneMinusDst,
    };
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BloomUniforms {
    params: [f32; 4],
    target_size: [f32; 4],
}

/// Half-resolution textures and the bind groups that read them, for one frame size.
struct BloomTargets {
    half_size: (u32, u32),
    half_a: wgpu::TextureView,
    half_b: wgpu::TextureView,
    /// Downsample (reads the emissive image), blur H (reads A), blur V (reads B), composite
    /// (reads A).
    bind_groups: [wgpu::BindGroup; 4],
}

pub struct BloomPass {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    downsample: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    /// One uniform buffer per step, in the order of `BloomTargets::bind_groups`.
    uniforms: [wgpu::Buffer; 4],
    targets: Option<BloomTargets>,
    config: BloomConfig,
}

impl BloomPass {
    /// `frame_format` is the format of the frame the glow is added to.
    pub fn new(
        device: &wgpu::Device,
        frame_format: wgpu::TextureFormat,
        config: BloomConfig,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom"),
            entries: &[
                super::uniform_entry(wgpu::ShaderStages::FRAGMENT, size_of::<BloomUniforms>()),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom.wgsl"),
            source: wgpu::ShaderSource::Wgsl(super::BLOOM_WGSL.into()),
        });
        let pipeline = |entry: &str, format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let uniform = |label| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(&BloomUniforms::zeroed()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        Self {
            downsample: pipeline("fs_downsample", EMISSIVE_FORMAT, None),
            blur: pipeline("fs_blur", EMISSIVE_FORMAT, None),
            composite: pipeline(
                "fs_composite",
                frame_format,
                Some(glow_blend(config.composite)),
            ),
            uniforms: [
                uniform("bloom downsample"),
                uniform("bloom blur horizontal"),
                uniform("bloom blur vertical"),
                uniform("bloom composite"),
            ],
            layout,
            sampler,
            targets: None,
            config,
        }
    }

    /// Recreates the half-resolution textures for a new frame size. `emissive` is the resolved
    /// emissive image of the scene pass at that size.
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        emissive: &wgpu::TextureView,
        (width, height): (u32, u32),
    ) {
        let half_size = (width.div_ceil(2).max(1), height.div_ceil(2).max(1));
        let half = |label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: half_size.0,
                        height: half_size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: EMISSIVE_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let half_a = half("bloom A");
        let half_b = half("bloom B");
        let bind_group = |uniforms: &wgpu::Buffer, source: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        };
        let bind_groups = [
            bind_group(&self.uniforms[0], emissive),
            bind_group(&self.uniforms[1], &half_a),
            bind_group(&self.uniforms[2], &half_b),
            bind_group(&self.uniforms[3], &half_a),
        ];
        self.targets = Some(BloomTargets {
            half_size,
            half_a,
            half_b,
            bind_groups,
        });
    }

    /// Uploads this frame's kernel and sizes. Call after [`Self::resize`].
    pub fn prepare(&self, queue: &wgpu::Queue, view: &ViewTransform) {
        let Some(targets) = &self.targets else {
            return;
        };
        let kernel = BlurKernel::new(self.config, view);
        let (hw, hh) = (targets.half_size.0 as f32, targets.half_size.1 as f32);
        let half = [hw, hh, 0.0, 0.0];
        let full = [
            view.viewport_px.x,
            view.viewport_px.y,
            self.config.intensity,
            0.0,
        ];
        let blur = |step: [f32; 2]| BloomUniforms {
            params: [step[0], step[1], kernel.sigma_texels, kernel.taps as f32],
            target_size: half,
        };
        let steps = [
            BloomUniforms {
                params: [0.0; 4],
                target_size: half,
            },
            blur([1.0 / hw, 0.0]),
            blur([0.0, 1.0 / hh]),
            BloomUniforms {
                params: [0.0; 4],
                target_size: full,
            },
        ];
        for (buffer, uniforms) in self.uniforms.iter().zip(&steps) {
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(uniforms));
        }
    }

    /// Encodes the downsample and the two blur passes. Run after the scene pass.
    pub fn encode_blur(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(targets) = &self.targets else {
            return;
        };
        let steps = [
            ("bloom downsample", &self.downsample, &targets.half_a, 0),
            ("bloom blur horizontal", &self.blur, &targets.half_b, 1),
            ("bloom blur vertical", &self.blur, &targets.half_a, 2),
        ];
        for (label, pipeline, destination, bind_group) in steps {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // The full-screen triangle overwrites every pixel.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &targets.bind_groups[bind_group], &[]);
            pass.set_pipeline(pipeline);
            pass.draw(0..3, 0..1);
        }
    }

    /// Adds the blurred glow to the frame, inside a pass that draws on the resolved frame.
    pub fn draw_composite(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(targets) = &self.targets else {
            return;
        };
        pass.set_bind_group(0, &targets.bind_groups[3], &[]);
        pass.set_pipeline(&self.composite);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::camera::{Camera, Projection};
    use crate::config::{load_scene_config, phase1_fixture_root, scene_config_path};

    /// Doc 01 §2: about 6 px at zoom 1 on a 1080p window; it scales with zoom and window size.
    #[test]
    fn kernel_scales_with_zoom_and_window() {
        let config = load_scene_config(&scene_config_path(&phase1_fixture_root())).unwrap();
        let projection = Projection::from_config(&config);
        let kernel = |size: Vec2, zoom_index: usize| {
            let mut camera = Camera::new(projection.clone(), size);
            camera.set_zoom_index(zoom_index);
            BlurKernel::new(config.bloom, &camera.transform())
        };
        let hd = Vec2::new(1920.0, 1080.0);
        // Zoom 1 at 1080p: radius 6 px, sigma 3 px, 1.5 half-resolution texels, taps to 3 sigma.
        let base = kernel(hd, 2);
        assert!((base.sigma_texels - 1.5).abs() < 1e-6);
        assert_eq!(base.taps, 5);
        let zoomed = kernel(hd, 4);
        assert!((zoomed.sigma_texels - 3.0).abs() < 1e-6);
        let uhd = kernel(Vec2::new(3840.0, 2160.0), 4);
        assert!((uhd.sigma_texels - 6.0).abs() < 1e-6);
        assert_eq!(uhd.taps, 18);
        assert!(uhd.taps <= MAX_BLUR_TAPS);
    }

    #[test]
    fn uniform_layout_matches_the_shader() {
        assert_eq!(size_of::<BloomUniforms>(), 32);
    }
}
