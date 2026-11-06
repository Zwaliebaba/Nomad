//! Game client.
//!
//! Phase 1 implements the visual slice specified in `Design/phase1-visual-slice/`. This is
//! milestone M3 (phase-1 doc 06): a `winit` window with a `wgpu` surface, the screen-space
//! background, the infinite ops grid, the snap-rotation camera with zoom and pan (M1), the
//! station voxels from `station_scene.json`, greedy-meshed and shaded relative to the camera
//! (M2), bloom on emissive voxels, elevation cues under lifted modules, and the hover highlight
//! with its `ELEV +n` tag (M3).
//!
//! ```text
//! nomad-client [--windowed] [--size WxH] [--view N] [--zoom Z]
//! nomad-client --screenshot OUT.png [--size WxH] [--view N] [--zoom Z]
//!              [--hover X,Y] [--turn-ms MS]
//! ```
//!
//! `--screenshot` renders one frame offscreen, without a window, and writes a PNG.

mod app;
mod assets;
mod camera;
mod cli;
mod config;
mod mesh;
mod pick;
mod render;
mod screenshot;

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use glam::Vec2;
use tracing_subscriber::EnvFilter;

use assets::SceneAssets;
use camera::{Camera, Projection, Turn};
use cli::{Command, Options};
use config::{ConfigError, SceneConfig};
use render::context::{Gpu, GpuError};
use render::{FrameInput, Layers, Renderer};
use screenshot::ScreenshotError;

/// Fatal errors, reported once at exit.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Gpu(#[from] GpuError),
    #[error(transparent)]
    Screenshot(#[from] ScreenshotError),
    #[error("event loop: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error("cannot create the window: {0}")]
    Window(#[from] winit::error::OsError),
    #[error("the window surface was lost (was the window destroyed?)")]
    SurfaceLost,
    #[error("--zoom {zoom} is not one of the zoom levels {levels:?}")]
    UnknownZoom { zoom: f32, levels: Vec<f32> },
}

fn main() -> ExitCode {
    init_tracing();
    let options = match cli::parse(std::env::args_os().skip(1)) {
        Ok(Command::Run(options)) => options,
        Ok(Command::Help) => {
            // Ignore a closed stdout; there is nothing else to report.
            let _ = writeln!(io::stdout(), "{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            let _ = writeln!(io::stderr(), "nomad-client: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Messages already include their causes.
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .init();
}

fn run(options: &Options) -> Result<(), AppError> {
    let assets = SceneAssets::load(&config::asset_root())?;
    let camera = initial_camera(&assets.config, options)?;
    match &options.screenshot {
        Some(path) => render_screenshot(&assets, camera, options, path),
        None => app::run(assets, camera, options.size, !options.windowed),
    }
}

fn initial_camera(scene: &SceneConfig, options: &Options) -> Result<Camera, AppError> {
    let (w, h) = options.size;
    let projection = Projection::from_config(scene);
    let zoom_index = match options.zoom {
        Some(zoom) => {
            Some(
                projection
                    .zoom_index_of(zoom)
                    .ok_or_else(|| AppError::UnknownZoom {
                        zoom,
                        levels: projection.zoom_levels().to_vec(),
                    })?,
            )
        }
        None => None,
    };
    let mut camera = Camera::new(projection, Vec2::new(w as f32, h as f32));
    camera.set_view(options.view);
    if let Some(index) = zoom_index {
        camera.set_zoom_index(index);
    }
    Ok(camera)
}

/// `--screenshot`: one frame offscreen, no window. `--turn-ms` and `--hover` freeze a moment of a
/// turn and a cursor position, so the M3 hover and mid-turn cues can be checked offscreen.
fn render_screenshot(
    assets: &SceneAssets,
    mut camera: Camera,
    options: &Options,
    path: &Path,
) -> Result<(), AppError> {
    let (width, height) = options.size;
    if let Some(ms) = options.turn_ms {
        camera.rotate(Turn::Plus90);
        camera.update(ms as f32 / 1000.0);
    }
    let view = camera.transform();
    let hovered = options
        .hover
        .and_then(|(x, y)| pick::module_at(&assets.station, &view, Vec2::new(x, y)));
    if options.hover.is_some() {
        tracing::info!(
            module = ?hovered.map(|i| assets.station.modules[i].id.as_str()),
            "hover"
        );
    }
    let gpu = Gpu::headless()?;
    gpu.check_size(width, height)?;
    let mut renderer = Renderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, assets);
    let image = renderer.render_to_image(
        width,
        height,
        &view,
        Layers::default(),
        FrameInput { hovered },
    )?;
    screenshot::save(path, &image)?;
    tracing::info!(
        path = %path.display(),
        view = camera.view_index(),
        zoom = camera.zoom(),
        "screenshot written"
    );
    Ok(())
}
