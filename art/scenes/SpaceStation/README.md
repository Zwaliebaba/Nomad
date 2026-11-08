# SpaceStation

A free-floating scene assembled from all 24 PascalCase station module types.
It contains a command core, industrial and habitat branches, two hangars,
solar wings and a circular dock, joined by enclosed tubes. This is an art scene,
not a new gameplay slot layout. The original phase-1 scene is unchanged.

Four dedicated `ShieldGenerator` modules sit outside the main plant, at its
north, east, south and west perimeter. Each has an exposed cyan/white energy core,
magnetic coil rings and four swept projector yokes above an armoured capacitor
housing. The field passes directly through the four emitter cores at z=46;
it spans between the generators rather than beyond them. Their outer machinery
straddles the boundary. The plant and all tubes lie inside the protective
ellipsoid. The build checks actual emissive attachment voxels, exact emitter/field
alignment and coverage of the outer corners of every non-projector voxel.
This art-scene envelope does not change the simulation's shield rules.

- **`SpaceStation.vox`**: open directly in MagicaVoxel 0.99.6+. Named objects
  retain the shared palette, Glass glazing and Emit lights. Rotations are baked
  into voxel coordinates; scene-graph rotations remain identity for D-289.
- **`layout.json`**: the editable source layout. Module instances reference
  catalog IDs, positions and quarter turns; connections specify their two
  axis-aligned endpoint positions. Junctions reuse the tube module sources.
- **`assets/`**: an alternate asset directory containing the generated
  `voxels/station_scene.json`, matching camera configuration and supporting data.
  The game-format importer supports RGB and Emit; Glass remains opaque there.
- **`BuildReport.json`**: source coverage, placements, tube runs, material counts,
  removed port-bore voxels, physical connectivity, emitter alignment and field coverage.

The individual module `.vox` files remain the art sources. Edit them in
`art/voxels/modules/`, or edit placements in `layout.json`, then rebuild from the
repository root:

```bash
cargo run --locked -p nomad-vox -- import
python3 tools/vox/audit_modules.py
python3 tools/station-scene/build.py
python3 tools/station-scene/build.py --check
python3 -m unittest discover -s tools/station-scene -p 'test_*.py'
```

The generator preserves module detail and materials. It opens circular bores
at attachment points, adapts straight tube lengths using the source's wall
sections and end collars, and assigns shared coupling voxels to one object to
avoid overlap. It rejects unrelated collisions, unsupported routes, missing
module types, disconnected layout graphs and physically disconnected geometry.
The turret is mounted on its service platform rather than connected by a tube.
Do not edit the generated scene, `.vox` assembly or asset copies directly.

To build a standalone browser preview:

```bash
python3 tools/preview/build.py art/scenes/SpaceStation/assets tools/preview/template.html target/SpaceStation-preview.html
```

Open `target/SpaceStation-preview.html` locally. This viewer displays RGB/Emit;
use MagicaVoxel for the full Glass appearance. For the repository's native
renderer, select this asset directory through `NOMAD_ASSETS`:

```bash
NOMAD_ASSETS=art/scenes/SpaceStation/assets cargo run --locked -p nomad-client -- --size 1280x720
```

No station editor application is required. Rebuild the complete scene after any
module palette, material or geometry edit.
