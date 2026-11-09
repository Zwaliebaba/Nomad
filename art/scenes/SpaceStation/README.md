# SpaceStation

The supported default scene, assembled from all 24 PascalCase station module types (D-290).
It contains a command core, industrial and habitat branches, two hangars,
solar wings and a circular dock, joined by enclosed tubes. This is an art scene,
not a new gameplay slot layout; the art conforms to the simulation's slot template
(D-297, below). The older authoring layouts and their lowercase
module sources are retired. Historical phase-1 data remains for regression tests
and shared rendering templates only.

Four dedicated `ShieldGenerator` modules sit outside the main plant, at its
north, east, south and west perimeter. Each has an exposed cyan/white energy core,
magnetic coil rings and four swept projector yokes above an armoured capacitor
housing. A top-mounted projector lens has its highest emitting voxels at z=60;
each visible energy link starts at their exposed top faces, local
`[0.5, 0.5, 61]`, and ends on the field surface 32 voxels inward. Generator
housings are entirely outside the protected volume and remain exposed to attack
(D-255). Their external power-feed tubes also cross outside the field; all other
station modules and internal tubes remain inside. Energy links are rendering
effects, not solid voxel or collision geometry. The build checks the complete
generator voxel volumes are outside, the plant/internal tube corners are inside,
the links start at actual highest emitter faces, and their endpoints meet the field.
Native tests verify both endpoints in all four views. The preview uses the same
shield height; equatorial strokes and the below-grid opacity discontinuity are disabled.
This art-scene envelope does not change the simulation's shield rules.

**Conforming to the simulation (D-297).** The simulation is the source of truth for
station scale and layout, and this scene is to be re-authored or scaled to fit it;
D-297 supersedes D-290 where the scene conflicts. The β consistency check found
these conflicts, which belong to the art owner's rework:

- The shield is centred at `z = 61` with `radius_xy 320` and `radius_z 270`; D-100
  and D-153 make it a globe centred on the ship plane (`z = 16`, vertical radius
  26), and its equator is the dome's 36-voxel hit circle.
- The modules are placed far outside the slot template (the layout spans about
  ±364 voxels; the core alone is wider than the dome).
- The sector rings are hidden and `hits.impacts` is empty; D-297 restores the rings
  and hit ripples M4 shipped.
- `pylon_n` stands on the boundary of the N sector; D-046 centres each sector arc on
  its pylon, and D-228/D-255 put pylons and peripheral slots wholly outside the dome.
- Both solar arrays sit inside the field, and every module is at elevation tier 0;
  D-003 puts the solar array outside the field and structures on tiers 0/4/8/12.
- The fixture's world labels, the phase-1 pylon positions and structure elevations in
  `scene_config.json`, the empty ship placements and the fixture's pylon strengths
  do not match this scene (no weak E sector for M4's scenario).

Each module kind's model is to be named by a `kind` field in `set.json` (D-298;
see `art/voxels/modules/MODULES.md`).

- **`SpaceStation.vox`**: open directly in MagicaVoxel 0.99.6+. Named objects
  retain the shared palette, Glass glazing and Emit lights. Rotations are baked
  into voxel coordinates; scene-graph rotations remain identity for D-289.
- **`layout.json`**: the editable source layout. Module instances reference
  catalog IDs, positions and quarter turns; connections specify their two
  axis-aligned endpoint positions. Junctions reuse the tube module sources.
- **`assets/`**: the default renderer asset directory, containing the generated
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
use MagicaVoxel for the full Glass appearance. The repository's native renderer
loads this layout by default:

```bash
cargo run --locked -p nomad-client -- --size 1280x720
```

No station editor application is required. Rebuild the complete scene after any
module palette, material or geometry edit.
