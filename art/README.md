# Voxel art sources

The models here are maintained in [MagicaVoxel](https://ephtracy.github.io/) (0.99.6 or later). The `.vox` files are the source of truth (D-289). The JSON files the game and the tools read are generated from them by `nomad-vox`, and are never edited by hand.

| Set | `.vox` files | Generated file | Read by |
|---|---|---|---|
| `voxels/modules` | the 24 active PascalCase module sources | `voxels/modules/ModuleCatalog.json` (`voxel-rts/models@1`) | authoring catalog; `tools/station-scene` assembles the source models with full materials |
| `voxels/ships` | one per ship model | `Design/phase1-visual-slice/assets/voxels/ships_placeholder.json` (`voxel-rts/ships@1`) | `tools/preview`, then the client |

**Gameplay mapping (D-298).** Each `ModuleKind` names its one model with a `kind` field in `voxels/modules/set.json`; `nomad-vox` generates `module_kinds.json` from it (D-289's file), and a test fails unless every kind has exactly one model. That field, the generated file and the test are still to be added; until then no module model maps to a kind. **Scale (D-297).** The simulation's slot template, dome radius (36 voxels), hit radii and pylon positions are the source of truth; the module models and the SpaceStation scene are re-authored or scaled to fit them, not the reverse. See [modules/MODULES.md](voxels/modules/MODULES.md).

## Workflow

1. Open a `.vox` file in MagicaVoxel, edit it, and save it in place.
2. From the repository root, run:

   ```bash
   cargo run -p nomad-vox -- import
   ```

3. Commit the `.vox` file together with the regenerated JSON.

`cargo test` (and so CI) fails if `ModuleCatalog.json` or `ships_placeholder.json` does not match its `.vox` files. `cargo run -p nomad-vox -- import --check` reports the same thing without writing anything. The SpaceStation assembly and its assets are checked only by `python3 tools/station-scene/build.py --check` and the material audit `python3 tools/vox/audit_modules.py`, which CI does not run; run both after editing.

To add a model, add `{ "id": "...", "name": "..." }` to the set's `set.json`, save the new model as `<id>.vox` in the same folder, and run the import. An optional `file` property selects a different source filename, for example `{ "id": "command_core", "name": "Command core", "file": "CommandCore.vox" }`. IDs remain lowercase; artist-facing filenames can use PascalCase. Filenames must be unique (including on case-insensitive filesystems) and cannot contain directories.

To get back a `.vox` file you lost, regenerate it from the committed JSON with `cargo run -p nomad-vox -- export --force art/voxels/<set>`. This overwrites the `.vox` files of that set. **Catalog export retains RGB and Emit only; it loses Glass and the shared editing palette.** Prefer restoring artist-maintained sources from Git.

The PascalCase station modules are documented in [modules/MODULES.md](voxels/modules/MODULES.md). The assembled [SpaceStation scene](scenes/SpaceStation/README.md) includes a MagicaVoxel scene, editable layout and the default renderer asset directory. The old `start`/`full` layouts, their generators and ten lowercase module sources are retired (D-290). Historical phase-1 data is retained for regression tests and shared configuration only.

## Conventions

**Axes and origin.** MagicaVoxel and the game use the same axes: `+z` is up, and one MagicaVoxel voxel is one game voxel (about 35 m, doc 10 §8). The model's origin is MagicaVoxel's world origin, and the ground plane MagicaVoxel draws is `z = 0`. Where you put an object in World mode is where it ends up in the game.

- **Module kinds:** preserve each source's local origin and orientation; `tools/station-scene` places instances with explicit translations and quarter turns from `layout.json`. Generator contacts and forcefield alignment are validated against the actual source geometry. See `voxels/modules/MODULES.md`.
- **Ships:** `+x` is the nose, `+y` is port, and the origin is roughly the centre of mass. Ships rotate freely in the game (phase-1 doc 03).

**Colours.** Each voxel's colour is its exact RGB value. A file can use at most 255 colours, which is MagicaVoxel's palette size. An export puts the model's colours in the first palette slots and leaves the rest as MagicaVoxel's default palette for painting. You can use, add or change colours freely: the import collects the colours that are actually used. Alpha is ignored.

**Glowing parts.** A colour whose material is *Emit* is emissive in the game (lights, engines, emitters; phase-1 doc 01 §2). Every other material type renders as a plain colour. To make a colour glow, select it in the palette and set its material to Emit.

**Ship colour names.** `voxel-rts/ships@1` names its colours (`hull`, `eng`, ...). The names come from `colour_names` in `voxels/ships/set.json`, keyed by RGB. A new colour without a name there becomes `c_rrggbb`. If one RGB value is used both emissive and non-emissive, the emissive one gets `_emit` appended.

**Objects and layers.** A file can hold several objects. The import merges every visible object into one model. Objects must not overlap: two voxels in the same place is an error. Hidden objects and hidden layers are left out, so you can keep reference geometry in a file (a ship for scale, a slot outline) without it reaching the game. One object holds at most 256 voxels per axis; split a larger model into several objects.

**No rotation in World mode.** The import refuses an object rotated or mirrored with the World-mode transform, because the half-voxel pivot would shift it by one voxel. Rotate the voxels in Edit mode instead, or reset the object's rotation. Moving objects is fine. Animation frames other than the first are ignored.
