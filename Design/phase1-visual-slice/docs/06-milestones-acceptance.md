# 06 — Milestones and acceptance criteria

Each milestone ends in a runnable build plus an F12 screenshot committed to `screenshots/milestones/`.

## M1 — Window, camera, grid
- A winit window with a wgpu surface at 1920×1080, resizable. *(Design D-108: the client now opens borderless fullscreen by default; `--windowed` gives the 1920×1080 window.)*
- Background and infinite ops grid render; minor/major spacing and colours match 01 §4.
- **Accept**: the grid reaches every screen edge at every zoom level; Q/E rotate in 250 ms; Home resets.

## M2 — Station voxels
- Load `station_scene.json`, greedy-mesh per module, opaque pass with camera-relative face shading.
- **Accept**: view 0 matches the silhouette and colours of `ops-grid-scene-no-hud.png` (excluding ships, shield and effects); views 1–3 match `station-view-1..3.png`; no holes or z-fighting in any view; voxel pass under 2 ms GPU.

## M3 — Emissive and elevation cues
- Bloom on emissive palette entries; footprints and stalks for FA-1 (+8) and FB-2 (+4); hover a module → highlight + ELEV tag.
- **Accept**: the station has no more than about 100 emissive voxels (the data has 98: emitters, collars, power-bus markers, sparse running lights); height is readable without rotating (check by a person against the reference render).

## M4 — Shield dome
- Dome with rim, four sectors, E sector at 38% shown orange and dashed, hit ripples on a 2 s scripted loop at the two impact points of the reference.
- **Accept**: the shield reads as a globe around the station in all four views (Design D-100); the solar array stays visibly outside it.

## M5 — Ships and effects
- Placeholder ships at their placements (free headings), engine trails, scripted bolt volleys, ranging laser ending at the dome, dry-dock frigate at a 74% reveal.
- **Accept**: no ship is grid-snapped; all ships share z = 16; hostile fire stops at the dome.

## M6 — HUD
- All panels from 04, driven by `assets/fixtures/siege_kr114.json`; minimap with the iso pulse; camera widget tracks yaw; H toggles the HUD.
- **Accept**: overlay comparison against `ops-grid-hud@2x.png` (scaled to 1080p) shows panels within ±4 px and matching text; the pulse period is 3.6 s with three rings visible; text is crisp at 1080p and 4K.

## Definition of done (phase 1)
All six milestones accepted; golden-image test green; `cargo clippy -D warnings` clean; README documents run commands and key bindings.
