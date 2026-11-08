# SpaceStation voxel preview

A single-file WebGL viewer for the supported SpaceStation layout. It renders
`station_scene.json` and uses the selected asset directory's projection, shield
centre, opacity and ring settings. Energy links run from the external generator
lens top faces to the field surface. Glass is still displayed as an opaque colour
in this viewer.

It is a design-review aid, not the game client. The client is the `wgpu` renderer in `crates/nomad-client` (D-004). Not included: bloom (emissive voxels get a cheap glow instead), elevation cues, effects, labels and the HUD.

Build it from the current assets (the output goes into the git-ignored `target/`):

```bash
python3 tools/station-scene/build.py
python3 tools/preview/build.py art/scenes/SpaceStation/assets tools/preview/template.html target/SpaceStation-preview.html
```

Open `target/SpaceStation-preview.html` in a browser; `template.html` is only the
source template and cannot run without embedded data. Controls: Q/E rotate
between four views, mouse wheel or +/− zoom, drag or WASD pan, Home resets.
The buttons do the same on a phone.

If your browser blocks local file URLs, serve the generated preview locally:

```bash
python3 -m http.server 8765 --bind 127.0.0.1 --directory target
```

Open `SpaceStation-preview.html` through that local server rather than opening
the source template. The preview itself does not need Internet access.

The preview works offline with local font fallbacks. Packed station coordinates
use signed 16-bit integers so larger assemblies do not wrap at ±128 voxels.
The projection scale follows the selected asset directory's scene configuration.
For the new connected assembly, see [SpaceStation](../../art/scenes/SpaceStation/README.md).
