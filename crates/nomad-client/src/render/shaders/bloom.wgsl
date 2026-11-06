// Bloom on emissive surfaces, phase-1 doc 01 §2 and doc 05 frame order step 8 (render/bloom.rs).
//
// The scene pass leaves the emissive colours (and nothing else) in their own target, so that image
// already is the bright pass. It is averaged down to half resolution, blurred with a separable
// Gaussian (horizontal, then vertical), and added to the frame times the intensity. Every value is
// sRGB-encoded, like the rest of the frame (render/context.rs).

struct Bloom {
    // xy: one texel step of the source in uv along the blur direction (zero for the downsample
    // and composite); z: Gaussian sigma in source texels; w: taps on each side of the centre.
    params: vec4<f32>,
    // xy: size of the destination in pixels; z: composite intensity; w unused.
    target_size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Bloom;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var linear_clamp: sampler;

fn uv_of(frag: vec4<f32>) -> vec2<f32> {
    return frag.xy / u.target_size.xy;
}

// Half resolution: one bilinear sample at the shared corner of four source texels is their mean.
@fragment
fn fs_downsample(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, linear_clamp, uv_of(frag), 0.0);
}

@fragment
fn fs_blur(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(frag);
    let sigma = max(u.params.z, 1e-3);
    let taps = i32(u.params.w);
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var i = -taps; i <= taps; i++) {
        let x = f32(i);
        let weight = exp(-x * x / (2.0 * sigma * sigma));
        sum += weight * textureSampleLevel(source, linear_clamp, uv + u.params.xy * x, 0.0);
        total += weight;
    }
    return sum / total;
}

// Drawn onto the resolved frame with additive (or screen) blending; bilinear filtering upsamples
// the glow. The blend leaves the frame's alpha alone.
@fragment
fn fs_composite(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let glow = textureSampleLevel(source, linear_clamp, uv_of(frag), 0.0);
    return vec4<f32>(glow.rgb * u.target_size.z, 0.0);
}
