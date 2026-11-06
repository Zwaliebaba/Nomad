// Opaque station voxels, phase-1 doc 01 §2 and doc 05 "Palette".
//
// Vertices carry integer world positions (the z scale lives in view_proj, so meshes stay integer),
// a palette index and a face id. Each face gets one flat colour: the palette albedo times the
// camera-relative factor of its face, top 1.00, left-facing 0.77, right-facing 0.48. Side faces
// are classified by the view-space x of their normal, so the light turns with the camera and every
// view looks alike. Emissive entries ignore the factor and render at full albedo (bloom is M3).

struct Voxels {
    view_proj: mat4x4<f32>,
    // xy: screen-right direction on the ground, the view-space x axis; zw unused.
    view_right: vec4<f32>,
    // x: top factor; y: left-facing factor; z: right-facing factor; w unused.
    shading: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Voxels;
// One texel per palette entry, row-major, PALETTE_WIDTH texels per row. rgb: sRGB albedo;
// a: 1 for emissive entries.
@group(0) @binding(1) var palette: texture_2d<f32>;

const PALETTE_WIDTH: u32 = 256u;

struct VoxelOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat, either) color: vec4<f32>,
};

@vertex
fn vs_voxel(
    @location(0) position: vec4<i32>,
    // x: palette index; y: face id (0 +x, 1 −x, 2 +y, 3 −y, 4 +z, 5 −z).
    @location(1) palette_face: vec2<u32>,
) -> VoxelOut {
    var normals = array<vec3<f32>, 6>(
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, -1.0),
    );
    let normal = normals[min(palette_face.y, 5u)];

    // Top faces are lit fully. Side faces face screen-left or screen-right. Bottom faces are never
    // visible from the fixed downward pitch (and are culled as back faces); they get the darkest
    // factor in case one ever shows.
    var factor = u.shading.x;
    if (normal.z == 0.0) {
        let view_x = dot(normal.xy, u.view_right.xy);
        factor = select(u.shading.z, u.shading.y, view_x < 0.0);
    } else if (normal.z < 0.0) {
        factor = u.shading.z;
    }

    let index = palette_face.x;
    let texel = textureLoad(palette, vec2<u32>(index % PALETTE_WIDTH, index / PALETTE_WIDTH), 0);
    let emissive = texel.a > 0.5;

    var out: VoxelOut;
    out.position = u.view_proj * vec4<f32>(vec3<f32>(position.xyz), 1.0);
    out.color = vec4<f32>(select(texel.rgb * factor, texel.rgb, emissive), 1.0);
    return out;
}

@fragment
fn fs_voxel(in: VoxelOut) -> @location(0) vec4<f32> {
    return in.color;
}
