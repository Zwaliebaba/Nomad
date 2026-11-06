//! The windowed client: owns the window, GPU, renderer and camera, routes input and paces frames
//! (phase-1 doc 05, `app.rs`; key bindings from doc 02).

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{
    ElementState, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::AppError;
use crate::assets::SceneAssets;
use crate::camera::{Camera, Turn, ZoomStep};
use crate::pick;
use crate::render::context::{Acquired, Gpu, WindowSurface};
use crate::render::{FrameInput, Layers, Renderer};
use crate::screenshot;

/// Window title: the working title (D-018).
const WINDOW_TITLE: &str = "Nomad";
/// Edge-scroll margin in reference pixels (doc 02).
const EDGE_SCROLL_MARGIN_REF_PX: f32 = 16.0;
/// Touchpad scroll distance per zoom step.
const PIXELS_PER_ZOOM_STEP: f32 = 50.0;
/// Longest frame step fed to animations, so a stall does not skip a whole turn.
const MAX_FRAME_DT_S: f32 = 0.1;
/// When no frame is available (minimised or occluded), try again after this long instead of
/// spinning.
const SKIPPED_FRAME_RETRY: Duration = Duration::from_millis(100);

/// Opens the window and runs until it is closed. With `fullscreen` the window opens
/// borderless fullscreen on the current monitor (D-108); `size` is then the window size F11
/// returns to.
pub fn run(
    assets: SceneAssets,
    camera: Camera,
    size: (u32, u32),
    fullscreen: bool,
) -> Result<(), AppError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        assets,
        initial_camera: camera,
        size,
        fullscreen,
        running: None,
        error: None,
    };
    event_loop.run_app(&mut app)?;
    app.error.map_or(Ok(()), Err)
}

struct App {
    assets: SceneAssets,
    initial_camera: Camera,
    size: (u32, u32),
    /// Open borderless fullscreen (D-108).
    fullscreen: bool,
    running: Option<Running>,
    /// A fatal error raised inside the event loop, reported after it exits.
    error: Option<AppError>,
}

struct Running {
    window: Arc<Window>,
    gpu: Gpu,
    surface: WindowSurface,
    renderer: Renderer,
    camera: Camera,
    input: Input,
    /// Keyboard and edge-scroll pan speed, from `scene_config.json` (D-063).
    pan_speed_ref_px_per_s: f32,
    last_frame: Instant,
    /// What the last frame drew besides the camera (the hovered module); F12 reuses it.
    frame_input: FrameInput,
}

impl App {
    fn start(&self, event_loop: &ActiveEventLoop) -> Result<Running, AppError> {
        let (width, height) = self.size;
        let attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(PhysicalSize::new(width, height))
            .with_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
        let window = Arc::new(event_loop.create_window(attributes)?);
        let (gpu, surface) = WindowSurface::create(&window, event_loop.owned_display_handle())?;
        let renderer = Renderer::new(&gpu, surface.view_format(), &self.assets);
        let mut camera = self.initial_camera.clone();
        let (w, h) = surface.size();
        camera.set_viewport(Vec2::new(w as f32, h as f32));
        Ok(Running {
            window,
            gpu,
            surface,
            renderer,
            camera,
            input: Input::default(),
            pan_speed_ref_px_per_s: self.assets.config.camera.pan_speed_ref_px_per_s,
            last_frame: Instant::now(),
            frame_input: FrameInput::default(),
        })
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            event_loop.set_control_flow(ControlFlow::Wait);
            if let Some(running) = &self.running {
                running.window.request_redraw();
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(running) => {
                running.window.request_redraw();
                self.running = Some(running);
            }
            Err(error) => {
                self.error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(running) = &mut self.running else {
            return;
        };
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Resized(size) => {
                running.resize(size);
                running.window.request_redraw();
            }
            WindowEvent::Focused(focused) => running.input.set_focused(focused),
            WindowEvent::ModifiersChanged(modifiers) => {
                running.input.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => running.key(&event),
            WindowEvent::CursorMoved { position, .. } => {
                let cursor = Vec2::new(to_f32(position.x), to_f32(position.y));
                if let Some(last) = &mut running.input.drag_from {
                    let delta = cursor - *last;
                    *last = cursor;
                    running.camera.drag_px(delta);
                }
                running.input.cursor = Some(cursor);
            }
            WindowEvent::CursorLeft { .. } => running.input.cursor = None,
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Middle,
                ..
            } => {
                running.input.drag_from = match state {
                    ElementState::Pressed => running.input.cursor,
                    ElementState::Released => None,
                };
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => lines,
                    MouseScrollDelta::PixelDelta(pixels) => to_f32(pixels.y) / PIXELS_PER_ZOOM_STEP,
                };
                running.wheel(steps);
            }
            WindowEvent::RedrawRequested => match running.frame(&self.assets) {
                Ok(true) => running.window.request_redraw(),
                Ok(false) => event_loop
                    .set_control_flow(ControlFlow::WaitUntil(Instant::now() + SKIPPED_FRAME_RETRY)),
                Err(error) => {
                    self.error = Some(error);
                    event_loop.exit();
                }
            },
            _ => {}
        }
    }
}

impl Running {
    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.surface.resize(&self.gpu, size.width, size.height);
        let (w, h) = self.surface.size();
        self.camera.set_viewport(Vec2::new(w as f32, h as f32));
    }

    fn key(&mut self, event: &KeyEvent) {
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        let pressed = event.state == ElementState::Pressed;
        if self.input.pan_keys.set(code, pressed) || !pressed || event.repeat {
            return;
        }
        // Ctrl+key is reserved for command-card hotkeys (doc 02, phase-1 doc 07 item 1).
        let ctrl = self.input.modifiers.control_key();
        match code {
            KeyCode::KeyQ if !ctrl => self.camera.rotate(Turn::Minus90),
            KeyCode::KeyE if !ctrl => self.camera.rotate(Turn::Plus90),
            KeyCode::Home => self.camera.reset_home(),
            KeyCode::F11 => {
                let fullscreen = self.window.fullscreen().is_some();
                self.window
                    .set_fullscreen((!fullscreen).then_some(Fullscreen::Borderless(None)));
            }
            KeyCode::F12 => self.screenshot(),
            _ => {}
        }
    }

    fn wheel(&mut self, steps: f32) {
        self.input.wheel_steps += steps;
        while self.input.wheel_steps >= 1.0 {
            self.input.wheel_steps -= 1.0;
            self.camera.zoom_step(ZoomStep::In, self.input.cursor);
        }
        while self.input.wheel_steps <= -1.0 {
            self.input.wheel_steps += 1.0;
            self.camera.zoom_step(ZoomStep::Out, self.input.cursor);
        }
    }

    /// Advances the camera, picks the module under the cursor and draws one frame. Returns whether
    /// a frame was presented; fails only when the window surface is lost.
    fn frame(&mut self, assets: &SceneAssets) -> Result<bool, AppError> {
        let now = Instant::now();
        let dt_s = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .min(MAX_FRAME_DT_S);
        self.last_frame = now;

        let (w, h) = self.surface.size();
        let viewport = Vec2::new(w as f32, h as f32);
        let layout = self.camera.layout_scale();
        let direction = self
            .input
            .pan_direction(viewport, EDGE_SCROLL_MARGIN_REF_PX * layout);
        if direction != Vec2::ZERO {
            self.camera
                .pan_screen_px(direction * self.pan_speed_ref_px_per_s * layout * dt_s);
        }
        self.camera.update(dt_s);
        let view = self.camera.transform();
        // Doc 06 M3: hover a module → highlight + ELEV tag. Picked against this frame's view, so
        // the highlight follows turns, zooms and pans under a still cursor.
        let hovered = self
            .input
            .cursor
            .filter(|_| !self.input.unfocused)
            .and_then(|cursor| pick::module_at(&assets.station, &view, cursor));
        self.frame_input = FrameInput { hovered };

        match self.surface.acquire(&self.gpu) {
            Acquired::Frame(frame, target) => {
                self.renderer
                    .render(&target, &view, Layers::default(), self.frame_input);
                self.window.pre_present_notify();
                self.gpu.queue.present(frame);
                Ok(true)
            }
            Acquired::Skip => Ok(false),
            Acquired::Lost => Err(AppError::SurfaceLost),
        }
    }

    fn screenshot(&mut self) {
        let (w, h) = self.surface.size();
        let view = self.camera.transform();
        match self
            .renderer
            .render_to_image(w, h, &view, Layers::default(), self.frame_input)
        {
            Ok(image) => {
                let path = screenshot::next_path();
                // Encode off the frame loop.
                std::thread::spawn(move || match screenshot::save(&path, &image) {
                    Ok(()) => tracing::info!(path = %path.display(), "screenshot written"),
                    Err(error) => tracing::error!(%error, "screenshot failed"),
                });
            }
            Err(error) => tracing::error!(%error, "screenshot failed"),
        }
    }
}

/// Pan keys currently held. WASD and arrows by physical position, so they work on any layout.
#[derive(Default)]
struct PanKeys {
    up: [bool; 2],
    down: [bool; 2],
    left: [bool; 2],
    right: [bool; 2],
}

impl PanKeys {
    /// Records a pan key; returns false for keys that do not pan.
    fn set(&mut self, code: KeyCode, pressed: bool) -> bool {
        let slot = match code {
            KeyCode::KeyW => &mut self.up[0],
            KeyCode::ArrowUp => &mut self.up[1],
            KeyCode::KeyS => &mut self.down[0],
            KeyCode::ArrowDown => &mut self.down[1],
            KeyCode::KeyA => &mut self.left[0],
            KeyCode::ArrowLeft => &mut self.left[1],
            KeyCode::KeyD => &mut self.right[0],
            KeyCode::ArrowRight => &mut self.right[1],
            _ => return false,
        };
        *slot = pressed;
        true
    }

    /// Screen direction, y down, each component in -1..=1.
    fn direction(&self) -> Vec2 {
        let held = |keys: [bool; 2]| if keys.contains(&true) { 1.0 } else { 0.0 };
        Vec2::new(
            held(self.right) - held(self.left),
            held(self.down) - held(self.up),
        )
    }
}

#[derive(Default)]
struct Input {
    pan_keys: PanKeys,
    modifiers: ModifiersState,
    cursor: Option<Vec2>,
    /// Cursor position at the last middle-drag update while the middle button is held.
    drag_from: Option<Vec2>,
    wheel_steps: f32,
    unfocused: bool,
}

impl Input {
    fn set_focused(&mut self, focused: bool) {
        self.unfocused = !focused;
        if !focused {
            // Releases are not delivered to an unfocused window.
            self.pan_keys = PanKeys::default();
            self.drag_from = None;
            self.modifiers = ModifiersState::empty();
        }
    }

    /// Combined pan direction from keys and edge scrolling, at most unit length.
    fn pan_direction(&self, viewport: Vec2, edge_margin_px: f32) -> Vec2 {
        let mut direction = self.pan_keys.direction();
        if !self.unfocused
            && self.drag_from.is_none()
            && let Some(cursor) = self.cursor
        {
            direction += edge_scroll(cursor, viewport, edge_margin_px);
        }
        let direction = direction.clamp(Vec2::NEG_ONE, Vec2::ONE);
        direction.normalize_or_zero() * direction.length().min(1.0)
    }
}

/// Window coordinates arrive as `f64`; pixel positions fit `f32` with room to spare.
#[expect(
    clippy::cast_possible_truncation,
    reason = "window pixel coordinates are far inside f32 range"
)]
fn to_f32(value: f64) -> f32 {
    value as f32
}

/// Edge scrolling: pan toward any window edge the cursor is within `margin_px` of (doc 02).
fn edge_scroll(cursor: Vec2, viewport: Vec2, margin_px: f32) -> Vec2 {
    let axis = |position: f32, size: f32| {
        if position < margin_px {
            -1.0
        } else if position >= size - margin_px {
            1.0
        } else {
            0.0
        }
    };
    Vec2::new(axis(cursor.x, viewport.x), axis(cursor.y, viewport.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: Vec2 = Vec2::new(1920.0, 1080.0);

    #[test]
    fn edge_scroll_uses_a_16_px_margin() {
        assert_eq!(
            edge_scroll(Vec2::new(960.0, 540.0), VIEWPORT, 16.0),
            Vec2::ZERO
        );
        assert_eq!(
            edge_scroll(Vec2::new(15.9, 540.0), VIEWPORT, 16.0),
            Vec2::new(-1.0, 0.0)
        );
        assert_eq!(
            edge_scroll(Vec2::new(16.0, 540.0), VIEWPORT, 16.0),
            Vec2::ZERO
        );
        assert_eq!(
            edge_scroll(Vec2::new(1905.0, 0.0), VIEWPORT, 16.0),
            Vec2::new(1.0, -1.0)
        );
        assert_eq!(
            edge_scroll(Vec2::new(500.0, 1079.0), VIEWPORT, 16.0),
            Vec2::new(0.0, 1.0)
        );
    }

    #[test]
    fn pan_direction_combines_keys_and_edges() {
        let mut input = Input::default();
        assert!(input.pan_keys.set(KeyCode::KeyD, true));
        assert!(input.pan_keys.set(KeyCode::ArrowRight, true));
        assert!(!input.pan_keys.set(KeyCode::KeyQ, true));
        assert_eq!(input.pan_direction(VIEWPORT, 16.0), Vec2::new(1.0, 0.0));

        // Key and edge in the same direction do not stack; diagonals are unit length.
        input.cursor = Some(Vec2::new(1919.0, 0.0));
        let direction = input.pan_direction(VIEWPORT, 16.0);
        assert!((direction.length() - 1.0).abs() < 1e-6);
        assert!(direction.x > 0.0 && direction.y < 0.0);

        // Releasing one of two keys for the same direction keeps panning.
        input.cursor = None;
        input.pan_keys.set(KeyCode::KeyD, false);
        assert_eq!(input.pan_direction(VIEWPORT, 16.0), Vec2::new(1.0, 0.0));

        // No edge scrolling while unfocused or dragging; focus loss clears held keys.
        input.cursor = Some(Vec2::new(0.0, 540.0));
        input.drag_from = Some(Vec2::ZERO);
        assert_eq!(input.pan_direction(VIEWPORT, 16.0), Vec2::new(1.0, 0.0));
        input.set_focused(false);
        assert_eq!(input.pan_direction(VIEWPORT, 16.0), Vec2::ZERO);
    }
}
