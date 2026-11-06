//! World-space strokes and fills drawn in the scene pass with `shaders/overlay.wgsl`: the line
//! machinery shared by the elevation overlays (phase-1 doc 05 frame order step 5, `overlay.rs`)
//! and the shield's rim and sector ring (step 6, `shield.rs`).
//!
//! A stroke is a world-space segment drawn with a width and a dash pattern in window pixels; a
//! fill is a world-space triangle pair. Both are depth-tested against the voxels and write no
//! depth (doc 05 steps 5 and 6), blend with premultiplied alpha, and do not glow. The geometry is
//! rebuilt on the CPU every frame by the owner of each [`LineBatch`] and uploaded whole.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::util::DeviceExt;

use super::context::TargetFormat;
use crate::camera::ViewTransform;
use crate::config::CueLineStyle;

/// Vertices per stroke or fill quad (two triangles, not indexed).
pub const QUAD_VERTICES: usize = 6;

/// One line vertex (`OverlayIn` in the shader). 64 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct LineVertex {
    pub(super) start: [f32; 3],
    pub(super) end: [f32; 3],
    pub(super) corner: [f32; 2],
    pub(super) color: [f32; 4],
    /// Dash, gap, phase and stroke width in window pixels; width 0 marks a fill.
    pub(super) dash: [f32; 4],
}

/// A dashed stroke style in window pixels. A gap of 0 draws a solid line.
#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    /// Straight (not premultiplied) colour and opacity.
    pub color: [f32; 4],
    pub dash_px: f32,
    pub gap_px: f32,
    pub width_px: f32,
}

impl Stroke {
    /// An elevation-cue style (1080p pixels) scaled to the window.
    pub fn from_cue(style: &CueLineStyle, layout_scale: f32) -> Self {
        Self {
            color: style.color.to_vec4(style.alpha),
            dash_px: style.dash_ref_px[0] * layout_scale,
            gap_px: style.dash_ref_px[1] * layout_scale,
            width_px: style.width_ref_px * layout_scale,
        }
    }
}

/// Appends a stroke from `start` to `end`. `phase_px` is how far into the dash pattern the start
/// lies, so a path's segments continue each other's dashes; `opacity` scales the stroke's alpha.
pub fn push_stroke(
    out: &mut Vec<LineVertex>,
    start: Vec3,
    end: Vec3,
    stroke: &Stroke,
    phase_px: f32,
    opacity: f32,
) {
    let mut color = stroke.color;
    color[3] *= opacity;
    let vertex = |along: f32, side: f32| LineVertex {
        start: start.to_array(),
        end: end.to_array(),
        corner: [along, side],
        color,
        dash: [stroke.dash_px, stroke.gap_px, phase_px, stroke.width_px],
    };
    let [a, b, c, d] = [
        vertex(0.0, -1.0),
        vertex(1.0, -1.0),
        vertex(1.0, 1.0),
        vertex(0.0, 1.0),
    ];
    out.extend([a, b, c, a, c, d]);
}

/// Appends a filled quad with the given straight colour.
pub fn push_fill(out: &mut Vec<LineVertex>, quad: [Vec3; 4], color: [f32; 4]) {
    let vertex = |p: Vec3| LineVertex {
        start: p.to_array(),
        end: p.to_array(),
        corner: [0.0, 0.0],
        color,
        dash: [0.0; 4],
    };
    let [a, b, c, d] = quad.map(vertex);
    out.extend([a, b, c, a, c, d]);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct LineUniforms {
    view_proj: [[f32; 4]; 4],
    viewport: [f32; 4],
}

/// A vertex buffer of strokes and fills with its pipeline, uploaded once per frame and drawn in
/// one or more ranges.
pub struct LineBatch {
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    capacity: usize,
    count: u32,
}

impl LineBatch {
    /// A batch holding at most `capacity` vertices.
    pub fn new(device: &wgpu::Device, target: TargetFormat, capacity: usize, label: &str) -> Self {
        let capacity = capacity.max(1);
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::bytes_of(&LineUniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[super::uniform_entry(
                wgpu::ShaderStages::VERTEX,
                size_of::<LineUniforms>(),
            )],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (capacity * size_of::<LineVertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            uniform_buffer,
            bind_group,
            pipeline: line_pipeline(device, &layout, target, label),
            vertex_buffer,
            capacity,
            count: 0,
        }
    }

    /// Uploads this frame's view and vertices. Vertices beyond the capacity are dropped (a debug
    /// build asserts that none are).
    pub fn upload(&mut self, queue: &wgpu::Queue, view: &ViewTransform, vertices: &[LineVertex]) {
        let uniforms = LineUniforms {
            view_proj: view.view_proj().to_cols_array_2d(),
            viewport: [view.viewport_px.x, view.viewport_px.y, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        debug_assert!(vertices.len() <= self.capacity);
        let vertices = &vertices[..vertices.len().min(self.capacity)];
        self.count = u32::try_from(vertices.len())
            .expect("invariant: the capacity is a few thousand vertices");
        if !vertices.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(vertices));
        }
    }

    /// Number of vertices uploaded for this frame.
    pub fn len(&self) -> u32 {
        self.count
    }

    /// Draws the uploaded vertices in `range` (clamped to what was uploaded).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, range: Range<u32>) {
        let range = range.start.min(self.count)..range.end.min(self.count);
        if range.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(range, 0..1);
    }
}

fn line_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    target: TargetFormat,
    label: &str,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("overlay.wgsl"),
        source: wgpu::ShaderSource::Wgsl(super::OVERLAY_WGSL.into()),
    });
    let attributes = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x4,
        4 => Float32x4,
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_overlay"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<LineVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        // Doc 05 steps 5 and 6: depth test on, depth write off. Footprints lie exactly on the
        // plane of the deck's bottom faces, so equal depths pass.
        depth_stencil: Some(super::context::depth_stencil(
            false,
            Some(wgpu::CompareFunction::LessEqual),
        )),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_overlay"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &super::scene_targets(
                target,
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            ),
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_layout_matches_the_shader() {
        assert_eq!(size_of::<LineVertex>(), 64);
        assert_eq!(size_of::<LineUniforms>(), 80);
    }

    #[test]
    fn a_stroke_is_one_quad_with_its_style() {
        let stroke = Stroke {
            color: [1.0, 0.5, 0.25, 0.8],
            dash_px: 4.0,
            gap_px: 5.0,
            width_px: 2.0,
        };
        let mut out = Vec::new();
        push_stroke(&mut out, Vec3::ZERO, Vec3::X, &stroke, 3.0, 0.5);
        assert_eq!(out.len(), QUAD_VERTICES);
        assert!(out.iter().all(|v| {
            v.dash
                .iter()
                .zip([4.0, 5.0, 3.0, 2.0])
                .all(|(a, b)| (a - b).abs() < 1e-6)
        }));
        assert!((out[0].color[3] - 0.4).abs() < 1e-6);
    }
}
