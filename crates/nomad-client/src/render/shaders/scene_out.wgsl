// Fragment output of every pipeline in the scene pass (render/context.rs, "Emissive target").
// Location 0 is the frame colour, location 1 the emissive image that feeds the bloom. Both targets
// of a pipeline share one blend state and write mask (the GLES fallback has no independent blend),
// so surfaces that must not glow write zero there: with premultiplied alpha that leaves the
// emissive target unchanged, and opaque surfaces overwrite it with black where they occlude.

struct SceneOut {
    @location(0) color: vec4<f32>,
    @location(1) emissive: vec4<f32>,
};

// A colour that does not glow.
fn no_glow(color: vec4<f32>) -> SceneOut {
    var out: SceneOut;
    out.color = color;
    out.emissive = vec4<f32>(0.0);
    return out;
}
