// The shield's surface, phase-1 doc 05 frame order step 6: a translucent ellipsoid (Design D-100:
// a globe centred on the ship plane), drawn twice with alpha blending, depth test on and depth
// write off: back faces first, then front faces (doc 01 §6). The rim and the sector ring are
// strokes drawn by `render/lines.rs`, not here.
//
// The mesh is a unit sphere; the vertex shader scales it to the ellipsoid. Facing and fresnel use
// the normal in the projection's z-scaled space, where the view is a rigid orthographic camera,
// so a fragment is front-facing when its normal points against the view direction. The ripples of
// the scripted hits (doc 06 M4) add a hex pattern, an outline and a hot flash; the flash also
// feeds the bloom (doc 05 step 8).

struct Shield {
    view_proj: mat4x4<f32>,
    // xyz: centre (world voxels); w: 1 for the dome shape (only z >= centre is drawn), else 0.
    center: vec4<f32>,
    // xyz: semi-axes (world voxels); w: the projection's z scale.
    radii: vec4<f32>,
    // xyz: view direction into the screen, z-scaled space, unit; w: below-grid opacity factor.
    view_dir: vec4<f32>,
    // x: rest fill alpha; y: edge fill alpha; z: fresnel power; w: hex cell width (voxels).
    fill: vec4<f32>,
    // rgb: fill colour; w: number of active hits.
    fill_color: vec4<f32>,
    // rgb: hex line colour; w: hex alpha.
    hex_color: vec4<f32>,
    // rgb: ripple outline colour in a healthy sector; w: outline alpha.
    outline: vec4<f32>,
    // rgb: ripple outline colour in a weak sector; w unused.
    outline_weak: vec4<f32>,
    // rgb: flash centre colour; w: flash radius (voxels).
    flash_core: vec4<f32>,
    // rgb: flash edge colour; w: voxels per window pixel on a surface facing the viewer.
    flash_edge: vec4<f32>,
    // Per hit, xyz: impact point (world); w: current ripple radius (voxels).
    hit_pos: array<vec4<f32>, 4>,
    // Per hit, xyz: outward surface normal at the impact (world, unit); w: ripple fade.
    hit_normal: array<vec4<f32>, 4>,
    // Per hit, x: 1 in a weak sector; y: flash intensity; zw unused.
    hit_info: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> u: Shield;

struct ShieldIn {
    @location(0) unit: vec3<f32>,
};

struct ShieldOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) unit: vec3<f32>,
};

@vertex
fn vs_shield(in: ShieldIn) -> ShieldOut {
    var out: ShieldOut;
    let world = u.center.xyz + u.radii.xyz * in.unit;
    out.position = u.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.unit = in.unit;
    return out;
}

// Distance from p to the nearest edge of a pointy-top hex tiling with cells one unit across
// (0 on an edge, 0.5 at a cell centre).
fn hex_edge_distance(p: vec2<f32>) -> f32 {
    let r = vec2<f32>(1.0, 1.7320508);
    let h = r * 0.5;
    let a = p - r * floor(p / r) - h;
    let pb = p - h;
    let b = pb - r * floor(pb / r) - h;
    let g = select(b, a, dot(a, a) < dot(b, b));
    let q = abs(g);
    let d = max(dot(q, vec2<f32>(0.5, 0.8660254)), q.x);
    return 0.5 - d;
}

struct Hits {
    // Premultiplied colour added on top of the fill.
    add: vec3<f32>,
    // The part that glows (the flash).
    glow: vec3<f32>,
};

// The scripted hits at this fragment. `px` is the size of one pixel in voxels on the surface.
fn shade_hits(world: vec3<f32>, px: f32) -> Hits {
    var hits: Hits;
    hits.add = vec3<f32>(0.0);
    hits.glow = vec3<f32>(0.0);
    let count = u32(u.fill_color.w);
    let cell = u.fill.w;
    for (var i = 0u; i < 4u; i = i + 1u) {
        if (i >= count) {
            break;
        }
        let pos = u.hit_pos[i];
        let normal = u.hit_normal[i];
        let info = u.hit_info[i];
        let offset = world - pos.xyz;
        // 3D distance: points within a ripple radius of the impact lie on its side of the globe.
        let d = length(offset);
        let r = pos.w;
        let fade = normal.w;
        if (r > 0.0) {
            // Hex cells on the tangent plane at the impact, inside the growing disc, brighter
            // towards its front, fading out over the ripple.
            var t1 = cross(vec3<f32>(0.0, 0.0, 1.0), normal.xyz);
            if (dot(t1, t1) < 1e-6) {
                t1 = vec3<f32>(1.0, 0.0, 0.0);
            }
            t1 = normalize(t1);
            let t2 = cross(normal.xyz, t1);
            let q = vec2<f32>(dot(offset, t1), dot(offset, t2)) / cell;
            let aa = max(px / cell, 1e-4);
            let line = 1.0 - smoothstep(0.05, 0.05 + aa, hex_edge_distance(q));
            let inside = 1.0 - smoothstep(r - px, r + px, d);
            let front = smoothstep(r - 2.0 * cell, r, d);
            let hex = u.hex_color.w * line * inside * (0.35 + 0.65 * front) * fade;
            let ring = (1.0 - smoothstep(0.0, 1.5 * px, abs(d - r))) * u.outline.w * fade;
            let outline = mix(u.outline.rgb, u.outline_weak.rgb, info.x);
            hits.add = hits.add + u.hex_color.rgb * hex + outline * ring;
        }
        let flash = info.y;
        if (flash > 0.0) {
            let t = d / u.flash_core.w;
            let color = mix(u.flash_core.rgb, u.flash_edge.rgb, smoothstep(0.0, 0.3, t));
            let glow = color * (1.0 - smoothstep(0.3, 1.0, t)) * flash;
            hits.add = hits.add + glow;
            hits.glow = hits.glow + glow;
        }
    }
    return hits;
}

fn shade(in: ShieldOut, front: bool) -> SceneOut {
    // Derivatives first, while control flow is uniform. Near the silhouette the surface is seen
    // edge-on and one pixel spans many voxels, so the footprint is clamped to between one and
    // three pixels' worth on a facing surface; otherwise the anti-aliasing would smear the hits
    // and the grid line across the rim.
    let vpp = u.flash_edge.w;
    let fw = fwidth(in.world);
    let px = clamp(max(max(fw.x, fw.y), fw.z), vpp, 3.0 * vpp);
    let below_aa = clamp(fwidth(in.world.z), 1e-4, 3.0 * vpp);

    let a = u.radii.x;
    let c = u.radii.z * u.radii.w;
    let n = normalize(vec3<f32>(in.unit.x / a, in.unit.y / a, in.unit.z / c));
    let facing = dot(n, u.view_dir.xyz);
    let dome_cut = u.center.w > 0.5 && in.world.z < u.center.z;
    if (dome_cut || front != (facing < 0.0)) {
        discard;
    }
    let fresnel = clamp(1.0 - abs(facing), 0.0, 1.0);
    let alpha = mix(u.fill.x, u.fill.y, pow(fresnel, u.fill.z));
    let hits = shade_hits(in.world, px);
    // Everything below the ops grid is fainter (D-100).
    let below = mix(u.view_dir.w, 1.0, smoothstep(-below_aa, below_aa, in.world.z));
    var out: SceneOut;
    out.color = vec4<f32>((u.fill_color.rgb * alpha + hits.add) * below, alpha * below);
    // Alpha 0: the glow adds to the emissive image without dimming what is already there.
    out.emissive = vec4<f32>(hits.glow * below, 0.0);
    return out;
}

@fragment
fn fs_shield_back(in: ShieldOut) -> SceneOut {
    return shade(in, false);
}

@fragment
fn fs_shield_front(in: ShieldOut) -> SceneOut {
    return shade(in, true);
}
