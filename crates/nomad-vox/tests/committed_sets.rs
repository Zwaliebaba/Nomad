//! The committed JSON matches the committed `.vox` sources (D-289).
//!
//! The `.vox` files under `art/voxels/` are the source of truth. This test fails when one was
//! changed without running the import, or when a generated file was edited by hand. Fix it
//! with `cargo run -p nomad-vox -- import` and commit the result.

use std::path::Path;

use nomad_vox::set::{self, Set, SetKind};

#[test]
fn generated_files_match_their_vox_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../art/voxels");
    let dirs = set::discover(&root).expect("art/voxels must be readable");
    let kinds: Vec<SetKind> = dirs
        .iter()
        .map(|dir| {
            let set = Set::load(dir).unwrap_or_else(|e| panic!("{e}"));
            assert!(
                set.is_current().unwrap_or_else(|e| panic!("{e}")),
                "{} is out of date with art/voxels/{}: run `cargo run -p nomad-vox -- import`",
                set.output_path().display(),
                set.name()
            );
            set.kind()
        })
        .collect();
    // Both sets exist, so the test cannot pass vacuously.
    assert!(kinds.contains(&SetKind::Models) && kinds.contains(&SetKind::Ships));
}
