# Voxel RTS — Phase 1 design package (Ops grid: base under siege)

This package is the hand-off from the design canvas ("Ops grid — proposal: core spindle + lifted modules") to the Rust implementation. It is written for Claude Code working inside the Rust project. It lives in the repository at `Design/phase1-visual-slice/`; point Claude Code at this README.

## What phase 1 is

A **visual slice**: render the mother-station scene from voxel data with a custom `wgpu` + `winit` renderer, with 4-way snap camera rotation, the infinite ops grid, the sectored power shield, placeholder ships, and the full HUD layout as a static (data-driven, non-simulated) overlay including the animated iso-pulse minimap. No game simulation, no networking, no picking logic beyond what the acceptance criteria list.

## Reading order

| # | File | Purpose |
|---|------|---------|
| 0 | `docs/00-brief.md` | Vision, scope in/out, decisions already taken (do not re-open these) |
| 1 | `docs/01-visual-spec.md` | Projection, voxel shading, palette, grid, shield, effects, labels |
| 2 | `docs/02-camera-and-input.md` | Snap rotation, zoom, pan, key bindings |
| 3 | `docs/03-data-formats.md` | Schemas of every JSON in `assets/`, coordinate system, meshing notes |
| 4 | `docs/04-hud-spec.md` | Every HUD panel, its content and behaviour, minimap pulse algorithm |
| 5 | `docs/05-architecture-wgpu.md` | Crate/module layout, render passes, shaders, dependencies |
| 6 | `docs/06-milestones-acceptance.md` | Ordered milestones with testable acceptance criteria |
| 7 | `docs/07-open-questions.md` | Things deliberately left open for phase 2 |

## Assets

```
assets/voxels/station_scene.json       ~17k voxels, 17 modules, 339-colour palette (reconstructed from the mockup, closed for all 4 views)
assets/voxels/ships_placeholder.json   6 blockout ship models (placeholder art)
assets/layout/scene_config.json        projection, camera, grid, shield, pylons, elevation tiers, background
assets/layout/hud_layout.json          panel rectangles at 1920x1080, colours, fonts, marker styles
assets/layout/ships_placement.json     18 ship placements for the slice (ship plane z = 16)
assets/fixtures/siege_kr114.json       every HUD string and number of the scenario (render code reads only this)
reference/renders/                     target images: HUD composite, scene without HUD, station in all 4 camera views
reference/mockup/ops-grid-proposal.html  the original mockup page (open in a browser; needs internet for Google Fonts)
```

## Ground rules for the implementer

1. The reference renders are the visual target. When the spec and a render disagree, the **spec wins** and the discrepancy is noted in `docs/07-open-questions.md`.
2. All numbers in the spec are at the 1920×1080 reference resolution and zoom 1.0. Scale uniformly.
3. Everything that looks like data (labels, values, roster entries, objectives) comes from a JSON fixture, never hard-coded strings in render code — phase 2 will feed it from the simulation.
4. Fonts (JetBrains Mono, Rajdhani) are OFL-licensed and are **not** bundled; download them into `assets/fonts/`.
