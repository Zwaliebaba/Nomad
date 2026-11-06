# 00 — Brief

## Vision

A long-lived multiplayer space RTS with a Homeworld-like premise: the player starts with a **mother station** and grows an empire of factories and shipyards. The look is an **isometric voxel diorama**: visible voxels, military HUD, readable at a glance. The camera is a fixed-pitch "pane of glass" with snap rotation — not a free 3D/FPS camera.

## Phase 1 goal

Prove the look and the rendering architecture with one scene: *Holding KR-114, mother station MS-01 "Aegis" under siege*. Target: the scene in `reference/renders/ops-grid-hud@2x.png`, rotatable to the four views in `reference/renders/station-4-views.png`.

### In scope

- Voxel scene rendering from `station_scene.json` (all modules, 4 views)
- Infinite ops grid with distance fade (never ends at the screen edge)
- Sectored power-shield dome with a weakened sector and hit ripples
- Elevation cues for lifted structures (footprint on grid, stalks, ELEV tag)
- Placeholder ships at their placements, static or on simple scripted loops
- Cosmetic combat effects: laser bolts, hit flashes, ranging laser, engine glows (scripted, not simulated)
- Full HUD layout as data-driven overlay; minimap with iso pulse; camera widget reflects the current view
- 4-way snap rotation, zoom steps, panning

### Out of scope (phase 2+)

Simulation, economy, building placement, unit control, picking/selection logic beyond hover highlight, networking, audio, save/load, system/sector/galaxy views (tabs render but do nothing), real minimap click-to-move.

## Decisions already taken (do not re-open)

| Topic | Decision | Why |
|---|---|---|
| Defence | No perimeter walls. A **sectored shield** projected by four pylons (N/E/S/W); each sector has its own strength | Walls are meaningless in 3D space; sectors give attackers choices and tie defence to power |
| Power | The solar array SOL-1 sits **outside** the dome and feeds the shield via a visible power bus | Gives the array a reason to be defended |
| Centre piece | **Core spindle** with habitat ring, four docking arms and sensor mast replaces the old keep | Keeps a clear centre, reads as a station not a castle |
| Elevation | Structures snap to **tiers 0 / 4 / 8 / 12**; FA-1 at +8, FB-2 at +4. All **ships fly on one plane** (z = 16) | Readable in a fixed-pitch view |
| Height cues | Footprint outline on the ops grid + dashed stalks; ELEV tag only on hover/selection | Shows altitude without camera pitch changes |
| Camera | Fixed dimetric pitch, **4 snap yaw angles** (Q/E), camera-relative lighting | Pane-of-glass readability; voxels stay crisp |
| Physics look | No cast shadows on the ops grid (there is no surface). No loose cargo: containers sit in clamped racks. Aviation terms banned (use ELEV, station-keeping, quadrant) | Space plausibility |
| Lights | Emissive voxels limited (about 100 voxels, roughly 40 light points, on the station): pylon emitters, docking collars, power-bus markers, sparse running lights | Earlier versions were unreadable with light rows |
| HUD | Event log removed; system/navigation panel top-left; command card is a 4×3 hotkey grid; minimap is an iso radar with pulse | RTS readability review |
| Minimap orientation | North-up minimap; the camera frame on it rotates with the view | Allies can call positions consistently |
