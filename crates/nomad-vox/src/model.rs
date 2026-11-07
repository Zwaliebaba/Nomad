//! The neutral voxel model both sides convert through: integer positions with a colour each.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Largest absolute voxel coordinate accepted, as in the client's scene loader
/// (`nomad-client` `assets::voxel::MAX_ABS_COORD`): mesh vertices store coordinates as `i16`.
pub const MAX_ABS_COORD: i32 = 16_000;

/// A voxel colour: its RGB value and whether it glows (rendered at full albedo, phase-1 doc 01
/// §2). Ordered non-emissive first, then by RGB, so palettes come out in a stable order that
/// does not depend on which model uses a colour first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Colour {
    pub emissive: bool,
    pub rgb: [u8; 3],
}

impl Colour {
    /// Parses `#rrggbb`.
    pub fn from_hex(text: &str, emissive: bool) -> Option<Colour> {
        let digits = text.strip_prefix('#')?;
        if digits.len() != 6 || !digits.is_ascii() {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        Some(Colour {
            emissive,
            rgb: [channel(0)?, channel(2)?, channel(4)?],
        })
    }

    /// `#rrggbb`, lower case.
    pub fn hex(&self) -> String {
        let [r, g, b] = self.rgb;
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())?;
        if self.emissive {
            f.write_str(" (emissive)")?;
        }
        Ok(())
    }
}

/// A voxel position `[x, y, z]`: `+z` up, the voxel spans `[x, x+1] × [y, y+1] × [z, z+1]`
/// (phase-1 doc 03).
pub type Pos = [i32; 3];

/// A set of coloured voxels, kept sorted by position so every output is deterministic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoxelModel {
    voxels: BTreeMap<Pos, Colour>,
}

impl VoxelModel {
    pub fn new() -> VoxelModel {
        VoxelModel::default()
    }

    /// Adds a voxel. A second voxel at the same position is refused with that position: in a
    /// `.vox` file it means two objects overlap, and which one wins would be arbitrary.
    pub fn insert(&mut self, pos: Pos, colour: Colour) -> Result<(), Pos> {
        if self.voxels.contains_key(&pos) {
            return Err(pos);
        }
        self.voxels.insert(pos, colour);
        Ok(())
    }

    /// Voxels in position order (x, then y, then z).
    pub fn iter(&self) -> impl Iterator<Item = (Pos, Colour)> + '_ {
        self.voxels.iter().map(|(p, c)| (*p, *c))
    }

    pub fn len(&self) -> usize {
        self.voxels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.voxels.is_empty()
    }

    /// Inclusive bounds `(min, max)`, or `None` for an empty model.
    pub fn bounds(&self) -> Option<(Pos, Pos)> {
        let mut it = self.voxels.keys();
        let first = *it.next()?;
        Some(it.fold((first, first), |(mut lo, mut hi), p| {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
            (lo, hi)
        }))
    }

    /// The distinct colours used, in [`Colour`] order.
    pub fn colours(&self) -> BTreeSet<Colour> {
        self.voxels.values().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_junk() {
        let c = Colour::from_hex("#7fE6ff", true).unwrap();
        assert_eq!(c.rgb, [0x7f, 0xe6, 0xff]);
        assert_eq!(c.hex(), "#7fe6ff");
        for bad in ["7fe6ff", "#7fe6f", "#7fe6ffa", "#gggggg", "#7fé6f"] {
            assert!(Colour::from_hex(bad, false).is_none(), "{bad}");
        }
    }

    #[test]
    fn duplicate_positions_are_refused_and_bounds_are_inclusive() {
        let grey = Colour {
            emissive: false,
            rgb: [1, 2, 3],
        };
        let mut m = VoxelModel::new();
        assert!(m.bounds().is_none());
        m.insert([-3, 0, 2], grey).unwrap();
        m.insert([4, -1, 0], grey).unwrap();
        assert_eq!(m.insert([4, -1, 0], grey), Err([4, -1, 0]));
        assert_eq!(m.bounds(), Some(([-3, -1, 0], [4, 0, 2])));
    }
}
