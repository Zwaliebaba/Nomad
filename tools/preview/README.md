# KR-114 voxel preview

A throwaway, single-file WebGL viewer for the phase-1 scene. It renders `station_scene.json`, the placeholder ships, the infinite ops grid and the sectored shield. It uses the projection, face shading and camera of phase-1 docs 01 and 02, so the look can be judged in any browser, including a phone, without a GPU build of the game.

It is a design-review aid, not the game client. The client is the `wgpu` renderer in `crates/nomad-client` (D-004). Not included: bloom (emissive voxels get a cheap glow instead), elevation cues, effects, labels and the HUD.

Build it from the current assets (the output goes into the git-ignored `target/`):

```bash
python3 tools/preview/build.py Design/phase1-visual-slice/assets tools/preview/template.html target/kr114-preview.html
```

Open the generated file in a browser. Controls: Q/E rotate between the four views, mouse wheel or +/− to zoom, drag or WASD to pan, Home to reset. The buttons do the same on a phone.
