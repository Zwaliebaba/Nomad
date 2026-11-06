//! GPU device, queue and window surface (phase-1 doc 05, `render/context.rs`).
//!
//! # Colour pipeline
//!
//! Every render target uses an 8-bit UNORM (non-sRGB) format, and shaders write sRGB-encoded
//! values directly. The phase-1 colours, alphas and gradients come from a browser mockup, and
//! browsers blend in sRGB space. Blending the same way makes faint lines, such as the grid at
//! alpha 0.05, look like the reference renders; blending in linear space would make them
//! noticeably brighter. Revisit if bloom (M3) needs a linear intermediate target.
//!
//! # Depth and MSAA
//!
//! The scene pass renders into a 4× multisampled colour target with a depth buffer and resolves
//! into the frame (doc 05: "use MSAA, not FXAA, on voxels"). Adapters that cannot multisample the
//! frame format fall back to one sample, with a warning.

use std::sync::Arc;

use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

/// Depth buffer format of the scene pass. 32-bit float is available on every backend, including
/// the GLES fallback.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// MSAA sample count of the scene pass (doc 05).
pub const MSAA_SAMPLES: u32 = 4;

/// Adapter, device and queue. (wgpu objects keep their instance alive.)
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("no usable GPU adapter: {0}")]
    NoAdapter(#[from] wgpu::RequestAdapterError),
    #[error("cannot open the GPU device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("cannot create the window surface: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("the window surface offers no 8-bit RGBA or BGRA format on this adapter")]
    NoSurfaceFormat,
    #[error("{width}×{height} exceeds the adapter's texture size limit of {limit}")]
    TooLarge { width: u32, height: u32, limit: u32 },
    #[error("reading the frame back from the GPU failed: {0}")]
    Readback(String),
}

impl Gpu {
    /// A device without a window, for offscreen rendering (`--screenshot`) and GPU tests.
    /// `WGPU_BACKEND`, `WGPU_ADAPTER_NAME` and `WGPU_POWER_PREF` select the adapter.
    pub fn headless() -> Result<Self, GpuError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        Self::from_instance(&instance, None)
    }

    fn from_instance(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, GpuError> {
        let adapter =
            pollster::block_on(
                instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::from_env()
                        .unwrap_or(wgpu::PowerPreference::HighPerformance),
                    compatible_surface: surface,
                    ..Default::default()
                }),
            )?;
        let info = adapter.get_info();
        tracing::info!(
            adapter = %info.name,
            backend = ?info.backend,
            device_type = ?info.device_type,
            driver = %info.driver,
            "GPU adapter"
        );
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("nomad-client"),
                required_features: wgpu::Features::empty(),
                // The lowest common limits, with the adapter's real texture size limits.
                required_limits:
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                ..Default::default()
            }))?;
        Ok(Self {
            adapter,
            device,
            queue,
        })
    }

    /// [`MSAA_SAMPLES`] when the adapter can multisample both `color` and [`DEPTH_FORMAT`],
    /// otherwise 1.
    pub fn sample_count(&self, color: wgpu::TextureFormat) -> u32 {
        let supported = [color, DEPTH_FORMAT].into_iter().all(|format| {
            self.adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(MSAA_SAMPLES)
        });
        if supported {
            MSAA_SAMPLES
        } else {
            tracing::warn!(
                ?color,
                "the adapter cannot multisample the frame; rendering without MSAA"
            );
            1
        }
    }

    /// Rejects sizes above the device's 2D texture limit.
    pub fn check_size(&self, width: u32, height: u32) -> Result<(), GpuError> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(GpuError::TooLarge {
                width,
                height,
                limit,
            });
        }
        Ok(())
    }
}

/// Formats and sample count shared by every pipeline of the scene pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetFormat {
    pub color: wgpu::TextureFormat,
    pub sample_count: u32,
}

impl TargetFormat {
    pub fn multisample(self) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: self.sample_count,
            ..Default::default()
        }
    }
}

/// Depth state for a pipeline of the scene pass. The screen-space passes (background, grid)
/// neither test nor write depth: they are drawn first, behind everything (doc 05 frame order).
pub fn depth_stencil(
    write: bool,
    compare: Option<wgpu::CompareFunction>,
) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: compare,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The multisampled colour target (when MSAA is on) and the depth buffer for one frame size.
pub struct FrameTargets {
    size: (u32, u32),
    msaa_color: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
}

impl FrameTargets {
    pub fn new(device: &wgpu::Device, format: TargetFormat, (width, height): (u32, u32)) -> Self {
        // Both attachments share the pass's sample count.
        let attachment = |label, texture_format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: format.sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format: texture_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        Self {
            size: (width, height),
            msaa_color: (format.sample_count > 1).then(|| attachment("msaa colour", format.color)),
            depth: attachment("depth", DEPTH_FORMAT),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The colour attachment for `frame`: the MSAA target resolving into `frame`, or `frame`
    /// itself without MSAA. Cleared to `clear`.
    pub fn color_attachment<'a>(
        &'a self,
        frame: &'a wgpu::TextureView,
        clear: wgpu::Color,
    ) -> wgpu::RenderPassColorAttachment<'a> {
        let (view, resolve_target, store) = match &self.msaa_color {
            // The samples are not needed once resolved.
            Some(msaa) => (msaa, Some(frame), wgpu::StoreOp::Discard),
            None => (frame, None, wgpu::StoreOp::Store),
        };
        wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store,
            },
        }
    }

    /// The depth attachment, cleared to the far plane and discarded after the pass.
    pub fn depth_attachment(&self) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: None,
        }
    }
}

/// Picks the surface format and the format pipelines render with. Prefers a UNORM format; when
/// the surface offers only sRGB formats, renders through a UNORM view of one (see module docs).
pub fn choose_surface_format(
    formats: &[wgpu::TextureFormat],
) -> Option<(wgpu::TextureFormat, wgpu::TextureFormat)> {
    use wgpu::TextureFormat::{Bgra8Unorm, Rgba8Unorm};
    let usable = |f: wgpu::TextureFormat| matches!(f, Bgra8Unorm | Rgba8Unorm);
    if let Some(&format) = formats.iter().find(|&&f| usable(f)) {
        return Some((format, format));
    }
    formats
        .iter()
        .find(|f| usable(f.remove_srgb_suffix()))
        .map(|&f| (f, f.remove_srgb_suffix()))
}

/// A window's swap chain. The surface owns a handle to the window, keeping it alive.
pub struct WindowSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    /// Set after a suboptimal frame; the surface is reconfigured before the next one.
    reconfigure: bool,
}

/// Outcome of asking for the next frame.
pub enum Acquired {
    Frame(wgpu::SurfaceTexture, wgpu::TextureView),
    /// No frame this time (minimised, occluded, timed out, or the surface was reconfigured).
    Skip,
    /// The surface is gone, usually because its window was destroyed.
    Lost,
}

impl WindowSurface {
    /// Creates the GPU and a surface for `window`.
    pub fn create(
        window: &Arc<Window>,
        display: OwnedDisplayHandle,
    ) -> Result<(Gpu, Self), GpuError> {
        // GLES needs the display connection up front, especially on Wayland.
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display)),
        );
        let surface = instance.create_surface(wgpu::SurfaceTarget::from_window_without_display(
            Arc::clone(window),
        ))?;
        let gpu = Gpu::from_instance(&instance, Some(&surface))?;

        let caps = surface.get_capabilities(&gpu.adapter);
        let (format, view_format) =
            choose_surface_format(&caps.formats).ok_or(GpuError::NoSurfaceFormat)?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or(GpuError::NoSurfaceFormat)?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.view_formats = if view_format == format {
            Vec::new()
        } else {
            vec![view_format]
        };
        tracing::info!(?format, ?view_format, "window surface");

        let mut this = Self {
            surface,
            config,
            view_format,
            reconfigure: false,
        };
        this.resize(&gpu, size.width, size.height);
        Ok((gpu, this))
    }

    /// The format pipelines must target.
    pub fn view_format(&self) -> wgpu::TextureFormat {
        self.view_format
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Reconfigures for a new window size. A zero size (minimised) is ignored.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let limit = gpu.device.limits().max_texture_dimension_2d;
        self.config.width = width.min(limit);
        self.config.height = height.min(limit);
        self.surface.configure(&gpu.device, &self.config);
    }

    /// The next frame to draw into, or `Skip` when there is none right now. The previous frame
    /// must have been presented or dropped.
    pub fn acquire(&mut self, gpu: &Gpu) -> Acquired {
        if self.reconfigure {
            self.reconfigure = false;
            self.surface.configure(&gpu.device, &self.config);
        }
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => self.frame(texture),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                // Draw this one; reconfigure before the next.
                self.reconfigure = true;
                self.frame(texture)
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&gpu.device, &self.config);
                Acquired::Skip
            }
            // Recreating the surface is what wgpu suggests, but when the window itself is gone
            // wgpu-hal panics inside surface creation (seen on Vulkan/X11 after the window was
            // destroyed externally). The caller ends the session cleanly instead.
            wgpu::CurrentSurfaceTexture::Lost => Acquired::Lost,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Acquired::Skip
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                tracing::error!("validation error while acquiring a frame");
                Acquired::Skip
            }
        }
    }

    fn frame(&self, texture: wgpu::SurfaceTexture) -> Acquired {
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        Acquired::Frame(texture, view)
    }
}

#[cfg(test)]
mod tests {
    use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb, Rgba8UnormSrgb, Rgba16Float};

    use super::*;

    #[test]
    fn prefers_unorm_surface_formats() {
        assert_eq!(
            choose_surface_format(&[Bgra8UnormSrgb, Bgra8Unorm]),
            Some((Bgra8Unorm, Bgra8Unorm))
        );
        assert_eq!(
            choose_surface_format(&[Rgba16Float, Rgba8UnormSrgb]),
            Some((Rgba8UnormSrgb, wgpu::TextureFormat::Rgba8Unorm))
        );
        assert_eq!(choose_surface_format(&[Rgba16Float]), None);
    }
}
