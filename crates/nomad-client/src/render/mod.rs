//! Renderer (phase-1 doc 05). The frame order of doc 05 so far:
//!
//! 1–3, 5 and 6, in one multisampled **scene pass** with a depth buffer: the background, the ops
//! grid, the opaque station voxels, the elevation overlays (M3) and the shield (M4: back faces,
//! the far half of the sector ring, front faces, then the near half and the rim; `shield`).
//! Every pipeline in it writes two colour targets, the frame and the emissive image (`context`,
//! "Emissive target").
//!
//! 8. **Bloom** (M3): the resolved emissive image is downsampled and blurred, then added to the
//!    resolved frame.
//!
//! 9. **World labels** (M3: the hovered module's `ELEV +n` tag), screen space, on the resolved
//!    frame after the bloom so text does not glow.
//!
//! Ships (4), FX (7) and the HUD (10) follow in later milestones.

pub mod background;
pub mod bloom;
pub mod context;
pub mod grid;
pub mod labels;
pub mod lines;
pub mod overlay;
pub mod shield;
pub mod voxels;

use std::num::NonZeroU64;
use std::sync::mpsc;

use glam::Vec2;

use crate::assets::SceneAssets;
use crate::camera::ViewTransform;
use crate::config::OpsGridConfig;
use background::BackgroundPass;
use bloom::BloomPass;
use context::{EMISSIVE_FORMAT, FrameTargets, Gpu, GpuError, TargetFormat};
use grid::{GridPass, GridUniforms};
use labels::LabelPass;
use overlay::OverlayPass;
use shield::ShieldPass;
use voxels::VoxelPass;

pub const BACKGROUND_WGSL: &str = concat!(
    include_str!("shaders/fullscreen.wgsl"),
    include_str!("shaders/scene_out.wgsl"),
    include_str!("shaders/background.wgsl")
);
pub const GRID_WGSL: &str = concat!(
    include_str!("shaders/fullscreen.wgsl"),
    include_str!("shaders/scene_out.wgsl"),
    include_str!("shaders/grid.wgsl")
);
pub const VOXELS_WGSL: &str = concat!(
    include_str!("shaders/scene_out.wgsl"),
    include_str!("shaders/voxels.wgsl")
);
pub const OVERLAY_WGSL: &str = concat!(
    include_str!("shaders/scene_out.wgsl"),
    include_str!("shaders/overlay.wgsl")
);
pub const SHIELD_WGSL: &str = concat!(
    include_str!("shaders/scene_out.wgsl"),
    include_str!("shaders/shield.wgsl")
);
pub const BLOOM_WGSL: &str = concat!(
    include_str!("shaders/fullscreen.wgsl"),
    include_str!("shaders/bloom.wgsl")
);

/// Which layers to draw. Tests use it to isolate one layer from the others.
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent on/off switches, one per layer of the doc 05 frame order"
)]
pub struct Layers {
    pub background: bool,
    pub grid: bool,
    pub voxels: bool,
    /// Elevation footprints and stalks (doc 05 step 5).
    pub overlays: bool,
    /// The shield (doc 05 step 6).
    pub shield: bool,
    /// Glow around emissive surfaces (doc 05 step 8).
    pub bloom: bool,
    /// World labels: the hover tag (doc 05 step 9).
    pub labels: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            background: true,
            grid: true,
            voxels: true,
            overlays: true,
            shield: true,
            bloom: true,
            labels: true,
        }
    }
}

/// Per-frame input other than the camera.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameInput {
    /// The module under the cursor (an index into the station's modules): highlighted, and
    /// tagged with its elevation when lifted (doc 06 M3).
    pub hovered: Option<usize>,
    /// Milliseconds since the scripted effects started: drives the shield's hit loop (doc 06
    /// M4).
    pub elapsed_ms: u64,
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
    target: TargetFormat,
    /// MSAA colour and depth for the last frame size; rebuilt when the size changes.
    frame_targets: Option<FrameTargets>,
    /// What shows where no layer draws (only visible with the background layer off).
    clear_color: wgpu::Color,
    background: BackgroundPass,
    grid: GridPass,
    grid_config: OpsGridConfig,
    voxels: VoxelPass,
    overlay: OverlayPass,
    shield: ShieldPass,
    bloom: BloomPass,
    labels: LabelPass,
    /// `scene_config.json` → `hover.brightness_lift`.
    hover_lift: f32,
}

impl Renderer {
    /// `format` is the format of every target this renderer draws into.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, assets: &SceneAssets) -> Self {
        let config = &assets.config;
        let target = TargetFormat {
            color: format,
            sample_count: gpu.sample_count(format),
        };
        tracing::info!(?format, samples = target.sample_count, "scene pass");
        let elevated = overlay::elevated_modules(&assets.station);
        tracing::info!(modules = elevated.len(), "elevation cues");
        let labels = LabelPass::new(
            &gpu.device,
            &gpu.queue,
            format,
            &assets.hud,
            &assets.fonts,
            elevated.clone(),
        );
        Self {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            target,
            frame_targets: None,
            clear_color: wgpu::Color::BLACK,
            background: BackgroundPass::new(&gpu.device, target, config),
            grid: GridPass::new(&gpu.device, target),
            grid_config: config.ops_grid.clone(),
            voxels: VoxelPass::new(&gpu.device, &gpu.queue, target, &assets.station),
            overlay: OverlayPass::new(&gpu.device, target, elevated, config.elevation),
            shield: ShieldPass::new(&gpu.device, target, config, &assets.fixture),
            bloom: BloomPass::new(&gpu.device, format, config.bloom),
            labels,
            hover_lift: config.hover.brightness_lift,
        }
    }

    /// Draws one frame into `frame`, whose size must match `view.viewport_px`.
    pub fn render(
        &mut self,
        frame: &wgpu::TextureView,
        view: &ViewTransform,
        layers: Layers,
        input: FrameInput,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        self.encode(&mut encoder, frame, view, layers, input);
        self.queue.submit([encoder.finish()]);
    }

    fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        view: &ViewTransform,
        layers: Layers,
        input: FrameInput,
    ) {
        let size = frame.texture().size();
        let size = (size.width, size.height);
        if self.frame_targets.as_ref().map(FrameTargets::size) != Some(size) {
            let targets = FrameTargets::new(&self.device, self.target, size);
            // The bloom reads the emissive image of exactly these targets.
            self.bloom
                .resize(&self.device, targets.emissive_view(), size);
            self.frame_targets = Some(targets);
        }
        let targets = self
            .frame_targets
            .as_ref()
            .expect("invariant: frame targets were created above");

        self.background.prepare(&self.queue, view.viewport_px);
        self.grid
            .prepare(&self.queue, &GridUniforms::new(&self.grid_config, view));
        self.voxels
            .prepare(&self.queue, view, input.hovered, self.hover_lift);
        if layers.overlays {
            self.overlay.prepare(&self.queue, view);
        }
        if layers.shield {
            self.shield.prepare(&self.queue, view, input.elapsed_ms);
        }
        if layers.bloom {
            self.bloom.prepare(&self.queue, view);
        }
        let tagged = input.hovered.filter(|_| layers.labels);
        self.labels.prepare(&self.device, &self.queue, view, tagged);

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &targets.color_attachments(frame, self.clear_color),
                depth_stencil_attachment: Some(targets.depth_attachment()),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Doc 05 frame order: 1 background, 2 ops grid (no depth test or write, so it lies
            // under everything), 3 opaque voxels with depth, 5 elevation overlays and 6 the
            // shield (depth test, no depth write).
            if layers.background {
                self.background.draw(&mut pass);
            }
            if layers.grid {
                self.grid.draw(&mut pass);
            }
            if layers.voxels {
                self.voxels.draw(&mut pass);
            }
            if layers.overlays {
                self.overlay.draw(&mut pass);
            }
            if layers.shield {
                self.shield.draw(&mut pass);
            }
        }

        // 8 bloom, then 9 world labels, on the resolved frame.
        if layers.bloom {
            self.bloom.encode_blur(encoder);
        }
        let draw_labels = self.labels.has_text();
        if layers.bloom || draw_labels {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if layers.bloom {
                self.bloom.draw_composite(&mut pass);
            }
            if draw_labels {
                self.labels.draw(&mut pass);
            }
        }
    }

    /// Renders one frame offscreen and reads it back (F12 and `--screenshot`).
    pub fn render_to_image(
        &mut self,
        width: u32,
        height: u32,
        view: &ViewTransform,
        layers: Layers,
        input: FrameInput,
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
            format: self.target.color,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let frame = texture.create_view(&wgpu::TextureViewDescriptor::default());

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
        self.encode(&mut encoder, &frame, view, layers, input);
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
            self.target.color,
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

    #[cfg(test)]
    fn set_clear_color(&mut self, color: wgpu::Color) {
        self.clear_color = color;
    }

    /// Whether the hover tag's font is installed.
    #[cfg(test)]
    fn has_label_font(&self) -> bool {
        self.labels.has_font()
    }
}

/// The two colour targets of every scene-pass pipeline: the frame and the emissive image
/// (`context`, "Emissive target"). Both share one blend state and write mask, because the GLES
/// fallback cannot blend attachments independently (wgpu's `INDEPENDENT_BLEND`); shaders write
/// zero into the emissive target where nothing glows (`shaders/scene_out.wgsl`).
fn scene_targets(
    target: TargetFormat,
    blend: Option<wgpu::BlendState>,
) -> [Option<wgpu::ColorTargetState>; 2] {
    let state = |format| {
        Some(wgpu::ColorTargetState {
            format,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        })
    };
    [state(target.color), state(EMISSIVE_FORMAT)]
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
/// It neither tests nor writes depth (see [`context::depth_stencil`]).
fn fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &str,
    target: TargetFormat,
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
        depth_stencil: Some(context::depth_stencil(false, None)),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &scene_targets(target, blend),
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::assets::voxel::StationScene;
    use crate::camera::{Camera, Projection, Turn};
    use crate::config::{SceneConfig, default_asset_root};

    /// Shaders are validated without a GPU, so CI catches WGSL errors.
    #[test]
    fn shaders_parse_and_validate() {
        use wgpu::naga::valid::{Capabilities, ValidationFlags, Validator};
        for (name, source) in [
            ("background", BACKGROUND_WGSL),
            ("grid", GRID_WGSL),
            ("voxels", VOXELS_WGSL),
            ("overlay", OVERLAY_WGSL),
            ("shield", SHIELD_WGSL),
            ("bloom", BLOOM_WGSL),
        ] {
            let module = wgpu::naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|e| panic!("{name}.wgsl: {}", e.emit_to_string(source)));
            Validator::new(ValidationFlags::all(), Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name}.wgsl: {}", e.emit_to_string(source)));
        }
    }

    /// The shipped assets, with whatever fonts are installed locally.
    fn assets() -> SceneAssets {
        SceneAssets::load(&default_asset_root()).unwrap()
    }

    fn renderer(format: wgpu::TextureFormat) -> (Renderer, SceneConfig) {
        let assets = assets();
        let gpu = Gpu::headless().expect("a GPU adapter");
        (Renderer::new(&gpu, format, &assets), assets.config)
    }

    /// The layers up to the voxels, without the M3 additions.
    const M2_LAYERS: Layers = Layers {
        background: true,
        grid: true,
        voxels: true,
        overlays: false,
        shield: false,
        bloom: false,
        labels: false,
    };

    const NO_INPUT: FrameInput = FrameInput {
        hovered: None,
        elapsed_ms: 0,
    };

    fn differs(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
        (0..3).any(|c| a[c].abs_diff(b[c]) > tolerance)
    }

    /// Pixels along each screen edge where the grid changed the image.
    fn grid_pixels_per_edge(with: &RgbaImage, without: &RgbaImage) -> [usize; 4] {
        let (w, h) = (with.width, with.height);
        let changed = |x: u32, y: u32| differs(with.pixel(x, y), without.pixel(x, y), 2);
        let strip = 2;
        let top = (0..w)
            .filter(|&x| (0..strip).any(|y| changed(x, y)))
            .count();
        let bottom = (0..w)
            .filter(|&x| (h - strip..h).any(|y| changed(x, y)))
            .count();
        let left = (0..h)
            .filter(|&y| (0..strip).any(|x| changed(x, y)))
            .count();
        let right = (0..h)
            .filter(|&y| (w - strip..w).any(|x| changed(x, y)))
            .count();
        [top, bottom, left, right]
    }

    /// M1 acceptance: the grid reaches every screen edge at every zoom level, in every view.
    /// Run with `cargo test -p nomad-client -- --ignored` on a machine with a GPU or a software
    /// Vulkan driver (Mesa lavapipe).
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn grid_reaches_every_screen_edge() {
        let (mut renderer, scene) = renderer(wgpu::TextureFormat::Rgba8Unorm);
        let (w, h) = (960, 540);
        let mut camera = Camera::new(
            Projection::from_config(&scene),
            Vec2::new(w as f32, h as f32),
        );
        let grid_only = Layers {
            voxels: false,
            ..M2_LAYERS
        };
        let neither = Layers {
            grid: false,
            ..grid_only
        };
        for view in 0..4 {
            camera.set_view(view);
            for zoom_index in 0..scene.camera.zoom_levels.len() {
                camera.set_zoom_index(zoom_index);
                let transform = camera.transform();
                let with = renderer
                    .render_to_image(w, h, &transform, grid_only, NO_INPUT)
                    .unwrap();
                let without = renderer
                    .render_to_image(w, h, &transform, neither, NO_INPUT)
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
        let (mut renderer, scene) = renderer(wgpu::TextureFormat::Bgra8Unorm);
        let camera = Camera::new(Projection::from_config(&scene), Vec2::new(1920.0, 1080.0));
        let layers = Layers {
            grid: false,
            voxels: false,
            ..M2_LAYERS
        };
        let image = renderer
            .render_to_image(1920, 1080, &camera.transform(), layers, NO_INPUT)
            .unwrap();
        // Bottom-right corner lies beyond the gradient radius: the edge colour #060a12. The
        // nearest nebula adds well under one level there. (Checks the BGRA → RGBA swizzle too.)
        let [r, g, b, a] = image.pixel(1917, 1077);
        assert!(r.abs_diff(0x06) <= 2 && g.abs_diff(0x0a) <= 2 && b.abs_diff(0x12) <= 2);
        assert_eq!(a, 255);
    }

    /// Station voxels only, over a flat clear colour.
    fn voxels_over(renderer: &mut Renderer, view: &ViewTransform, clear: wgpu::Color) -> RgbaImage {
        renderer.set_clear_color(clear);
        let layers = Layers {
            background: false,
            grid: false,
            ..M2_LAYERS
        };
        let [width, height] = view.viewport_px.to_array().map(whole_px);
        let image = renderer
            .render_to_image(width, height, view, layers, NO_INPUT)
            .unwrap();
        renderer.set_clear_color(wgpu::Color::BLACK);
        image
    }

    /// A non-negative pixel coordinate as an integer, rounding down.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test coordinates are small, non-negative pixel positions"
    )]
    fn whole_px(value: f32) -> u32 {
        value.max(0.0) as u32
    }

    /// The RGB of the pixel containing a screen position.
    fn rgb_at(image: &RgbaImage, at: Vec2) -> [u8; 3] {
        let [r, g, b, _] = image.pixel(whole_px(at.x), whole_px(at.y));
        [r, g, b]
    }

    /// Pixels fully covered by voxels: identical over a black and a white clear colour.
    fn coverage(over_black: &RgbaImage, over_white: &RgbaImage) -> Vec<bool> {
        over_black
            .pixels
            .chunks_exact(4)
            .zip(over_white.pixels.chunks_exact(4))
            .map(|(b, w)| (0..3).all(|c| b[c] == w[c]))
            .collect()
    }

    /// Pixels the station's voxels cover fully.
    fn station_coverage(renderer: &mut Renderer, view: &ViewTransform) -> Vec<bool> {
        coverage(
            &voxels_over(renderer, view, wgpu::Color::BLACK),
            &voxels_over(renderer, view, wgpu::Color::WHITE),
        )
    }

    /// Doc 06 M2: no holes in any view. Greedy merging creates T-junctions, where one long edge
    /// meets several short ones, and those can rasterise with pinholes (see `render::voxels`).
    /// The reference is the same station meshed without merging: every edge is shared vertex for
    /// vertex, so it is crack-free. (A neighbourhood test alone cannot tell holes from geometry:
    /// the solar array has real one-voxel gaps that thin to a pixel.) Missing or extra faces are
    /// ruled out on the CPU by the `mesh::greedy` tests.
    ///
    /// In the settled views the greedy mesh must fully cover every pixel the reference covers
    /// fully. Mid-turn, a long silhouette edge may miss a sample that a chain of short edges along
    /// the same line covers (a sub-pixel deviation where the background is next to it anyway), so
    /// there only interior pinholes count: a pixel the reference covers whose eight neighbours the
    /// greedy mesh covers but which it does not. Checked in every view and zoom, at 1080p and at
    /// 720p (a non-integer layout scale).
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn station_has_no_holes() {
        let assets = assets();
        let scene = &assets.config;
        let gpu = Gpu::headless().expect("a GPU adapter");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut greedy = Renderer::new(&gpu, format, &assets);
        let mut unmerged = Renderer::new(&gpu, format, &assets);
        let reference_mesh = voxels::build_unmerged_station_mesh(&assets.station);
        unmerged.voxels = VoxelPass::from_mesh(
            &gpu.device,
            &gpu.queue,
            unmerged.target,
            &assets.station,
            &reference_mesh,
        );
        for (width, height) in [(1920_u32, 1080_u32), (1280, 720)] {
            let mut camera = Camera::new(
                Projection::from_config(scene),
                Vec2::new(width as f32, height as f32),
            );
            for view in 0..4 {
                for zoom_index in 0..scene.camera.zoom_levels.len() {
                    for mid_turn in [false, true] {
                        camera.set_view(view);
                        camera.set_zoom_index(zoom_index);
                        if mid_turn {
                            camera.rotate(Turn::Plus90);
                            camera.update(0.1);
                        }
                        let transform = camera.transform();
                        let reference = station_coverage(&mut unmerged, &transform);
                        let actual = station_coverage(&mut greedy, &transform);
                        let covered = |x: u32, y: u32| actual[(y * width + x) as usize];
                        let enclosed = |x: u32, y: u32| {
                            x > 0
                                && y > 0
                                && x + 1 < width
                                && y + 1 < height
                                && (y - 1..=y + 1).all(|ny| {
                                    (x - 1..=x + 1).all(|nx| (nx, ny) == (x, y) || covered(nx, ny))
                                })
                        };
                        let holes: Vec<(u32, u32)> = (0..width * height)
                            .filter(|&i| reference[i as usize] && !actual[i as usize])
                            .map(|i| (i % width, i / width))
                            .filter(|&(x, y)| !mid_turn || enclosed(x, y))
                            .collect();
                        assert!(
                            holes.is_empty(),
                            "{width}x{height} view {view} zoom index {zoom_index} mid-turn \
                             {mid_turn}: {} hole pixels, first {:?}",
                            holes.len(),
                            &holes[..holes.len().min(8)]
                        );
                        assert!(reference.iter().filter(|&&c| c).count() > 1000);
                    }
                }
            }
        }
    }

    /// Doc 05 frame order: the grid lies behind and under the voxels, so it never changes a pixel
    /// the station covers.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn voxels_cover_the_grid() {
        let (mut renderer, scene) = renderer(wgpu::TextureFormat::Rgba8Unorm);
        let (w, h) = (1280, 720);
        let mut camera = Camera::new(
            Projection::from_config(&scene),
            Vec2::new(w as f32, h as f32),
        );
        for view in [0, 3] {
            camera.set_view(view);
            let transform = camera.transform();
            let covered = coverage(
                &voxels_over(&mut renderer, &transform, wgpu::Color::BLACK),
                &voxels_over(&mut renderer, &transform, wgpu::Color::WHITE),
            );
            let with_grid = renderer
                .render_to_image(w, h, &transform, M2_LAYERS, NO_INPUT)
                .unwrap();
            let no_grid = Layers {
                grid: false,
                ..M2_LAYERS
            };
            let without_grid = renderer
                .render_to_image(w, h, &transform, no_grid, NO_INPUT)
                .unwrap();
            let mut grid_visible_elsewhere = false;
            for y in 0..h {
                for x in 0..w {
                    let changed = differs(with_grid.pixel(x, y), without_grid.pixel(x, y), 0);
                    if covered[(y * w + x) as usize] {
                        assert!(
                            !changed,
                            "view {view}: grid shows through voxels at ({x}, {y})"
                        );
                    } else {
                        grid_visible_elsewhere |= changed;
                    }
                }
            }
            assert!(grid_visible_elsewhere);
        }
    }

    /// Doc 01 §2: faces are shaded top 1.00, left-facing 0.77, right-facing 0.48, relative to the
    /// camera, and emissive entries render at full albedo. Checked on a grey and an emissive
    /// 4×4×4 block, in every view.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn faces_are_shaded_relative_to_the_camera() {
        let gpu = Gpu::headless().expect("a GPU adapter");
        let text = serde_json::json!({
            "format": "voxel-rts/scene@1",
            "shading": { "top": 1.0, "left": 0.77, "right": 0.48 },
            "palette": [{ "rgb": "#c8c8c8", "emissive": false }, { "rgb": "#7ed9ee", "emissive": true }],
            "modules": [
                { "id": "grey", "bounds": { "min": [0, 0, 0], "max": [3, 3, 3] },
                  "voxels": (0..4).flat_map(|x| (0..4).flat_map(move |y| (0..4).map(move |z| [x, y, z, 0]))).collect::<Vec<_>>() },
                { "id": "lit", "bounds": { "min": [20, 0, 0], "max": [23, 3, 3] },
                  "voxels": (20..24).flat_map(|x| (0..4).flat_map(move |y| (0..4).map(move |z| [x, y, z, 1]))).collect::<Vec<_>>() }
            ]
        })
        .to_string();
        let station = StationScene::from_json(&text, std::path::Path::new("test.json")).unwrap();
        let assets = SceneAssets {
            station,
            ..assets()
        };
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let (width, height) = (960, 540);
        let mut camera = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        );
        camera.set_zoom_index(4);
        let layers = Layers {
            background: false,
            grid: false,
            ..M2_LAYERS
        };
        let expect = |image: &RgbaImage, at: Vec2, factor: f32, what: &str| {
            let rgb = rgb_at(image, at);
            assert!(
                rgb[0].abs_diff(rgb[1]) <= 1 && rgb[1].abs_diff(rgb[2]) <= 1,
                "{what}: not grey: {rgb:?}"
            );
            let expected = 200.0 * factor;
            assert!(
                (f32::from(rgb[0]) - expected).abs() <= 1.5,
                "{what}: {} instead of {expected}",
                rgb[0]
            );
        };
        for view in 0..4 {
            camera.set_view(view);
            let t = camera.transform();
            let image = renderer
                .render_to_image(width, height, &t, layers, NO_INPUT)
                .unwrap();
            // Each 4×4×4 block shows its top and the two side faces that point against
            // up_ground; one of those faces screen-left, the other screen-right. Sample the
            // projected centre of each face.
            let visible_sides: Vec<Vec3> = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y]
                .into_iter()
                .filter(|n| n.truncate().dot(t.up_ground) < 0.0)
                .collect();
            assert_eq!(visible_sides.len(), 2);
            let grey = Vec3::new(2.0, 2.0, 2.0);
            expect(
                &image,
                t.world_to_screen(grey + Vec3::Z * 2.0),
                1.0,
                &format!("view {view} top"),
            );
            for &normal in &visible_sides {
                let left = normal.truncate().dot(t.right) < 0.0;
                let factor = if left { 0.77 } else { 0.48 };
                let at = t.world_to_screen(grey + normal * 2.0);
                expect(&image, at, factor, &format!("view {view} side {normal}"));
            }
            // The emissive block keeps its full albedo on every visible face.
            let lit = Vec3::new(22.0, 2.0, 2.0);
            for offset in [
                Vec3::Z * 2.0,
                visible_sides[0] * 2.0,
                visible_sides[1] * 2.0,
            ] {
                assert_eq!(
                    rgb_at(&image, t.world_to_screen(lit + offset)),
                    [0x7e, 0xd9, 0xee],
                    "view {view}: emissive {offset}"
                );
            }
        }
    }

    /// Screen bounding box of the world box `[lo, hi]`.
    fn screen_box(t: &ViewTransform, lo: Vec3, hi: Vec3) -> (Vec2, Vec2) {
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for i in 0..8 {
            let pick = |bit: u32, a: f32, b: f32| if i & bit == 0 { a } else { b };
            let p = Vec3::new(
                pick(1, lo.x, hi.x),
                pick(2, lo.y, hi.y),
                pick(4, lo.z, hi.z),
            );
            let s = t.world_to_screen(p);
            min = min.min(s);
            max = max.max(s);
        }
        (min, max)
    }

    /// A mask of the pixels within `margin` of any of `boxes`.
    fn mask_around(boxes: &[(Vec2, Vec2)], margin: f32, width: u32, height: u32) -> Vec<bool> {
        let mut mask = vec![false; (width * height) as usize];
        for &(min, max) in boxes {
            let x0 = whole_px(min.x - margin);
            let y0 = whole_px(min.y - margin);
            let x1 = whole_px(max.x + margin + 1.0).min(width);
            let y1 = whole_px(max.y + margin + 1.0).min(height);
            for y in y0..y1 {
                for x in x0..x1 {
                    mask[(y * width + x) as usize] = true;
                }
            }
        }
        mask
    }

    /// The pixels that differ between two renders, as (x, y).
    fn changed_pixels(a: &RgbaImage, b: &RgbaImage) -> Vec<(u32, u32)> {
        (0..a.height)
            .flat_map(|y| (0..a.width).map(move |x| (x, y)))
            .filter(|&(x, y)| differs(a.pixel(x, y), b.pixel(x, y), 0))
            .collect()
    }

    fn module_index(assets: &SceneAssets, id: &str) -> usize {
        assets
            .station
            .modules
            .iter()
            .position(|m| m.id == id)
            .unwrap()
    }

    /// Doc 01 §2 and doc 06 M3: emissive voxels glow, and everything farther from them than the
    /// blur reaches keeps its M2 colour bit for bit. The glow only ever adds light. Checked in
    /// all four views at 1080p and zoom 1, and at zoom 2.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn bloom_brightens_only_around_emitters() {
        let assets = assets();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let (width, height) = (1920, 1080);
        let mut camera = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        );
        let station = &assets.station;
        let emissive: Vec<Vec3> = station
            .modules
            .iter()
            .flat_map(|m| m.chunk.voxels())
            .filter(|&(_, p)| station.palette[usize::from(p)].emissive)
            .map(|(p, _)| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
            .collect();
        // The emitter column of pylon N: its top voxel.
        let pylon_n = module_index(&assets, "pylon_n");
        let emitter_top = station.modules[pylon_n]
            .chunk
            .voxels()
            .filter(|&(_, p)| station.palette[usize::from(p)].emissive)
            .max_by_key(|(p, _)| p[2])
            .map(|(p, _)| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
            .unwrap();
        let with_bloom = Layers {
            bloom: true,
            ..M2_LAYERS
        };
        for (view, zoom_index) in [(0, 2), (1, 2), (2, 2), (3, 2), (0, 4)] {
            camera.set_view(view);
            camera.set_zoom_index(zoom_index);
            let t = camera.transform();
            let plain = renderer
                .render_to_image(width, height, &t, M2_LAYERS, NO_INPUT)
                .unwrap();
            let glow = renderer
                .render_to_image(width, height, &t, with_bloom, NO_INPUT)
                .unwrap();
            let kernel = bloom::BlurKernel::new(assets.config.bloom, &t);
            // Taps are half-resolution texels; add the half-resolution sampling footprint.
            let reach = kernel.taps as f32 * 2.0 + 3.0;
            let boxes: Vec<_> = emissive
                .iter()
                .map(|&p| screen_box(&t, p, p + Vec3::ONE))
                .collect();
            let near = mask_around(&boxes, reach, width, height);
            let changed = changed_pixels(&plain, &glow);
            for &(x, y) in &changed {
                assert!(
                    near[(y * width + x) as usize],
                    "view {view} zoom index {zoom_index}: bloom changed ({x}, {y}), far from \
                     every emitter"
                );
                let (before, after) = (plain.pixel(x, y), glow.pixel(x, y));
                assert!(
                    (0..3).all(|c| after[c] >= before[c]),
                    "view {view}: bloom darkened ({x}, {y})"
                );
            }
            // The glow is visible, and it stays within the reach mask (checked above), which
            // covers a few percent of the screen at zoom 1.
            assert!(
                changed.len() > 2000,
                "view {view} zoom index {zoom_index}: only {} pixels glow",
                changed.len()
            );
            if zoom_index == 2 {
                assert!(
                    changed.len() < (width * height / 50) as usize,
                    "view {view}: bloom spreads over {} pixels",
                    changed.len()
                );
            }

            // The emitter's top face is brighter, and so is the dark space just above it (the
            // halo), where the frame shows background without bloom.
            let top_face = t.world_to_screen(emitter_top + Vec3::new(0.5, 0.5, 1.0));
            let (face_plain, face_glow) = (rgb_at(&plain, top_face), rgb_at(&glow, top_face));
            assert!(
                face_glow[0] >= face_plain[0].saturating_add(10),
                "view {view}: emitter top {face_plain:?} → {face_glow:?}"
            );
            let (min, _) = screen_box(&t, emitter_top, emitter_top + Vec3::ONE);
            let above = Vec2::new(top_face.x, min.y - 3.0 * t.zoom);
            let (halo_plain, halo_glow) = (rgb_at(&plain, above), rgb_at(&glow, above));
            assert!(
                halo_glow[2] >= halo_plain[2].saturating_add(4),
                "view {view}: no halo above the emitter: {halo_plain:?} → {halo_glow:?}"
            );
        }
    }

    /// Doc 01 §7: a footprint and stalks under FA-1 and FB-2, in every view and halfway through
    /// a turn, cyan, and nothing outside the column under each lifted module.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn elevation_cues_lie_under_the_lifted_modules() {
        let assets = assets();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let (width, height) = (1920, 1080);
        let mut camera = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        );
        let modules = overlay::elevated_modules(&assets.station);
        assert_eq!(modules.len(), 2);
        let with_cues = Layers {
            overlays: true,
            ..M2_LAYERS
        };
        for (view, mid_turn) in [(0, false), (1, false), (2, false), (3, false), (0, true)] {
            camera.set_view(view);
            if mid_turn {
                camera.rotate(Turn::Plus90);
                camera.update(0.125);
            }
            let t = camera.transform();
            let plain = renderer
                .render_to_image(width, height, &t, M2_LAYERS, NO_INPUT)
                .unwrap();
            let cues = renderer
                .render_to_image(width, height, &t, with_cues, NO_INPUT)
                .unwrap();
            let columns: Vec<_> = modules
                .iter()
                .map(|m| {
                    screen_box(
                        &t,
                        m.footprint_min.extend(0.0),
                        m.footprint_max.extend(m.bottom_z),
                    )
                })
                .collect();
            let anywhere = mask_around(&columns, 2.0, width, height);
            let changed = changed_pixels(&plain, &cues);
            let mut tint = [0_i64; 3];
            for &(x, y) in &changed {
                assert!(
                    anywhere[(y * width + x) as usize],
                    "view {view} mid-turn {mid_turn}: overlay pixel ({x}, {y}) outside the columns"
                );
                let (before, after) = (plain.pixel(x, y), cues.pixel(x, y));
                for c in 0..3 {
                    tint[c] += i64::from(after[c]) - i64::from(before[c]);
                }
            }
            // Cyan #38d6ff over the dark grid: blue and green rise, much more than red.
            assert!(
                tint[2] > 2 * tint[0].max(0) && tint[1] > tint[0],
                "view {view}: overlay tint {tint:?} is not cyan"
            );
            for (module, column) in modules.iter().zip(&columns) {
                let inside = mask_around(std::slice::from_ref(column), 0.0, width, height);
                let count = changed
                    .iter()
                    .filter(|&&(x, y)| inside[(y * width + x) as usize])
                    .count();
                assert!(
                    count > 300,
                    "view {view} mid-turn {mid_turn}: only {count} cue pixels under module {}",
                    module.index
                );
            }
        }
    }

    /// Doc 06 M3: hovering FA-1 lifts its faces and shows `ELEV +8` below its footprint (when the
    /// font is installed); hovering a module on the grid lifts it without a tag. Nothing else
    /// changes.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn hover_highlights_the_module_and_tags_lifted_ones() {
        let assets = assets();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let (width, height) = (1920, 1080);
        let camera = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        );
        let t = camera.transform();
        let layers = Layers {
            labels: true,
            ..M2_LAYERS
        };
        let plain = renderer
            .render_to_image(width, height, &t, layers, NO_INPUT)
            .unwrap();
        let module_box = |index: usize| {
            let chunk = &assets.station.modules[index].chunk;
            let lo = Vec3::from(chunk.origin().map(|v| v as f32));
            screen_box(&t, lo, lo + Vec3::from(chunk.size().map(|s| s as f32)))
        };

        let fa1 = module_index(&assets, "factory_fa1");
        let hovered = renderer
            .render_to_image(
                width,
                height,
                &t,
                layers,
                FrameInput {
                    hovered: Some(fa1),
                    ..NO_INPUT
                },
            )
            .unwrap();
        let elevated = overlay::elevated_modules(&assets.station);
        let anchor = elevated
            .iter()
            .find(|m| m.index == fa1)
            .unwrap()
            .tag_anchor(&t);
        let tag_box = (
            anchor + Vec2::new(-40.0, 0.0),
            anchor + Vec2::new(40.0, 20.0),
        );
        let in_module = mask_around(&[module_box(fa1)], 1.0, width, height);
        let in_tag = mask_around(&[tag_box], 0.0, width, height);
        let (mut lifted, mut text) = (0, 0);
        for (x, y) in changed_pixels(&plain, &hovered) {
            let index = (y * width + x) as usize;
            let (before, after) = (plain.pixel(x, y), hovered.pixel(x, y));
            if in_module[index] {
                assert!(
                    (0..3).all(|c| after[c] >= before[c]),
                    "hover darkened ({x}, {y})"
                );
                lifted += 1;
            } else if in_tag[index] {
                text += 1;
            } else {
                panic!("hovering FA-1 changed ({x}, {y}), outside FA-1 and its tag");
            }
        }
        assert!(lifted > 1000, "only {lifted} FA-1 pixels lifted");
        if renderer.has_label_font() {
            assert!(text > 30, "only {text} tag pixels");
        } else {
            assert_eq!(text, 0);
        }

        let core = module_index(&assets, "core_spindle");
        let hovered = renderer
            .render_to_image(
                width,
                height,
                &t,
                layers,
                FrameInput {
                    hovered: Some(core),
                    ..NO_INPUT
                },
            )
            .unwrap();
        let in_core = mask_around(&[module_box(core)], 1.0, width, height);
        let changed = changed_pixels(&plain, &hovered);
        assert!(changed.len() > 1000);
        assert!(
            changed
                .iter()
                .all(|&(x, y)| in_core[(y * width + x) as usize])
        );
    }

    // ---- M4: the shield (doc 06 M4, D-100) ----

    /// Only the shield, over black: lines and fill show as they are drawn.
    const SHIELD_ONLY: Layers = Layers {
        background: false,
        grid: false,
        voxels: false,
        overlays: false,
        shield: true,
        bloom: false,
        labels: false,
    };

    /// A moment of the hit loop with no ripple or flash running (both impacts ended).
    const QUIET_MS: u64 = 1500;

    fn shield_geometry(assets: &SceneAssets) -> shield::Ellipsoid {
        shield::Ellipsoid::new(
            &assets.config.shield,
            assets.config.projection.effective_z_scale(),
        )
    }

    fn at_ms(elapsed_ms: u64) -> FrameInput {
        FrameInput {
            hovered: None,
            elapsed_ms,
        }
    }

    /// Whether `p` lies inside the closed polygon `outline` (even-odd rule).
    fn inside_polygon(outline: &[Vec2], p: Vec2) -> bool {
        let mut inside = false;
        let n = outline.len();
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
        inside
    }

    /// Distance from `p` to the closed polygon's outline.
    fn distance_to_outline(outline: &[Vec2], point: Vec2) -> f32 {
        let count = outline.len();
        (0..count)
            .map(|i| {
                let (start, end) = (outline[i], outline[(i + 1) % count]);
                let edge = end - start;
                let along =
                    ((point - start).dot(edge) / edge.length_squared().max(1e-9)).clamp(0.0, 1.0);
                (start + edge * along).distance(point)
            })
            .fold(f32::INFINITY, f32::min)
    }

    /// The brightest pixel (sum of channels) within `radius` pixels of `at`.
    fn brightest_near(image: &RgbaImage, at: Vec2, radius: u32) -> [u8; 3] {
        let (cx, cy) = (whole_px(at.x), whole_px(at.y));
        let columns = cx.saturating_sub(radius)..=(cx + radius).min(image.width - 1);
        let rows = cy.saturating_sub(radius)..=(cy + radius).min(image.height - 1);
        rows.flat_map(|row| columns.clone().map(move |column| (column, row)))
            .map(|(column, row)| {
                let [r, g, b, _] = image.pixel(column, row);
                [r, g, b]
            })
            .max_by_key(|rgb| rgb.iter().map(|&c| u32::from(c)).sum::<u32>())
            .unwrap_or([0, 0, 0])
    }

    /// D-100 and doc 06 M4: the shield reads as a globe in all four views and mid-turn. The rim
    /// runs all the way round, below the ops grid as well as above the station; the part below
    /// the grid is fainter; and the outline is far from flat.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn shield_reads_as_a_globe_in_every_view() {
        let assets = assets();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let globe = shield_geometry(&assets);
        let (width, height) = (960, 540);
        let mut camera = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        );
        let mut frames = Vec::new();
        for view in 0..4 {
            camera.set_view(view);
            frames.push(camera.transform());
        }
        camera.set_view(0);
        camera.rotate(Turn::Plus90);
        camera.update(0.125);
        frames.push(camera.transform());
        for (i, t) in frames.iter().enumerate() {
            let image = renderer
                .render_to_image(width, height, t, SHIELD_ONLY, at_ms(QUIET_MS))
                .unwrap();
            let rim = globe.silhouette(t, 64);
            let lit = rim
                .iter()
                .filter(|p| brightest_near(&image, t.world_to_screen(**p), 2) != [0, 0, 0])
                .count();
            assert!(lit >= 60, "frame {i}: rim drawn at only {lit} of 64 points");
            let screen: Vec<Vec2> = rim.iter().map(|p| t.world_to_screen(*p)).collect();
            let top = (0..rim.len())
                .min_by(|&a, &b| screen[a].y.total_cmp(&screen[b].y))
                .unwrap();
            let bottom = (0..rim.len())
                .max_by(|&a, &b| screen[a].y.total_cmp(&screen[b].y))
                .unwrap();
            assert!(rim[top].z > 0.0 && rim[bottom].z < 0.0, "frame {i}");
            let brightness = |at: Vec2| -> u32 {
                brightest_near(&image, at, 2)
                    .iter()
                    .map(|&c| u32::from(c))
                    .sum()
            };
            let (above, below) = (brightness(screen[top]), brightness(screen[bottom]));
            assert!(
                below > 0 && below * 10 < above * 7,
                "frame {i}: rim below the grid {below} vs above {above}"
            );
            let (min, max) = screen.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            );
            let size = max - min;
            assert!(size.y / size.x > 0.6, "frame {i}: outline {size} is flat");
        }
    }

    /// D-100: the solar array reads as outside the shield. Its panels lie outside the shield's
    /// volume (only the power bus runs into the station). In views 1–3 the shield covers almost
    /// none of it on screen. In view 0 the array lies behind the globe, which covers most of it
    /// as the reference dome did (doc 07); the rim still crosses it, so part of it shows beyond
    /// the shield.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn solar_array_reads_as_outside_the_shield() {
        let assets = assets();
        let globe = shield_geometry(&assets);
        let solar = module_index(&assets, "solar_array");
        let module = &assets.station.modules[solar];
        let voxels: Vec<Vec3> = module
            .chunk
            .voxels()
            .map(|(v, _)| Vec3::from(v.map(|c| c as f32)) + Vec3::splat(0.5))
            .collect();
        let inside = voxels
            .iter()
            .filter(|v| (((**v - globe.center) / globe.radii).length()) < 1.0)
            .count();
        assert!(
            inside * 20 < voxels.len(),
            "{inside} of {} solar voxels inside the shield",
            voxels.len()
        );

        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let (width, height) = (960, 540);
        let mut camera = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        );
        let with_shield = Layers {
            bloom: false,
            ..Layers::default()
        };
        let no_shield = Layers {
            shield: false,
            ..with_shield
        };
        for view in 0..4 {
            camera.set_view(view);
            let t = camera.transform();
            // The array's pixels: where hovering it changes the frame.
            let plain = renderer
                .render_to_image(width, height, &t, no_shield, at_ms(QUIET_MS))
                .unwrap();
            let hovered = renderer
                .render_to_image(
                    width,
                    height,
                    &t,
                    no_shield,
                    FrameInput {
                        hovered: Some(solar),
                        elapsed_ms: QUIET_MS,
                    },
                )
                .unwrap();
            let array = changed_pixels(&plain, &hovered);
            assert!(
                array.len() > 500,
                "view {view}: {} array pixels",
                array.len()
            );
            // Which of them the shield draws over.
            let shielded = renderer
                .render_to_image(width, height, &t, with_shield, at_ms(QUIET_MS))
                .unwrap();
            let covered = array
                .iter()
                .filter(|&&(x, y)| differs(plain.pixel(x, y), shielded.pixel(x, y), 0))
                .count();
            let share = covered as f32 / array.len() as f32;
            if view == 0 {
                assert!(
                    share < 0.98,
                    "view 0: the shield covers all of the array ({share})"
                );
            } else {
                assert!(
                    share < 0.1,
                    "view {view}: the shield covers {share} of the array"
                );
            }
        }
    }

    /// Doc 01 §6 and doc 06 M4: the E sector, at 38% in the fixture, is drawn in the weak colour
    /// (its ring arc and the rim over it); a healthy sector's arc is cyan.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn weak_sector_is_drawn_in_the_weak_colour() {
        let assets = assets();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let globe = shield_geometry(&assets);
        let (width, height) = (1920, 1080);
        let t = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        )
        .transform();
        let image = renderer
            .render_to_image(width, height, &t, SHIELD_ONLY, at_ms(QUIET_MS))
            .unwrap();
        let ring_at = |az_deg: f32| {
            let a = az_deg.to_radians();
            t.world_to_screen(
                globe.center + Vec3::new(a.cos() * globe.radii.x, a.sin() * globe.radii.y, 0.0),
            )
        };
        let warm = |[r, _, b]: [u8; 3]| r > b + 40;
        let cool = |[r, _, b]: [u8; 3]| b > r + 40;
        // In view 0 the near half of the ring runs from 315° through 0° to 135°: E's near part
        // is 315–360°, S is 0–90°.
        for az in [325.0, 340.0, 355.0] {
            let rgb = brightest_near(&image, ring_at(az), 6);
            assert!(warm(rgb), "E sector ring at {az}°: {rgb:?}");
        }
        for az in [20.0, 45.0, 70.0] {
            let rgb = brightest_near(&image, ring_at(az), 3);
            assert!(cool(rgb), "S sector ring at {az}°: {rgb:?}");
        }
        // The rim: warm over E, cool elsewhere.
        let rim = globe.silhouette(&t, 96);
        let (mut warm_rim, mut cool_rim) = (0, 0);
        for p in &rim {
            let az = globe.azimuth_deg(*p);
            let rgb = brightest_near(&image, t.world_to_screen(*p), 6);
            if (275.0..355.0).contains(&az) && p.z > 0.0 {
                assert!(warm(rgb), "rim over E at {az}°: {rgb:?}");
                warm_rim += 1;
            } else if (5.0..265.0).contains(&az) && p.z > 0.0 {
                assert!(cool(rgb), "rim at {az}°: {rgb:?}");
                cool_rim += 1;
            }
        }
        assert!(warm_rim > 3 && cool_rim > 10);
    }

    /// Doc 06 M4: hit ripples on the scripted loop, at the impact points of the reference. A
    /// running ripple lights the surface around its impact only; between hits the surface is
    /// back at rest.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn hit_ripples_run_on_the_script_at_their_impacts() {
        let assets = assets();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let (width, height) = (1920, 1080);
        let t = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        )
        .transform();
        let hits = &assets.config.shield.hits;
        let quiet = renderer
            .render_to_image(width, height, &t, SHIELD_ONLY, at_ms(QUIET_MS))
            .unwrap();
        for impact in &hits.impacts {
            // A third of the way through this impact's ripple; the other one is over or far
            // away.
            let ms = u64::from(impact.delay_ms) + u64::from(hits.ripple_ms) / 3;
            let image = renderer
                .render_to_image(width, height, &t, SHIELD_ONLY, at_ms(ms))
                .unwrap();
            let at = Vec2::from(impact.reference_px);
            // The ripple's footprint on screen is at most its radius in voxels times the
            // pixels per voxel, plus the flash.
            let reach = (impact.radius_voxels.max(hits.flash_radius_voxels) + 1.0) * t.px_per_unit;
            let changed = changed_pixels(&quiet, &image);
            let near = changed
                .iter()
                .filter(|&&(x, y)| Vec2::new(x as f32, y as f32).distance(at) < reach)
                .count();
            assert!(near > 100, "impact at {at}: only {near} pixels lit");
            let other = hits
                .impacts
                .iter()
                .map(|o| Vec2::from(o.reference_px))
                .filter(|o| *o != at)
                .collect::<Vec<_>>();
            for &(x, y) in &changed {
                let p = Vec2::new(x as f32, y as f32);
                let by_other = other.iter().any(|o| p.distance(*o) < reach);
                assert!(
                    p.distance(at) < reach || by_other,
                    "impact at {at} lit ({x}, {y})"
                );
            }
        }
        // The loop repeats.
        let first = u64::from(hits.impacts[0].delay_ms) + u64::from(hits.ripple_ms) / 3;
        let a = renderer
            .render_to_image(width, height, &t, SHIELD_ONLY, at_ms(first))
            .unwrap();
        let b = renderer
            .render_to_image(
                width,
                height,
                &t,
                SHIELD_ONLY,
                at_ms(first + u64::from(hits.loop_ms)),
            )
            .unwrap();
        assert!(changed_pixels(&a, &b).is_empty());
    }

    /// Adding the shield changes nothing away from it: pixels more than a few pixels outside its
    /// rim are identical to the M3 frame, in every view.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn shield_leaves_the_rest_of_the_frame_alone() {
        let assets = assets();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let globe = shield_geometry(&assets);
        let (width, height) = (960, 540);
        let mut camera = Camera::new(
            Projection::from_config(&assets.config),
            Vec2::new(width as f32, height as f32),
        );
        let m3 = Layers {
            shield: false,
            ..Layers::default()
        };
        for view in 0..4 {
            camera.set_view(view);
            let t = camera.transform();
            let without = renderer
                .render_to_image(width, height, &t, m3, at_ms(QUIET_MS))
                .unwrap();
            let with = renderer
                .render_to_image(width, height, &t, Layers::default(), at_ms(QUIET_MS))
                .unwrap();
            let outline: Vec<Vec2> = globe
                .silhouette(&t, shield::RIM_SEGMENTS)
                .iter()
                .map(|p| t.world_to_screen(*p))
                .collect();
            let margin = 4.0;
            let (lo, hi) = outline.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            );
            let mut inside = 0;
            for (x, y) in changed_pixels(&without, &with) {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let near = p.x >= lo.x - margin
                    && p.x <= hi.x + margin
                    && p.y >= lo.y - margin
                    && p.y <= hi.y + margin
                    && (inside_polygon(&outline, p) || distance_to_outline(&outline, p) <= margin);
                assert!(
                    near,
                    "view {view}: the shield changed ({x}, {y}), outside its rim"
                );
                inside += 1;
            }
            assert!(
                inside > 1000,
                "view {view}: the shield drew only {inside} pixels"
            );
        }
    }
}
