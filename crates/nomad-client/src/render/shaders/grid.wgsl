// Infinite ops grid, phase-1 doc 01 §4 and doc 05 "Grid".
//
// A full-screen pass: every pixel unprojects its view ray with the inverse view-projection and
// intersects it with the ops-grid plane z = 0, so the grid reaches every screen edge at any zoom.
// Lines are anti-aliased analytically from screen-space derivatives. Alpha fades linearly from
// alpha_near to alpha_far with ground distance from the camera pivot and never reaches zero.

struct Grid {
    inv_view_proj: mat4x4<f32>,
    // xy: viewport size in pixels.
    viewport: vec4<f32>,
    // xy: camera pivot on the plane; z: near radius; w: far radius, in voxels.
    pivot_fade: vec4<f32>,
    // rgb: colour; w: line width in pixels.
    minor_color: vec4<f32>,
    // x: alpha near; y: alpha far; z: spacing; w unused.
    minor_params: vec4<f32>,
    // xy: offset of the lines (x ≡ offset.x, y ≡ offset.y mod spacing).
    minor_offset: vec4<f32>,
    major_color: vec4<f32>,
    major_params: vec4<f32>,
    major_offset: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Grid;

// The point on z = 0 seen through a window pixel.
fn ground_point(window_px: vec2<f32>) -> vec2<f32> {
    let ndc = vec2<f32>(
        window_px.x / u.viewport.x * 2.0 - 1.0,
        1.0 - window_px.y / u.viewport.y * 2.0,
    );
    let near = u.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    let far = u.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    let a = near.xyz / near.w;
    let b = far.xyz / far.w;
    return mix(a.xy, b.xy, a.z / (a.z - b.z));
}

// Coverage of a line of `width_px` by a one-pixel box filter, `distance_px` from its centre line.
fn line_coverage(distance_px: f32, width_px: f32) -> f32 {
    let half_width = 0.5 * width_px;
    return clamp(min(distance_px + 0.5, half_width) - max(distance_px - 0.5, -half_width), 0.0, 1.0);
}

// Distance in pixels from the nearest line, per axis. `cell` is the position in grid cells
// (lines at integers); `cell_per_px` is the length of its screen gradient per axis. The exact
// gradient length is used rather than fwidth's |dx| + |dy|, which would thin every line by up to
// a third because all grid lines are diagonal on screen.
fn line_distance_px(cell: vec2<f32>, cell_per_px: vec2<f32>) -> vec2<f32> {
    return abs(cell - round(cell)) / max(cell_per_px, vec2<f32>(1e-6));
}

@fragment
fn fs_grid(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = ground_point(frag.xy);

    let minor_spacing = u.minor_params.z;
    let major_spacing = u.major_params.z;
    let minor_cell = (p - u.minor_offset.xy) / minor_spacing;
    let major_cell = (p - u.major_offset.xy) / major_spacing;

    // Derivatives in uniform control flow, before any branching.
    let minor_dx = dpdx(minor_cell);
    let minor_dy = dpdy(minor_cell);
    let minor_grad = vec2<f32>(
        length(vec2<f32>(minor_dx.x, minor_dy.x)),
        length(vec2<f32>(minor_dx.y, minor_dy.y)),
    );
    let major_grad = minor_grad * (minor_spacing / major_spacing);

    let minor_dist = line_distance_px(minor_cell, minor_grad);
    let major_dist = line_distance_px(major_cell, major_grad);

    // A minor line that coincides with a major line is drawn only as the major line, as in the
    // mockup, so major lines do not get the minor colour stacked underneath.
    let nearest_minor = u.minor_offset.xy + round(minor_cell) * minor_spacing;
    let in_major = (nearest_minor - u.major_offset.xy) / major_spacing;
    let minor_is_major = abs(in_major - round(in_major)) < vec2<f32>(1e-3);

    var minor_cov = vec2<f32>(
        line_coverage(minor_dist.x, u.minor_color.w),
        line_coverage(minor_dist.y, u.minor_color.w),
    );
    minor_cov = select(minor_cov, vec2<f32>(0.0), minor_is_major);
    let major_cov = vec2<f32>(
        line_coverage(major_dist.x, u.major_color.w),
        line_coverage(major_dist.y, u.major_color.w),
    );

    // Linear, like the mockup's six equal steps (D-059; render/grid.rs). far > near is validated.
    let fade = clamp(
        (distance(p, u.pivot_fade.xy) - u.pivot_fade.z) / (u.pivot_fade.w - u.pivot_fade.z),
        0.0,
        1.0,
    );
    // Each family is one stroke, so crossings take the union (max), not a double blend.
    let minor_a = mix(u.minor_params.x, u.minor_params.y, fade) * max(minor_cov.x, minor_cov.y);
    let major_a = mix(u.major_params.x, u.major_params.y, fade) * max(major_cov.x, major_cov.y);

    // Major over minor, premultiplied.
    let alpha = major_a + minor_a * (1.0 - major_a);
    let rgb = u.major_color.rgb * major_a + u.minor_color.rgb * minor_a * (1.0 - major_a);
    return vec4<f32>(rgb, alpha);
}
