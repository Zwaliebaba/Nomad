//! `assets/voxels/station_scene.json`, format `voxel-rts/scene@1` (phase-1 doc 03).
//!
//! Each module becomes a dense chunk over its bounds: one `u16` per cell, 0 for empty and palette
//! index + 1 otherwise (doc 03, "Recommended in-memory form"). Voxel coordinates are world
//! integers; the voxel at `(x, y, z)` spans `[x, x+1] × [y, y+1] × [z, z+1]`. Bounds are inclusive
//! voxel coordinates, as in the shipped file.
//!
//! The file is external input, so everything the mesher and renderer rely on is checked here and
//! a bad file is rejected with an error, never a panic (`CODING_STANDARDS.md` §5): palette indices,
//! bounds, duplicate voxels, overlapping modules (doc 03: "Modules do not overlap"; overlapping
//! faces would z-fight) and sizes.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::config::{ConfigError, Rgb, check_format};

/// The one format version this build reads.
pub const STATION_SCENE_FORMAT: &str = "voxel-rts/scene@1";
/// Largest absolute voxel coordinate accepted. Mesh vertices store coordinates as `i16`, and a
/// voxel's far corner is one more than its coordinate.
pub const MAX_ABS_COORD: i32 = 16_000;
/// Most dense-chunk cells over all modules (two bytes each). The shipped scene needs about 240k.
pub const MAX_TOTAL_CELLS: usize = 1 << 24;
/// Most modules accepted. Mesh vertices carry their module index in an `i16` lane (for the hover
/// highlight); the shipped scene has 17.
pub const MAX_MODULES: usize = 4096;
/// Highest elevation tier accepted (phase 1 uses 0, 4, 8 and 12; doc 03).
pub const MAX_ELEVATION_TIER: u32 = 1024;

/// The parsed and validated station scene.
pub struct StationScene {
    pub palette: Vec<PaletteEntry>,
    pub shading: FaceShading,
    pub modules: Vec<VoxelModule>,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct PaletteEntry {
    pub rgb: Rgb,
    /// Rendered at full albedo regardless of face (doc 01 §2); bloom follows in M3.
    pub emissive: bool,
}

/// Camera-relative face factors (doc 01 §2): top 1.00, left-facing 0.77, right-facing 0.48.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct FaceShading {
    pub top: f32,
    pub left: f32,
    pub right: f32,
}

/// One module (a selectable entity in phase 2): its id, elevation tier and dense voxel chunk.
pub struct VoxelModule {
    pub id: String,
    /// Height of a lifted module above the ops grid (doc 03). Its voxels are already stored at
    /// that height; a tier above 0 asks for elevation cues (doc 01 §7). 0 when the file omits it.
    pub elevation_tier: u32,
    pub chunk: Chunk,
}

impl StationScene {
    /// Number of voxels whose palette entry is emissive (doc 06 M3 budget: about 100).
    pub fn emissive_voxel_count(&self) -> usize {
        self.modules
            .iter()
            .flat_map(|m| m.chunk.voxels())
            .filter(|&(_, p)| {
                self.palette
                    .get(usize::from(p))
                    .is_some_and(|entry| entry.emissive)
            })
            .count()
    }
}

/// A dense box of voxels. Cell value 0 is empty; any other value is palette index + 1.
pub struct Chunk {
    /// World coordinates of the cell at local `(0, 0, 0)`.
    origin: [i32; 3],
    size: [usize; 3],
    cells: Vec<u16>,
}

impl Chunk {
    fn empty(origin: [i32; 3], size: [usize; 3], cell_count: usize) -> Self {
        Self {
            origin,
            size,
            cells: vec![0; cell_count],
        }
    }

    pub fn origin(&self) -> [i32; 3] {
        self.origin
    }

    /// Cells along x, y and z.
    pub fn size(&self) -> [usize; 3] {
        self.size
    }

    fn index(&self, [x, y, z]: [usize; 3]) -> usize {
        x + self.size[0] * (y + self.size[1] * z)
    }

    /// The cell at local coordinates, which must lie inside the chunk.
    pub fn get(&self, local: [usize; 3]) -> u16 {
        self.cells[self.index(local)]
    }

    /// The neighbour of a cell one step along `axis` (towards + when `positive`); empty outside
    /// the chunk.
    pub fn neighbour(&self, mut local: [usize; 3], axis: usize, positive: bool) -> u16 {
        let next = if positive {
            local[axis].checked_add(1)
        } else {
            local[axis].checked_sub(1)
        };
        match next {
            Some(n) if n < self.size[axis] => {
                local[axis] = n;
                self.get(local)
            }
            _ => 0,
        }
    }

    /// Local coordinates of a world voxel, if it lies inside the chunk.
    fn local(&self, world: [i32; 3]) -> Option<[usize; 3]> {
        let mut local = [0; 3];
        for axis in 0..3 {
            let offset = usize::try_from(world[axis] - self.origin[axis]).ok()?;
            if offset >= self.size[axis] {
                return None;
            }
            local[axis] = offset;
        }
        Some(local)
    }

    /// Every solid voxel as (world position, palette index), in cell order.
    pub fn voxels(&self) -> impl Iterator<Item = ([i32; 3], u16)> + '_ {
        let [sx, sy, _] = self.size;
        self.cells
            .iter()
            .enumerate()
            .filter(|&(_, &cell)| cell != 0)
            .map(move |(i, &cell)| {
                let local = [i % sx, (i / sx) % sy, i / (sx * sy)];
                (world_of(self.origin, local), cell - 1)
            })
    }
}

/// World coordinates of a local cell. Chunk sizes are validated to stay within
/// ±[`MAX_ABS_COORD`], so every local coordinate fits `i32`.
pub fn world_of(origin: [i32; 3], local: [usize; 3]) -> [i32; 3] {
    let offset = |v: usize| i32::try_from(v).expect("invariant: chunk sizes fit i32 (validated)");
    [
        origin[0] + offset(local[0]),
        origin[1] + offset(local[1]),
        origin[2] + offset(local[2]),
    ]
}

/// Reads and validates `station_scene.json`.
pub fn load_station_scene(path: &Path) -> Result<StationScene, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    StationScene::from_json(&text, path)
}

#[derive(Deserialize)]
struct SceneFile {
    palette: Vec<PaletteEntry>,
    shading: FaceShading,
    modules: Vec<ModuleFile>,
}

#[derive(Deserialize)]
struct ModuleFile {
    id: String,
    #[serde(default)]
    elevation_tier: u32,
    bounds: BoundsFile,
    /// Optional cross-check of `voxels.len()`.
    voxel_count: Option<usize>,
    /// `[x, y, z, palette index]`, world coordinates.
    voxels: Vec<(i32, i32, i32, u32)>,
}

#[derive(Deserialize)]
struct BoundsFile {
    min: [i32; 3],
    max: [i32; 3],
}

/// A rejected field and the reason.
type Rejection = (&'static str, String);

impl StationScene {
    /// Parses and validates the JSON text of a station scene. `path` is used in error messages.
    pub fn from_json(text: &str, path: &Path) -> Result<Self, ConfigError> {
        check_format(text, path, STATION_SCENE_FORMAT)?;
        let file: SceneFile = serde_json::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        Self::build(file).map_err(|(field, reason)| ConfigError::Invalid {
            path: path.to_path_buf(),
            field,
            reason,
        })
    }

    fn build(file: SceneFile) -> Result<Self, Rejection> {
        let SceneFile {
            palette,
            shading,
            modules: module_files,
        } = file;
        // Cells store palette index + 1 in a u16.
        if palette.is_empty() || palette.len() > usize::from(u16::MAX) {
            return Err((
                "palette",
                format!("needs 1 to {} entries, has {}", u16::MAX, palette.len()),
            ));
        }
        let factor_ok = |f: f32| (0.0..=1.0).contains(&f);
        if !(factor_ok(shading.top) && factor_ok(shading.left) && factor_ok(shading.right)) {
            return Err(("shading", "factors must be in [0, 1]".into()));
        }

        if module_files.len() > MAX_MODULES {
            return Err((
                "modules",
                format!(
                    "at most {MAX_MODULES} modules are supported, found {}",
                    module_files.len()
                ),
            ));
        }
        let mut modules: Vec<VoxelModule> = Vec::with_capacity(module_files.len());
        let mut ids = HashSet::new();
        // Which module owns each voxel, to reject overlaps between modules.
        let mut owner: HashMap<[i32; 3], usize> = HashMap::new();
        let mut total_cells = 0_usize;
        for module in module_files {
            if !ids.insert(module.id.clone()) {
                return Err(("modules[].id", format!("duplicate id {:?}", module.id)));
            }
            if module.elevation_tier > MAX_ELEVATION_TIER {
                return Err((
                    "modules[].elevation_tier",
                    format!(
                        "module {:?}: tier {} is above {MAX_ELEVATION_TIER}",
                        module.id, module.elevation_tier
                    ),
                ));
            }
            let (origin, size, cell_count) = chunk_shape(&module)?;
            total_cells = total_cells.saturating_add(cell_count);
            if total_cells > MAX_TOTAL_CELLS {
                return Err((
                    "modules[].bounds",
                    format!("the modules' bounds exceed {MAX_TOTAL_CELLS} cells in total"),
                ));
            }
            if let Some(count) = module.voxel_count
                && count != module.voxels.len()
            {
                return Err((
                    "modules[].voxel_count",
                    format!(
                        "module {:?} says {count} voxels but lists {}",
                        module.id,
                        module.voxels.len()
                    ),
                ));
            }

            let mut chunk = Chunk::empty(origin, size, cell_count);
            for &(x, y, z, palette_index) in &module.voxels {
                let at = format!("module {:?}, voxel [{x}, {y}, {z}]", module.id);
                // palette.len() <= u16::MAX, so a valid index + 1 fits u16.
                let cell = usize::try_from(palette_index)
                    .ok()
                    .filter(|&i| i < palette.len())
                    .and_then(|i| u16::try_from(i + 1).ok())
                    .ok_or_else(|| {
                        (
                            "modules[].voxels",
                            format!("{at}: palette index {palette_index} is out of range"),
                        )
                    })?;
                let local = chunk
                    .local([x, y, z])
                    .ok_or_else(|| ("modules[].voxels", format!("{at}: outside the bounds")))?;
                let index = chunk.index(local);
                if chunk.cells[index] != 0 {
                    return Err(("modules[].voxels", format!("{at}: listed twice")));
                }
                chunk.cells[index] = cell;
                if let Some(other) = owner.insert([x, y, z], modules.len()) {
                    return Err((
                        "modules[].voxels",
                        format!("{at}: overlaps module {:?}", modules[other].id),
                    ));
                }
            }
            modules.push(VoxelModule {
                id: module.id,
                elevation_tier: module.elevation_tier,
                chunk,
            });
        }
        Ok(Self {
            palette,
            shading,
            modules,
        })
    }
}

/// Origin, size and cell count of a module's chunk, from its validated bounds.
fn chunk_shape(module: &ModuleFile) -> Result<([i32; 3], [usize; 3], usize), Rejection> {
    let BoundsFile { min, max } = module.bounds;
    let in_range = |v: i32| (-MAX_ABS_COORD..=MAX_ABS_COORD).contains(&v);
    if !min.iter().chain(&max).all(|&v| in_range(v)) {
        return Err((
            "modules[].bounds",
            format!(
                "module {:?}: coordinates must be within ±{MAX_ABS_COORD}",
                module.id
            ),
        ));
    }
    let mut size = [0_usize; 3];
    for axis in 0..3 {
        // Inclusive bounds: at least one cell per axis.
        size[axis] = usize::try_from(max[axis] - min[axis] + 1)
            .ok()
            .filter(|&n| n > 0)
            .ok_or_else(|| {
                (
                    "modules[].bounds",
                    format!("module {:?}: min must not exceed max", module.id),
                )
            })?;
    }
    let cell_count = size[0]
        .checked_mul(size[1])
        .and_then(|n| n.checked_mul(size[2]))
        .filter(|&n| n <= MAX_TOTAL_CELLS)
        .ok_or_else(|| {
            (
                "modules[].bounds",
                format!(
                    "module {:?}: bounds exceed {MAX_TOTAL_CELLS} cells",
                    module.id
                ),
            )
        })?;
    Ok((min, size, cell_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_asset_root, station_scene_path};

    fn real_scene() -> StationScene {
        load_station_scene(&station_scene_path(&default_asset_root()))
            .expect("the shipped station_scene.json must load")
    }

    fn parse(text: &str) -> Result<StationScene, ConfigError> {
        StationScene::from_json(text, Path::new("test.json"))
    }

    /// A tiny scene with a two-entry palette and the given modules.
    fn scene_json(modules: serde_json::Value) -> String {
        let mut scene = serde_json::json!({
            "format": STATION_SCENE_FORMAT,
            "shading": { "top": 1.0, "left": 0.77, "right": 0.48 },
            "palette": [
                { "rgb": "#858c9a", "emissive": false },
                { "rgb": "#7ed9ee", "emissive": true }
            ]
        });
        scene["modules"] = modules;
        scene.to_string()
    }

    fn module(
        id: &str,
        min: [i32; 3],
        max: [i32; 3],
        voxels: serde_json::Value,
    ) -> serde_json::Value {
        let mut module = serde_json::json!({ "id": id, "bounds": { "min": min, "max": max } });
        module["voxels"] = voxels;
        module
    }

    #[test]
    fn real_scene_loads() {
        let scene = real_scene();
        assert_eq!(scene.palette.len(), 339);
        assert_eq!(scene.modules.len(), 17);
        assert!((scene.shading.top - 1.0).abs() < 1e-6);
        assert!((scene.shading.left - 0.77).abs() < 1e-6);
        assert!((scene.shading.right - 0.48).abs() < 1e-6);
        let voxels: usize = scene.modules.iter().map(|m| m.chunk.voxels().count()).sum();
        assert_eq!(voxels, 17_348);
        // Phase-1 doc 06, M3: the data has 98 emissive voxels.
        assert_eq!(scene.emissive_voxel_count(), 98);
        // Doc 01 §5: FA-1 is lifted to tier +8 and FB-2 to +4; nothing else is lifted.
        let tiers: Vec<(&str, u32)> = scene
            .modules
            .iter()
            .filter(|m| m.elevation_tier > 0)
            .map(|m| (m.id.as_str(), m.elevation_tier))
            .collect();
        assert_eq!(tiers, [("factory_fa1", 8), ("factory_fb2", 4)]);
        let core = scene
            .modules
            .iter()
            .find(|m| m.id == "core_spindle")
            .expect("core_spindle exists");
        assert_eq!(core.chunk.origin(), [-11, -11, 1]);
        assert_eq!(core.chunk.size(), [23, 23, 33]);
    }

    /// Doc 06 M3 acceptance: "the station has no more than about 100 emissive voxels" (doc 00:
    /// earlier versions with light rows along the edges were unreadable). The emissive count is
    /// counted per voxel over every module, so this also guards future data exports.
    #[test]
    fn station_stays_within_the_emissive_budget() {
        const EMISSIVE_VOXEL_BUDGET: usize = 100;
        let emissive = real_scene().emissive_voxel_count();
        assert!(
            emissive <= EMISSIVE_VOXEL_BUDGET,
            "{emissive} emissive voxels exceed the M3 budget of about {EMISSIVE_VOXEL_BUDGET}"
        );
    }

    #[test]
    fn elevation_tier_is_optional_and_bounded() {
        let mut lifted = module("a", [0, 0, 4], [0, 0, 4], serde_json::json!([[0, 0, 4, 0]]));
        lifted["elevation_tier"] = 4.into();
        let plain = module("b", [5, 0, 0], [5, 0, 0], serde_json::json!([[5, 0, 0, 1]]));
        let scene = parse(&scene_json(serde_json::json!([lifted, plain]))).unwrap();
        assert_eq!(scene.modules[0].elevation_tier, 4);
        assert_eq!(scene.modules[1].elevation_tier, 0);
        assert_eq!(scene.emissive_voxel_count(), 1);

        let mut absurd = module("a", [0, 0, 0], [0, 0, 0], serde_json::json!([]));
        absurd["elevation_tier"] = (MAX_ELEVATION_TIER + 1).into();
        assert!(matches!(
            parse(&scene_json(serde_json::json!([absurd]))),
            Err(ConfigError::Invalid { .. })
        ));
        let mut negative = module("a", [0, 0, 0], [0, 0, 0], serde_json::json!([]));
        negative["elevation_tier"] = (-4).into();
        assert!(matches!(
            parse(&scene_json(serde_json::json!([negative]))),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn small_scene_round_trips() {
        let text = scene_json(serde_json::json!([module(
            "a",
            [-1, 0, 0],
            [0, 1, 2],
            serde_json::json!([[-1, 0, 0, 0], [0, 1, 2, 1]])
        )]));
        let scene = parse(&text).unwrap();
        let chunk = &scene.modules[0].chunk;
        assert_eq!(chunk.size(), [2, 2, 3]);
        assert_eq!(chunk.get([0, 0, 0]), 1);
        assert_eq!(chunk.get([1, 1, 2]), 2);
        assert_eq!(chunk.get([1, 0, 0]), 0);
        assert_eq!(chunk.neighbour([0, 0, 0], 0, false), 0, "outside is empty");
        let voxels: Vec<_> = chunk.voxels().collect();
        assert_eq!(voxels, vec![([-1, 0, 0], 0), ([0, 1, 2], 1)]);
    }

    #[test]
    fn unknown_format_is_rejected() {
        let text = scene_json(serde_json::json!([])).replace("scene@1", "scene@2");
        assert!(matches!(
            parse(&text),
            Err(ConfigError::UnsupportedFormat { .. })
        ));
    }

    #[test]
    fn bad_voxel_data_is_rejected() {
        let cases = [
            (
                "palette index out of range",
                serde_json::json!([module(
                    "a",
                    [0, 0, 0],
                    [1, 1, 1],
                    serde_json::json!([[0, 0, 0, 2]])
                )]),
            ),
            (
                "voxel outside the bounds",
                serde_json::json!([module(
                    "a",
                    [0, 0, 0],
                    [1, 1, 1],
                    serde_json::json!([[2, 0, 0, 0]])
                )]),
            ),
            (
                "duplicate voxel",
                serde_json::json!([module(
                    "a",
                    [0, 0, 0],
                    [1, 1, 1],
                    serde_json::json!([[0, 0, 0, 0], [0, 0, 0, 1]])
                )]),
            ),
            (
                "overlapping modules",
                serde_json::json!([
                    module("a", [0, 0, 0], [1, 1, 1], serde_json::json!([[1, 1, 1, 0]])),
                    module("b", [1, 1, 1], [2, 2, 2], serde_json::json!([[1, 1, 1, 0]]))
                ]),
            ),
            (
                "duplicate module id",
                serde_json::json!([
                    module("a", [0, 0, 0], [0, 0, 0], serde_json::json!([])),
                    module("a", [5, 0, 0], [5, 0, 0], serde_json::json!([]))
                ]),
            ),
            (
                "inverted bounds",
                serde_json::json!([module("a", [0, 0, 1], [0, 0, 0], serde_json::json!([]))]),
            ),
            (
                "coordinates beyond i16",
                serde_json::json!([module(
                    "a",
                    [0, 0, 0],
                    [20_000, 0, 0],
                    serde_json::json!([])
                )]),
            ),
            (
                "huge chunk",
                serde_json::json!([module(
                    "a",
                    [-16_000, -16_000, 0],
                    [16_000, 16_000, 100],
                    serde_json::json!([])
                )]),
            ),
        ];
        for (name, modules) in cases {
            assert!(
                matches!(
                    parse(&scene_json(modules)),
                    Err(ConfigError::Invalid { .. })
                ),
                "{name} should be rejected"
            );
        }

        let mut miscounted = module("a", [0, 0, 0], [1, 1, 1], serde_json::json!([[0, 0, 0, 0]]));
        miscounted["voxel_count"] = 2.into();
        assert!(matches!(
            parse(&scene_json(serde_json::json!([miscounted]))),
            Err(ConfigError::Invalid { .. })
        ));
        // A negative palette index is not even a valid voxel record.
        let negative = module(
            "a",
            [0, 0, 0],
            [1, 1, 1],
            serde_json::json!([[0, 0, 0, -1]]),
        );
        assert!(matches!(
            parse(&scene_json(serde_json::json!([negative]))),
            Err(ConfigError::Parse { .. })
        ));
    }
}
