# 01 — Visual specification

All pixel values at 1920×1080, zoom 1.0. Machine-readable values live in `assets/layout/scene_config.json`; this document explains them.

## 1. Coordinate system and projection

- Grid axes: `x`, `y` horizontal, `z` up. One voxel = 1 unit. Origin = command-core centre at the ops-grid plane (`z = 0`; the deck occupies `z ∈ [0,1)`).
- Projection: orthographic **2:1 dimetric**. In view 0 (yaw 45°):

```
sx = 1060 + 5   * (x - y)
sy =  490 + 2.5 * (x + y) - 5 * z        (z is the bottom of a voxel; a voxel spans z..z+1)
```

- As a 3D camera: orthographic, yaw 45° + n·90°, pitch 30° below horizontal, with the grid **z axis scaled by 0.8165** (the mockup's voxels are slightly squat). Implement the z scale in the model matrix so true cubes remain an option behind a config flag.
- Screen-space size: one voxel edge along x or y projects to (5, 2.5) px; along z to 5 px.
- Camera pivot: deck centre `(-8.5, 9.5, 0)`; rotation is about the vertical axis through the pivot.

## 2. Voxel shading

- Each voxel has one **albedo** from the palette. Faces are shaded by which screen-facing side they show, with a **camera-relative** key light so the look is identical in all four views:

| Face | Factor |
|---|---|
| Top (+z) | 1.00 |
| Left-facing side | 0.77 |
| Right-facing side | 0.48 |

- Emissive palette entries (`"emissive": true`) ignore shading, render at full albedo, and feed a bloom/glow pass (small radius, ~6 px at zoom 1, intensity modest). Emissive colours: station cyan `#7ed9ee`/`#4dd9fc`, emitter `#b8f6ff`, beacon red `#ff6a55`.
- The palette already contains slight per-voxel noise (±3–6 per channel). Do not add more texture noise; optional subtle AO on concave edges is allowed but must be off by default.
- No cast shadows anywhere. No fog.
- Cull faces between solid neighbours; greedy-mesh per module per palette entry.

## 3. Background

- Radial gradient: centre (35%, 25%) `#18243a` → 55% `#0c1322` → edge `#060a12`.
- Three large blurred nebula ellipses (alpha 0.35, colours `#2a4a5a`, `#283a55`, `#1e2f45`, rotated −15°), ~200 stars (radius 0.5–1.1 px, alpha 0.15–0.6), soft sun glow top-left.
- Background is screen-space and does not rotate with the camera (stars may parallax very slightly).

## 4. Ops grid

- Infinite plane at `z = 0`. Minor lines every **6** voxels (offset x≡4, y≡0 mod 6), major lines every **24** (offset x≡22, y≡18 mod 24).
- Minor: `#6fd6ff`, ~0.9 px; major: `#8fe2ff`, ~1.2 px. Alpha fades with distance from the camera pivot: `alpha_near` within ~55 voxels → `alpha_far` at the screen edge and beyond. **The grid must never stop at the screen edge** (design requirement: the playing field must read as vast).
- Implement as a full-screen pass that intersects the view ray with `z = 0` and computes anti-aliased lines analytically (fwidth-based). It is drawn before the voxels, depth-tested so voxels occlude it.

## 5. Station modules (from `station_scene.json`)

| Module | Notes |
|---|---|
| `core_spindle` | Centre piece. Octagonal core z 4–23, habitat ring z 12–13, docking arms z 8–9 ending in lit collars at radius 11, cap z 24, mast to z 33 with red beacon, sensor dish at z 29 |
| `pylon_n/e/s/w` | Former corner towers, now shield pylons with a 2×2 cyan emitter column, top face at z ≈ 20 |
| `deck` | Single-voxel deck plate plus low radiator modules (grey striped tops, no red roofs) |
| `factory_fa1` | Tier +8, with clamped container rack on its +y side |
| `factory_fb2` | Tier +4, with container rack |
| `truss` | Skybridges and struts linking deck ↔ FA-1, FB-2, dry dock |
| `solar_array` | SOL-1 panels outside the dome + power bus with cyan markers every 4 voxels into the station |
| `dry_dock` | SY-2 open frame; a WARDEN frigate is "under construction" inside |
| `asteroid_1..5` | Static rocks (AST-1 field) |
| `misc` | Cargo pads, small beacons, a docked MULE |

Data note: the station was reconstructed from the 2D mockup. Hidden back sides were filled heuristically so all four views are closed (see `reference/renders/station-4-views.png`). Small artefacts (e.g. a notch on one side of `pylon_w`) are acceptable in phase 1.

## 6. Shield dome

- Ellipsoid cap centred `(-8.6, 9.4, 0)`, horizontal radius 36, vertical radius 40 (before the 0.8165 z scale).
- At rest: fill alpha ≈ 0.05 with a fresnel rim (`#7fe6ff`, alpha 0.75). Rendered in a transparent pass after opaque voxels, back faces first (dashed rim on the far half of the base ring), then front faces.
- Four **sectors** by azimuth around the dome centre (N 225–315°, E 315–45°, S 45–135°, W 135–225°, measured in grid xy). Each sector's base-ring arc is drawn 2 px; a sector below 50% strength is tinted `#ff9a4a`, dashed, and its rim is also orange.
- Hits: hex-pattern ripple at the impact point (radius 4–6 voxels, 600 ms, additive), plus a hot flash (`#fff6e0` → `#ffb15a`). Projectiles stop at the dome surface.

## 7. Elevation cues

For every structure with tier > 0: dashed footprint outline on the grid (`#38d6ff`, alpha 0.7, fill alpha 0.06), dashed vertical stalks from three visible bottom corners down to the grid, and an `ELEV +n` tag (10 px bold) only on hover or selection. Ships: a short dashed stalk + 6×3 ellipse on the grid under each ship (their plane is constant, but the stalk anchors their ground position).

## 8. Effects (scripted in phase 1)

- Friendly bolts: cyan `#5fe2ff`, 3.4–4.2 px with a 1 px white core, dashed in 8–10 px segments, blurred glow.
- Hostile bolts: orange `#ff9a4a`, same construction.
- Ranging laser: `#ff5a4a`, 1 px dashed 6/5, alpha 0.55, ending at the dome surface with a 4 px ring.
- Engine trails: 3.5 px lines with linear alpha gradient 0.75 → 0, length ~45 px, cyan (friendly) / orange (hostile).
- Construction sparks at the dry dock: small white star bursts.

## 9. Scene labels and markers

See `hud_layout.json → scene_labels / markers`. Labels: dark translucent box with a 3 px accent left border, 22×24 icon, title in accent bold 11 px, value lines 11 px; a 1 px leader polyline ends in a 2 px dot on the object. Hostile labels use the orange border and title colour. Never more than ~8 labels at once; declutter by priority.
