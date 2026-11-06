# 05 — Architecture (custom wgpu + winit)

A proposal; adapt module names to the existing project's conventions.

## Dependencies

`wgpu`, `winit`, `glam`, `bytemuck`, `serde` + `serde_json`, `glyphon` (text via cosmic-text), `image` (screenshots and golden-image tests), `notify` (debug hot reload). Optional: `egui` + `egui-wgpu` for a **debug** overlay only — the game HUD is custom-drawn to match the spec exactly.

## Module layout

```
src/
  main.rs              winit event loop, window, frame pacing
  app.rs               owns state, routes input, ticks animations
  config/              serde structs for scene_config, hud_layout, fixtures; hot reload
  assets/voxel.rs      load station_scene / ships, dense chunks per module
  mesh/greedy.rs       greedy mesher -> vertex buffers (pos, normal-id, palette idx)
  camera.rs            ortho dimetric, snap yaw with tween, zoom steps, pan, screen<->world
  render/
    context.rs         device, queue, surface, depth, MSAA (4x)
    background.rs      gradient + nebula + stars (screen space)
    grid.rs            infinite ops grid (full-screen ray/plane pass)
    voxels.rs          opaque pass; palette as storage buffer/texture; camera-relative face shading
    ships.rs           instanced ship meshes with model matrices
    shield.rs          transparent dome, sectors, hit ripples (hex pattern in shader)
    fx.rs              bolts, trails, flashes, lasers (instanced quads, additive)
    overlay.rs         world-space lines: footprints, stalks, zone ellipses, brackets
    bloom.rs           emissive bright-pass + small blur + composite
    hud/               panels, text, bars, buttons, minimap (screen-space quads + glyphon)
  fixtures/            phase-1 data feed for HUD/world labels
tests/
  golden.rs            renders view 0..3 at 1920x1080 and compares against reference images
```

## Frame order

1. Background (screen space)
2. Ops grid (depth test vs cleared depth; writes nothing)
3. Opaque voxels (station modules) → depth
4. Ships (opaque) → depth
5. Elevation overlays and stalks (depth test on, depth write off, dashed in shader)
6. Shield dome: back faces, then front faces (alpha blend, depth test on, write off)
7. FX (additive)
8. Bloom on emissive + FX
9. World labels and markers (screen space, after projecting anchors)
10. HUD panels + minimap
11. Present; optional F12 screenshot

## Rendering notes

- **Palette**: upload the 339-colour palette as a storage buffer; vertices carry a `u16` palette index and a face-direction id. The fragment shader applies the top/left/right factor by *view-space* normal so lighting rotates with the camera.
- **Pixel crispness**: at integer zoom, snap the camera translation to whole pixels; use MSAA, not FXAA, on voxels.
- **z scale 0.8165**: put it in the world → view matrix so meshes stay integer.
- **Grid**: reconstruct world position on `z = 0` per pixel from the inverse view-projection; line intensity via `fwidth` for anti-aliasing; alpha = mix(near, far, smoothstep(r_near, r_far, distance_to_pivot)) with `far > 0`.
- **Shield**: generate an ellipsoid-cap mesh (64×24 segments). Sector by `atan2` of the fragment's world xy relative to the centre. Fresnel from the view-space normal. Hits are a small uniform array (position, start time) driving a hex-cell ripple.
- **Text**: glyphon with JetBrains Mono and Rajdhani; cache layout per string; HUD at 1920×1080 logical units scaled by `min(w/1920, h/1080)`.
- Performance budget: under 2 ms GPU at 1080p on integrated graphics for the voxel pass (about 17k voxels → about 20k faces after greedy meshing).

## Testing

- Golden images: render view 0 with the HUD off and compare against `reference/renders/ops-grid-scene-no-hud.png` with a perceptual tolerance (layout and colour, not pixel-exact: ships are placeholders and the effects differ). Views 1–3 compare against `station-view-1..3.png` for silhouette only.
- Unit tests: projection formula (01 §1) round-trip; snap rotation maps view 0 → 1 → 2 → 3 → 0; the greedy mesher produces watertight output for each module.
