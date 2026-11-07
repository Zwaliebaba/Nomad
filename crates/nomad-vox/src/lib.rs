//! MagicaVoxel import and export for Nomad's voxel art (Design D-289).
//!
//! The `.vox` files under `art/voxels/` are the source of truth for the art they hold; the JSON
//! files the game and the tools read are generated from them. Each directory there is a *set*
//! ([`set`]): a `set.json` manifest, one `.vox` file per model, and one generated output file.
//!
//! - [`vox`] reads and writes `.vox` files (coordinate convention, hidden objects, emissive
//!   colours).
//! - [`formats`] writes and reads the generated JSON: `voxel-rts/models@1` for module kinds
//!   (D-278) and `voxel-rts/ships@1` (phase-1 doc 03).
//! - [`set`] ties them together: import, the up-to-date check, and export for seeding.
//!
//! The `nomad-vox` binary is the command line; `art/README.md` is the artist's guide.

pub mod formats;
pub mod model;
pub mod set;
pub mod vox;

pub use model::{Colour, Pos, VoxelModel};
