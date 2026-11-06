// Background, phase-1 doc 01 §3: radial gradient, up to four blurred nebula ellipses (the config
// has three), a soft sun glow and about 200 stars. Screen space; it does not rotate with the camera.
//
// The background is a 1920×1080 reference canvas scaled to cover the window ("cover" fit), so a
// 16:9 window shows exactly the mockup layout. All colours are sRGB-encoded and blended as such,
// like the browser that produced the reference renders (see render/context.rs).

struct Background {
    // xy: viewport size in pixels; zw: window position of the canvas origin in pixels.
    viewport_offset: vec4<f32>,
    // x: window pixels per reference pixel; y: reference canvas width; z: height.
    canvas: vec4<f32>,
    // xy: gradient centre in canvas units (0..1); z: radius in canvas units; w: middle stop.
    gradient: vec4<f32>,
    color_center: vec4<f32>,
    color_mid: vec4<f32>,
    color_edge: vec4<f32>,
    // Per nebula: centre xy and radii zw in reference pixels.
    nebula_shape: array<vec4<f32>, 4>,
    // Per nebula: rgb and opacity.
    nebula_color: array<vec4<f32>, 4>,
    // x: cos(rotation); y: sin(rotation); z: blur sigma in reference pixels; w: nebula count (0-4).
    nebula_params: vec4<f32>,
    // xy: sun centre and z: radius in reference pixels; w: group opacity.
    sun_shape: vec4<f32>,
    // Gradient stops of the sun: rgb + stop opacity. Offsets 0, sun_mid_stop.x and 1.
    sun_inner: vec4<f32>,
    sun_mid: vec4<f32>,
    sun_outer: vec4<f32>,
    // x: offset of the middle sun stop.
    sun_mid_stop: vec4<f32>,
    // rgb: star colour.
    star_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Background;

// Window pixel -> reference canvas pixel.
fn to_canvas(window_px: vec2<f32>) -> vec2<f32> {
    return (window_px - u.viewport_offset.zw) / u.canvas.x;
}

// Standard normal CDF, tanh approximation (max error about 2e-4).
fn normal_cdf(x: f32) -> f32 {
    let y = clamp(x, -8.0, 8.0);
    return 0.5 * (1.0 + tanh(0.7978846 * (y + 0.044715 * y * y * y)));
}

// Approximate signed distance to an axis-aligned ellipse with radii r (Inigo Quilez).
fn ellipse_distance(p: vec2<f32>, r: vec2<f32>) -> f32 {
    let k1 = length(p / (r * r));
    if (k1 < 1e-6) {
        return -min(r.x, r.y);
    }
    let k0 = length(p / r);
    return k0 * (k0 - 1.0) / k1;
}

// A Gaussian-blurred filled ellipse (SVG feGaussianBlur on the nebula group), approximated by
// blurring along the distance to the outline.
fn nebula_coverage(p: vec2<f32>, shape: vec4<f32>) -> f32 {
    let d = p - shape.xy;
    let c = u.nebula_params.x;
    let s = u.nebula_params.y;
    // Undo the ellipse rotation: local = R(-angle) * d.
    let local = vec2<f32>(c * d.x + s * d.y, -s * d.x + c * d.y);
    return normal_cdf(-ellipse_distance(local, shape.zw) / u.nebula_params.z);
}

@fragment
fn fs_background(@builtin(position) frag: vec4<f32>) -> SceneOut {
    let p = to_canvas(frag.xy);

    // Radial gradient in canvas units, like SVG objectBoundingBox (an ellipse on screen).
    let uv = p / u.canvas.yz;
    let t = clamp(length(uv - u.gradient.xy) / u.gradient.z, 0.0, 1.0);
    let mid = u.gradient.w;
    var color = select(
        mix(u.color_mid.rgb, u.color_edge.rgb, (t - mid) / (1.0 - mid)),
        mix(u.color_center.rgb, u.color_mid.rgb, t / mid),
        t < mid,
    );

    let nebula_count = min(u32(u.nebula_params.w), 4u);
    for (var i = 0u; i < nebula_count; i++) {
        let coverage = nebula_coverage(p, u.nebula_shape[i]);
        color = mix(color, u.nebula_color[i].rgb, u.nebula_color[i].a * coverage);
    }

    // Sun glow: radial gradient with per-stop opacity, then the group opacity.
    let r = length(p - u.sun_shape.xy) / u.sun_shape.z;
    let sun_mid = u.sun_mid_stop.x;
    var sun = select(
        mix(u.sun_mid, u.sun_outer, clamp((r - sun_mid) / (1.0 - sun_mid), 0.0, 1.0)),
        mix(u.sun_inner, u.sun_mid, r / sun_mid),
        r < sun_mid,
    );
    color = mix(color, sun.rgb, sun.a * u.sun_shape.w);

    return no_glow(vec4<f32>(color, 1.0));
}

struct StarOut {
    @builtin(position) position: vec4<f32>,
    // Offset from the star centre in window pixels.
    @location(0) local_px: vec2<f32>,
    @location(1) radius_px: f32,
    @location(2) alpha: f32,
};

// Instanced quads, 6 vertices per star. Instance: centre (reference px), radius (reference px),
// alpha.
@vertex
fn vs_star(
    @builtin(vertex_index) index: u32,
    @location(0) center_ref: vec2<f32>,
    @location(1) radius_ref: f32,
    @location(2) alpha: f32,
) -> StarOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let radius_px = radius_ref * u.canvas.x;
    // One extra pixel around the disc for anti-aliasing.
    let local_px = corners[index] * (radius_px + 1.0);
    let window_px = u.viewport_offset.zw + center_ref * u.canvas.x + local_px;
    let ndc = vec2<f32>(
        window_px.x / u.viewport_offset.x * 2.0 - 1.0,
        1.0 - window_px.y / u.viewport_offset.y * 2.0,
    );
    var out: StarOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.local_px = local_px;
    out.radius_px = radius_px;
    out.alpha = alpha;
    return out;
}

@fragment
fn fs_star(in: StarOut) -> SceneOut {
    let coverage = clamp(in.radius_px + 0.5 - length(in.local_px), 0.0, 1.0);
    let a = in.alpha * coverage;
    // Premultiplied alpha.
    return no_glow(vec4<f32>(u.star_color.rgb * a, a));
}
