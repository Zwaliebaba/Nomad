//! World labels and markers (phase-1 doc 05 frame order step 9), drawn in screen space after the
//! bloom with `glyphon` (doc 05 "Text"). M3 draws one: the `ELEV +n` tag of the hovered module
//! when it is lifted (doc 01 §7: "an ELEV +n tag (10 px bold) only on hover or selection").
//!
//! The tag's text, font, size, weight, colour and offset come from `hud_layout.json` →
//! `markers.elevation_tag`; the tier comes from the station data. Nothing player-facing is a
//! literal here (phase-1 README, ground rule 3). The text is centred under the footprint, its
//! baseline `baseline_below_footprint_px` below the footprint's lowest screen point, like the
//! mockup (phase-1 doc 07 item 24). Sizes are 1080p pixels and scale with the window, not with
//! the zoom (doc 04).
//!
//! The fonts are not bundled (`assets::fonts`). Without the tag's font family the tag is skipped
//! (the hover highlight still shows); startup logs a warning that says where to put the fonts.

use glyphon::{
    Attrs, Buffer, Cache, Color, ColorMode, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight, fontdb,
};

use super::overlay::ElevatedModule;
use crate::assets::fonts::has_family;
use crate::camera::ViewTransform;
use crate::config::{ElevationTagStyle, HudLayout};

/// Line height as a multiple of the font size (CSS `normal` for these fonts is about 1.2).
const LINE_HEIGHT: f32 = 1.2;

/// Text rendering state, present only when the tag's font is installed.
struct TextState {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: TextRenderer,
    family: String,
    /// One shaped buffer per elevated module, in the same order.
    buffers: Vec<Buffer>,
    /// Layout scale the buffers were last shaped at.
    shaped_scale: f32,
    /// Whether the last `prepare` left anything to draw.
    has_text: bool,
}

pub struct LabelPass {
    modules: Vec<ElevatedModule>,
    style: ElevationTagStyle,
    texts: Vec<String>,
    text: Option<TextState>,
}

impl LabelPass {
    /// `format` is the frame format; the labels are drawn without MSAA onto the resolved frame.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        hud: &HudLayout,
        fonts: &fontdb::Database,
        modules: Vec<ElevatedModule>,
    ) -> Self {
        let style = hud.markers.elevation_tag.clone();
        let family = hud.font_families.family(style.font).to_owned();
        let texts: Vec<String> = modules.iter().map(|m| style.text_for(m.tier)).collect();
        let text = has_family(fonts, &family).then(|| {
            let mut font_system =
                FontSystem::new_with_locale_and_db("en-US".to_owned(), fonts.clone());
            let buffers = texts
                .iter()
                .map(|_| Buffer::new(&mut font_system, Metrics::new(style.size_px, style.size_px)))
                .collect();
            let cache = Cache::new(device);
            // The frame holds sRGB-encoded values in a UNORM target, as a browser does
            // (render::context), so glyph colours are passed through unconverted.
            let mut atlas =
                TextAtlas::with_color_mode(device, queue, &cache, format, ColorMode::Web);
            let renderer =
                TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
            TextState {
                font_system,
                swash_cache: SwashCache::new(),
                viewport: Viewport::new(device, &cache),
                atlas,
                renderer,
                family,
                buffers,
                shaped_scale: 0.0,
                has_text: false,
            }
        });
        Self {
            modules,
            style,
            texts,
            text,
        }
    }

    /// Whether tags can show text (their font is installed).
    #[cfg(test)]
    pub fn has_font(&self) -> bool {
        self.text.is_some()
    }

    /// Lays out this frame's tags: the tag of `hovered` (a module index) if it is lifted.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &ViewTransform,
        hovered: Option<usize>,
    ) {
        let Some(text) = &mut self.text else {
            return;
        };
        text.has_text = false;
        let Some(slot) = hovered.and_then(|h| self.modules.iter().position(|m| m.index == h))
        else {
            return;
        };

        let scale = view.layout_scale;
        if (text.shaped_scale - scale).abs() > f32::EPSILON {
            let size_px = self.style.size_px * scale;
            let attrs = Attrs::new()
                .family(Family::Name(&text.family))
                .weight(Weight(self.style.weight));
            for (buffer, label) in text.buffers.iter_mut().zip(&self.texts) {
                buffer.set_metrics(Metrics::new(size_px, size_px * LINE_HEIGHT));
                buffer.set_size(None, None);
                buffer.set_text(label, &attrs, Shaping::Advanced, None);
                buffer.shape_until_scroll(&mut text.font_system, false);
            }
            text.shaped_scale = scale;
        }

        let buffer = &text.buffers[slot];
        let Some(run) = buffer.layout_runs().next() else {
            return;
        };
        let anchor = self.modules[slot].tag_anchor(view);
        let baseline_y = anchor.y + self.style.baseline_below_footprint_px * scale;
        // Whole pixels keep the glyphs crisp.
        let left = (anchor.x - run.line_w * 0.5).round();
        let top = (baseline_y - run.line_y).round();
        let [r, g, b] = [self.style.color.r, self.style.color.g, self.style.color.b];
        let area = TextArea {
            buffer,
            left,
            top,
            scale: 1.0,
            bounds: TextBounds::default(),
            default_color: Color::rgb(r, g, b),
            custom_glyphs: &[],
        };

        let [width, height] = view.viewport_px.to_array().map(whole_px);
        text.viewport.update(queue, Resolution { width, height });
        // Glyphs not used since the last frame may be evicted when the atlas fills up.
        text.atlas.trim();
        match text.renderer.prepare(
            device,
            queue,
            &mut text.font_system,
            &mut text.atlas,
            &text.viewport,
            [area],
            &mut text.swash_cache,
        ) {
            Ok(()) => text.has_text = true,
            Err(error) => tracing::warn!(%error, "cannot lay out the elevation tag"),
        }
    }

    /// Draws what [`Self::prepare`] laid out, in a pass on the resolved frame.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(text) = &self.text else {
            return;
        };
        if !text.has_text {
            return;
        }
        if let Err(error) = text.renderer.render(&text.atlas, &text.viewport, pass) {
            tracing::warn!(%error, "cannot draw the elevation tag");
        }
    }

    /// Whether the last `prepare` laid out a tag.
    pub fn has_text(&self) -> bool {
        self.text.as_ref().is_some_and(|t| t.has_text)
    }
}

/// A viewport size in whole pixels.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "viewport sizes are positive and far below u32::MAX"
)]
fn whole_px(value: f32) -> u32 {
    value.max(1.0).round() as u32
}
