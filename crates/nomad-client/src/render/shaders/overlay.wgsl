// World-space overlays, phase-1 doc 05 frame order step 5: the elevation footprints and stalks of
// doc 01 §7. Depth test on, depth write off, dashed in the shader.
//
// Lines are quads expanded in screen space around the projected segment, one pixel wider on every
// side than the stroke for anti-aliasing. Coverage across and along the stroke and the dash pattern
// are box-filtered over one pixel, like the ops grid. Dash lengths are in window pixels and are
// measured from the segment start plus a phase, so a path's dashes run on across its corners.
// Fills (stroke width 0) are plain triangles at their world positions.

struct Overlay {
    view_proj: mat4x4<f32>,
    // xy: viewport size in pixels; zw unused.
    viewport: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Overlay;

const AA_MARGIN_PX: f32 = 1.0;

struct OverlayIn {
    // Segment start (or, for fills, the vertex itself) and end, world coordinates.
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
    // x: 0 at the start, 1 at the end; y: side of the centre line, -1 or +1. Unused for fills.
    @location(2) corner: vec2<f32>,
    // Straight (not premultiplied) colour and opacity.
    @location(3) color: vec4<f32>,
    // x: dash length; y: gap length (0: solid); z: dash phase at the start; w: stroke width,
    // all in window pixels. Width 0 marks a fill.
    @location(4) dash: vec4<f32>,
};

struct OverlayOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // x: distance along the stroke from its start; y: distance across from the centre line;
    // z: stroke length; w: stroke width (0 for fills). Window pixels.
    @location(1) stroke: vec4<f32>,
    // x: dash; y: gap; z: phase.
    @location(2) dash: vec3<f32>,
};

// Clip position → window-pixel offsets from the viewport centre, y up.
fn to_px(clip: vec4<f32>) -> vec2<f32> {
    return clip.xy / clip.w * 0.5 * u.viewport.xy;
}

@vertex
fn vs_overlay(in: OverlayIn) -> OverlayOut {
    var out: OverlayOut;
    out.color = in.color;
    out.dash = in.dash.xyz;
    let width = in.dash.w;
    let a = u.view_proj * vec4<f32>(in.start, 1.0);
    if (width <= 0.0) {
        out.position = a;
        out.stroke = vec4<f32>(0.0);
        return out;
    }
    let b = u.view_proj * vec4<f32>(in.end, 1.0);
    let pa = to_px(a);
    let pb = to_px(b);
    let delta = pb - pa;
    let len = length(delta);
    let dir = select(vec2<f32>(1.0, 0.0), delta / max(len, 1e-6), len > 1e-4);
    let normal = vec2<f32>(-dir.y, dir.x);
    let along = in.corner.x * len + (in.corner.x * 2.0 - 1.0) * AA_MARGIN_PX;
    let across = in.corner.y * (0.5 * width + AA_MARGIN_PX);
    let p = pa + dir * along + normal * across;
    // The view is orthographic (w = 1), so depth is linear along the segment.
    let depth = mix(a.z / a.w, b.z / b.w, in.corner.x);
    out.position = vec4<f32>(p / (0.5 * u.viewport.xy), depth, 1.0);
    out.stroke = vec4<f32>(along, across, len, width);
    return out;
}

// How much of the one-pixel box centred on x lies inside [lo, hi].
fn box_coverage(lo: f32, hi: f32, x: f32) -> f32 {
    return clamp(min(x + 0.5, hi) - max(x - 0.5, lo), 0.0, 1.0);
}

@fragment
fn fs_overlay(in: OverlayOut) -> SceneOut {
    var alpha = in.color.a;
    let width = in.stroke.w;
    if (width > 0.0) {
        let along = in.stroke.x;
        alpha *= box_coverage(-0.5 * width, 0.5 * width, in.stroke.y);
        alpha *= box_coverage(0.0, in.stroke.z, along);
        let dash = in.dash.x;
        let gap = in.dash.y;
        if (gap > 0.0) {
            let period = dash + gap;
            let s = along + in.dash.z;
            let phase = s - floor(s / period) * period;
            // The pixel box is narrower than a period, so the dashes of this, the previous and the
            // next period are all it can touch.
            let on = box_coverage(0.0, dash, phase)
                + box_coverage(period, period + dash, phase)
                + box_coverage(-period, dash - period, phase);
            alpha *= min(on, 1.0);
        }
    }
    // Premultiplied; overlays do not glow.
    return no_glow(vec4<f32>(in.color.rgb * alpha, alpha));
}
