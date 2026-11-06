//! Renderer (phase-1 doc 05). M1 draws the background and the ops grid; the frame order of doc 05
//! continues with voxels, ships, overlays, shield, FX, bloom and HUD in later milestones.

pub mod background;
pub mod context;
pub mod grid;

use std::num::NonZeroU64;
use std::sync::mpsc;

use glam::Vec2;

use crate::camera::ViewTransform;
use crate::config::{OpsGridConfig, SceneConfig};
use background::BackgroundPass;
use context::{Gpu, GpuError};
use grid::{GridPass, GridUniforms};

pub const BACKGROUND_WGSL: &str = concat!(
    include_str!("shaders/fullscreen.wgsl"),
    include_str!("shaders/background.wgsl")
);
pub const GRID_WGSL: &str = concat!(
    include_str!("shaders/fullscreen.wgsl"),
    include_str!("shaders/grid.wgsl")
);

/// Which optional layers to draw. Tests use it to isolate the grid from the background.
#[derive(Clone, Copy, Debug)]
pub struct Layers {
    pub grid: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self { grid: true }
    }
}

/// An RGBA8 image, sRGB-encoded, rows top to bottom.
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    #[cfg(test)]
    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    background: BackgroundPass,
    grid: GridPass,
    grid_config: OpsGridConfig,
}

impl Renderer {
    /// `format` is the format of every target this renderer draws into.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, config: &SceneConfig) -> Self {
        Self {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            format,
            background: BackgroundPass::new(&gpu.device, format, config),
            grid: GridPass::new(&gpu.device, format),
            grid_config: config.ops_grid.clone(),
        }
    }

    /// Draws one frame into `target`, whose size must match `view.viewport_px`.
    pub fn render(&mut self, target: &wgpu::TextureView, view: &ViewTransform, layers: Layers) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        self.encode(&mut encoder, target, view, layers);
        self.queue.submit([encoder.finish()]);
    }

    fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: &ViewTransform,
        layers: Layers,
    ) {
        self.background.prepare(&self.queue, view.viewport_px);
        self.grid
            .prepare(&self.queue, &GridUniforms::new(&self.grid_config, view));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        // Doc 05 frame order: 1 background, 2 ops grid.
        self.background.draw(&mut pass);
        if layers.grid {
            self.grid.draw(&mut pass);
        }
    }

    /// Renders one frame offscreen and reads it back (F12 and `--screenshot`).
    pub fn render_to_image(
        &mut self,
        width: u32,
        height: u32,
        view: &ViewTransform,
        layers: Layers,
    ) -> Result<RgbaImage, GpuError> {
        debug_assert_eq!(view.viewport_px, Vec2::new(width as f32, height as f32));
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen frame"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded_row = width * 4;
        let row = unpadded_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("offscreen frame"),
            });
        self.encode(&mut encoder, &target, view, layers);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);

        let (sender, receiver) = mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            // The receiver outlives the poll below; a failed send only means it has gone.
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        receiver
            .recv()
            .map_err(|e| GpuError::Readback(e.to_string()))?
            .map_err(|e| GpuError::Readback(e.to_string()))?;

        let mut pixels = Vec::with_capacity(unpadded_row as usize * height as usize);
        {
            let mapped = readback
                .get_mapped_range(..)
                .map_err(|e| GpuError::Readback(e.to_string()))?;
            for line in mapped.chunks_exact(row as usize) {
                pixels.extend_from_slice(&line[..unpadded_row as usize]);
            }
        }
        readback.unmap();
        if matches!(
            self.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for px in pixels.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        Ok(RgbaImage {
            width,
            height,
            pixels,
        })
    }
}

/// A uniform buffer binding of `size` bytes at binding 0.
fn uniform_entry(visibility: wgpu::ShaderStages, size: usize) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: u64::try_from(size).ok().and_then(NonZeroU64::new),
        },
        count: None,
    }
}

/// A pipeline drawing one full-screen triangle with `vs_fullscreen` and the given fragment entry.
fn fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_fullscreen"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, Projection};
    use crate::config::{default_asset_root, load_scene_config, scene_config_path};

    /// Shaders are validated without a GPU, so CI catches WGSL errors.
    #[test]
    fn shaders_parse_and_validate() {
        use wgpu::naga::valid::{Capabilities, ValidationFlags, Validator};
        for (name, source) in [("background", BACKGROUND_WGSL), ("grid", GRID_WGSL)] {
            let module = wgpu::naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|e| panic!("{name}.wgsl: {}", e.emit_to_string(source)));
            Validator::new(ValidationFlags::all(), Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name}.wgsl: {}", e.emit_to_string(source)));
        }
    }

    fn scene() -> SceneConfig {
        load_scene_config(&scene_config_path(&default_asset_root())).unwrap()
    }

    /// Pixels along each screen edge where the grid changed the image.
    fn grid_pixels_per_edge(with: &RgbaImage, without: &RgbaImage) -> [usize; 4] {
        let (w, h) = (with.width, with.height);
        let differs = |x: u32, y: u32| {
            let a = with.pixel(x, y);
            let b = without.pixel(x, y);
            (0..3).any(|c| a[c].abs_diff(b[c]) >= 3)
        };
        let strip = 2;
        let top = (0..w)
            .filter(|&x| (0..strip).any(|y| differs(x, y)))
            .count();
        let bottom = (0..w)
            .filter(|&x| (h - strip..h).any(|y| differs(x, y)))
            .count();
        let left = (0..h)
            .filter(|&y| (0..strip).any(|x| differs(x, y)))
            .count();
        let right = (0..h)
            .filter(|&y| (w - strip..w).any(|x| differs(x, y)))
            .count();
        [top, bottom, left, right]
    }

    /// M1 acceptance: the grid reaches every screen edge at every zoom level, in every view.
    /// Run with `cargo test -p nomad-client -- --ignored` on a machine with a GPU or a software
    /// Vulkan driver (Mesa lavapipe).
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn grid_reaches_every_screen_edge() {
        let scene = scene();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &scene);
        let (w, h) = (960, 540);
        let mut camera = Camera::new(
            Projection::from_config(&scene),
            Vec2::new(w as f32, h as f32),
        );
        for view in 0..4 {
            camera.set_view(view);
            for zoom_index in 0..scene.camera.zoom_levels.len() {
                camera.set_zoom_index(zoom_index);
                let transform = camera.transform();
                let with = renderer
                    .render_to_image(w, h, &transform, Layers::default())
                    .unwrap();
                let without = renderer
                    .render_to_image(w, h, &transform, Layers { grid: false })
                    .unwrap();
                let counts = grid_pixels_per_edge(&with, &without);
                for (edge, count) in ["top", "bottom", "left", "right"].iter().zip(counts) {
                    assert!(
                        count >= 8,
                        "view {view} zoom index {zoom_index}: only {count} grid pixels on the {edge} edge"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn background_matches_the_spec_colours() {
        let scene = scene();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Bgra8Unorm, &scene);
        let camera = Camera::new(Projection::from_config(&scene), Vec2::new(1920.0, 1080.0));
        let image = renderer
            .render_to_image(1920, 1080, &camera.transform(), Layers { grid: false })
            .unwrap();
        // Bottom-right corner lies beyond the gradient radius: the edge colour #060a12. The
        // nearest nebula adds well under one level there. (Checks the BGRA → RGBA swizzle too.)
        let [r, g, b, a] = image.pixel(1917, 1077);
        assert!(r.abs_diff(0x06) <= 2 && g.abs_diff(0x0a) <= 2 && b.abs_diff(0x12) <= 2);
        assert_eq!(a, 255);
    }
}
