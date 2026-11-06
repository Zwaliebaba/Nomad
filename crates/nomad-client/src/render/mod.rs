//! Renderer (phase-1 doc 05). M2 draws the background, the ops grid and the opaque station voxels
//! in one multisampled pass with a depth buffer; the frame order of doc 05 continues with ships,
//! overlays, shield, FX, bloom and HUD in later milestones.

pub mod background;
pub mod context;
pub mod grid;
pub mod voxels;

use std::num::NonZeroU64;
use std::sync::mpsc;

use glam::Vec2;

use crate::assets::voxel::StationScene;
use crate::camera::ViewTransform;
use crate::config::{OpsGridConfig, SceneConfig};
use background::BackgroundPass;
use context::{FrameTargets, Gpu, GpuError, TargetFormat};
use grid::{GridPass, GridUniforms};
use voxels::VoxelPass;

pub const BACKGROUND_WGSL: &str = concat!(
    include_str!("shaders/fullscreen.wgsl"),
    include_str!("shaders/background.wgsl")
);
pub const GRID_WGSL: &str = concat!(
    include_str!("shaders/fullscreen.wgsl"),
    include_str!("shaders/grid.wgsl")
);
pub const VOXELS_WGSL: &str = include_str!("shaders/voxels.wgsl");

/// Which layers to draw. Tests use it to isolate one layer from the others.
#[derive(Clone, Copy, Debug)]
pub struct Layers {
    pub background: bool,
    pub grid: bool,
    pub voxels: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            background: true,
            grid: true,
            voxels: true,
        }
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
    target: TargetFormat,
    /// MSAA colour and depth for the last frame size; rebuilt when the size changes.
    frame_targets: Option<FrameTargets>,
    /// What shows where no layer draws (only visible with the background layer off).
    clear_color: wgpu::Color,
    background: BackgroundPass,
    grid: GridPass,
    grid_config: OpsGridConfig,
    voxels: VoxelPass,
}

impl Renderer {
    /// `format` is the format of every target this renderer draws into.
    pub fn new(
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        config: &SceneConfig,
        station: &StationScene,
    ) -> Self {
        let target = TargetFormat {
            color: format,
            sample_count: gpu.sample_count(format),
        };
        tracing::info!(?format, samples = target.sample_count, "scene pass");
        Self {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            target,
            frame_targets: None,
            clear_color: wgpu::Color::BLACK,
            background: BackgroundPass::new(&gpu.device, target, config),
            grid: GridPass::new(&gpu.device, target),
            grid_config: config.ops_grid.clone(),
            voxels: VoxelPass::new(&gpu.device, &gpu.queue, target, station),
        }
    }

    /// Draws one frame into `frame`, whose size must match `view.viewport_px`.
    pub fn render(&mut self, frame: &wgpu::TextureView, view: &ViewTransform, layers: Layers) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        self.encode(&mut encoder, frame, view, layers);
        self.queue.submit([encoder.finish()]);
    }

    fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        view: &ViewTransform,
        layers: Layers,
    ) {
        let size = frame.texture().size();
        let size = (size.width, size.height);
        if self.frame_targets.as_ref().map(FrameTargets::size) != Some(size) {
            self.frame_targets = Some(FrameTargets::new(&self.device, self.target, size));
        }
        let targets = self
            .frame_targets
            .as_ref()
            .expect("invariant: frame targets were created above");

        self.background.prepare(&self.queue, view.viewport_px);
        self.grid
            .prepare(&self.queue, &GridUniforms::new(&self.grid_config, view));
        self.voxels.prepare(&self.queue, view);

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(targets.color_attachment(frame, self.clear_color))],
            depth_stencil_attachment: Some(targets.depth_attachment()),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        // Doc 05 frame order: 1 background, 2 ops grid (no depth test or write, so it lies under
        // everything), 3 opaque voxels with depth.
        if layers.background {
            self.background.draw(&mut pass);
        }
        if layers.grid {
            self.grid.draw(&mut pass);
        }
        if layers.voxels {
            self.voxels.draw(&mut pass);
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
        self.encode(&mut encoder, &frame, view, layers);
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
            targets: &[Some(wgpu::ColorTargetState {
                format: target.color,
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
    use glam::Vec3;

    use super::*;
    use crate::assets::voxel::load_station_scene;
    use crate::camera::{Camera, Projection, Turn};
    use crate::config::{
        default_asset_root, load_scene_config, scene_config_path, station_scene_path,
    };

    /// Shaders are validated without a GPU, so CI catches WGSL errors.
    #[test]
    fn shaders_parse_and_validate() {
        use wgpu::naga::valid::{Capabilities, ValidationFlags, Validator};
        for (name, source) in [
            ("background", BACKGROUND_WGSL),
            ("grid", GRID_WGSL),
            ("voxels", VOXELS_WGSL),
        ] {
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

    fn station() -> StationScene {
        load_station_scene(&station_scene_path(&default_asset_root())).unwrap()
    }

    fn renderer(format: wgpu::TextureFormat) -> (Renderer, SceneConfig) {
        let scene = scene();
        let gpu = Gpu::headless().expect("a GPU adapter");
        (Renderer::new(&gpu, format, &scene, &station()), scene)
    }

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
            ..Layers::default()
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
                    .render_to_image(w, h, &transform, grid_only)
                    .unwrap();
                let without = renderer.render_to_image(w, h, &transform, neither).unwrap();
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
            ..Layers::default()
        };
        let image = renderer
            .render_to_image(1920, 1080, &camera.transform(), layers)
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
            voxels: true,
        };
        let [width, height] = view.viewport_px.to_array().map(whole_px);
        let image = renderer
            .render_to_image(width, height, view, layers)
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
        let scene = scene();
        let station = station();
        let gpu = Gpu::headless().expect("a GPU adapter");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut greedy = Renderer::new(&gpu, format, &scene, &station);
        let mut unmerged = Renderer::new(&gpu, format, &scene, &station);
        let reference_mesh = voxels::build_unmerged_station_mesh(&station);
        unmerged.voxels = VoxelPass::from_mesh(
            &gpu.device,
            &gpu.queue,
            unmerged.target,
            &station,
            &reference_mesh,
        );
        for (width, height) in [(1920_u32, 1080_u32), (1280, 720)] {
            let mut camera = Camera::new(
                Projection::from_config(&scene),
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
                .render_to_image(w, h, &transform, Layers::default())
                .unwrap();
            let no_grid = Layers {
                grid: false,
                ..Layers::default()
            };
            let without_grid = renderer.render_to_image(w, h, &transform, no_grid).unwrap();
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
        let scene = scene();
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
        let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &scene, &station);
        let (width, height) = (960, 540);
        let mut camera = Camera::new(
            Projection::from_config(&scene),
            Vec2::new(width as f32, height as f32),
        );
        camera.set_zoom_index(4);
        let layers = Layers {
            background: false,
            grid: false,
            voxels: true,
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
            let image = renderer.render_to_image(width, height, &t, layers).unwrap();
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
}
