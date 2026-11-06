# 03 — Data formats

All files are UTF-8 JSON. Parse with `serde`; reject unknown `format` versions.

## `assets/voxels/station_scene.json` — `voxel-rts/scene@1`

```jsonc
{
  "format": "voxel-rts/scene@1",
  "origin": "command core centre at deck level (z=0 is the deck)",
  "elevation_tiers": [0, 4, 8, 12],
  "shading": { "top": 1.0, "left": 0.77, "right": 0.48 },
  "palette": [ { "rgb": "#858c9a", "emissive": false }, ... ],   // 339 entries
  "modules": [
    {
      "id": "core_spindle",
      "name": "Command core (centre piece)",
      "elevation_tier": 0,
      "bounds": { "min": [x,y,z], "max": [x,y,z] },
      "voxel_count": 2433,
      "voxels": [[x, y, z, paletteIndex], ...]      // WORLD coordinates, already at their tier height
    }, ...
  ]
}
```

- Voxels are in world coordinates (integers). Lifted modules are already stored at their elevated position; `elevation_tier` tells the renderer to draw footprint and stalks from `bounds.min.z` down to `z = 0`.
- Modules do not overlap. Mesh each module separately (it becomes a selectable entity in phase 2).
- Recommended in-memory form: per module a dense `Vec<u16>` chunk over its bounds (0 = empty, palette index + 1 otherwise), then greedy meshing per face direction and palette index.

## `assets/voxels/ships_placeholder.json` — `voxel-rts/ships@1`

```jsonc
{
  "status": "PLACEHOLDER — blockout models for phase 1, not final art",
  "axes": "+x nose, +y port, +z up; origin ~ centre of mass",
  "palette": { "hull": "#9aa1af", ... },
  "emissive": ["eng", "acc", "h_light"],
  "models": { "vx_interceptor": { "name": "...", "voxels": [[x,y,z,"hull"], ...] }, ... }
}
```

Ships are **free-rotating** (any heading, not grid-aligned), so render them as meshes with a model matrix (translation + yaw), not snapped into the world grid. Scale: same voxel size as the station.

## `assets/layout/ships_placement.json`

```jsonc
{ "ship_plane_z": 16,
  "placements": [ { "model": "hostile_strike", "group": "HOSTILE", "pos": [x,y,z], "heading_deg": 172 }, ... ] }
```

`heading_deg` is measured in world xy from +x toward +y. The WARDEN frigate in the dry dock is "under construction 74%": reveal its voxels bottom-up to 74% and draw the rest as a faint wireframe.

## `assets/layout/scene_config.json`

Projection, camera, grid, shield, pylons, elevation, background and lights policy. Field meanings are in 01 and 02. Load at startup; hot-reload on file change in debug builds (strongly recommended for tuning the look).

## `assets/layout/hud_layout.json`

Panel rectangles (`[x, y, w, h]` at 1920×1080), anchors, colours, fonts, label and marker styles. Panel **content** for phase 1 comes from the fixture `assets/fixtures/siege_kr114.json` (resources, roster, objectives, commanders, lane traffic, selection stats, build queue, command card entries, world labels, pylon tags, lock). The renderer reads only from the fixture.
