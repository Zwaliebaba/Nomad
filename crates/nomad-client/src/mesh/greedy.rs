//! Greedy mesher (phase-1 doc 01 §2, doc 03, doc 05 `mesh/greedy.rs`).
//!
//! A face of a voxel is emitted only when the neighbouring cell in the same module is empty
//! ("cull faces between solid neighbours"). Coplanar exposed faces with the same palette index
//! are then merged into rectangles, one slice and one face direction at a time ("greedy-mesh per
//! module per palette entry"). Modules are meshed separately (doc 03), so faces where two modules
//! touch are kept on both sides; they face each other and are never visible.
//!
//! The station's palette carries per-voxel noise, so merging mostly joins runs of identical
//! entries; the shipped scene meshes to about 19k quads.

use crate::assets::voxel::{Chunk, world_of};

/// Face directions. The discriminant is the face id the voxel shader receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Face {
    PosX = 0,
    NegX = 1,
    PosY = 2,
    NegY = 3,
    PosZ = 4,
    NegZ = 5,
}

impl Face {
    pub const ALL: [Self; 6] = [
        Self::PosX,
        Self::NegX,
        Self::PosY,
        Self::NegY,
        Self::PosZ,
        Self::NegZ,
    ];

    /// 0, 1 or 2 for x, y or z.
    pub fn axis(self) -> usize {
        match self {
            Self::PosX | Self::NegX => 0,
            Self::PosY | Self::NegY => 1,
            Self::PosZ | Self::NegZ => 2,
        }
    }

    /// Whether the outward normal points along + of [`Self::axis`].
    pub fn is_positive(self) -> bool {
        matches!(self, Self::PosX | Self::PosY | Self::PosZ)
    }

    /// The id passed to the shader (see `shaders/voxels.wgsl`).
    pub fn id(self) -> u16 {
        u16::from(self as u8)
    }

    /// Outward unit normal. (The shader has its own table, indexed by [`Self::id`].)
    #[cfg(test)]
    pub fn normal(self) -> [i32; 3] {
        let mut n = [0; 3];
        n[self.axis()] = if self.is_positive() { 1 } else { -1 };
        n
    }
}

/// A rectangle of coplanar faces that share a direction and a palette entry, in world voxel
/// coordinates (integers; no z scale, doc 05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quad {
    pub face: Face,
    pub palette_index: u16,
    /// Corners counter-clockwise when seen from outside, that is, by the right-hand rule about
    /// the outward normal.
    pub corners: [[i32; 3]; 4],
}

/// Meshes one chunk.
pub fn greedy_mesh(chunk: &Chunk) -> Vec<Quad> {
    mesh_chunk(chunk, true)
}

/// One quad per exposed face, without merging. Every edge is shared vertex for vertex, so there
/// are no T-junctions: the crack-free reference the GPU tests compare the greedy mesh against.
#[cfg(test)]
pub fn unmerged_mesh(chunk: &Chunk) -> Vec<Quad> {
    mesh_chunk(chunk, false)
}

fn mesh_chunk(chunk: &Chunk, merge: bool) -> Vec<Quad> {
    let size = chunk.size();
    let mut quads = Vec::new();
    let mut mask: Vec<u16> = Vec::new();
    for face in Face::ALL {
        let a = face.axis();
        // (a, u, v) is a cyclic permutation of (x, y, z), so e_u × e_v = e_a.
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        let (nu, nv) = (size[u], size[v]);
        mask.clear();
        mask.resize(nu * nv, 0);
        for slice in 0..size[a] {
            // The cell value of every exposed face in this slice, 0 elsewhere.
            for j in 0..nv {
                for i in 0..nu {
                    let mut local = [0; 3];
                    local[a] = slice;
                    local[u] = i;
                    local[v] = j;
                    let cell = chunk.get(local);
                    let exposed = cell != 0 && chunk.neighbour(local, a, face.is_positive()) == 0;
                    mask[i + j * nu] = if exposed { cell } else { 0 };
                }
            }
            // Grow each unvisited face into the widest run along u, then as many full rows along
            // v as match, and clear what was taken.
            for j in 0..nv {
                let mut i = 0;
                while i < nu {
                    let cell = mask[i + j * nu];
                    if cell == 0 {
                        i += 1;
                        continue;
                    }
                    let mut width = 1;
                    while merge && i + width < nu && mask[i + width + j * nu] == cell {
                        width += 1;
                    }
                    let mut height = 1;
                    while merge
                        && j + height < nv
                        && mask[i + (j + height) * nu..i + width + (j + height) * nu]
                            .iter()
                            .all(|&c| c == cell)
                    {
                        height += 1;
                    }
                    for row in j..j + height {
                        mask[i + row * nu..i + width + row * nu].fill(0);
                    }
                    quads.push(quad(
                        chunk.origin(),
                        face,
                        Rect {
                            slice,
                            i,
                            j,
                            width,
                            height,
                        },
                        cell - 1,
                    ));
                    i += width;
                }
            }
        }
    }
    quads
}

/// A merged rectangle in local chunk coordinates: `slice` along the face axis, `i..i + width`
/// along u and `j..j + height` along v.
#[derive(Clone, Copy)]
struct Rect {
    slice: usize,
    i: usize,
    j: usize,
    width: usize,
    height: usize,
}

fn quad(origin: [i32; 3], face: Face, rect: Rect, palette_index: u16) -> Quad {
    let a = face.axis();
    let (u, v) = ((a + 1) % 3, (a + 2) % 3);
    // A +face lies on the far side of its voxels, a −face on the near side.
    let mut near = [0; 3];
    near[a] = rect.slice + usize::from(face.is_positive());
    near[u] = rect.i;
    near[v] = rect.j;
    let mut far = near;
    far[u] += rect.width;
    far[v] += rect.height;
    let (p0, p2) = (world_of(origin, near), world_of(origin, far));
    let mut p1 = p0;
    p1[u] = p2[u];
    let mut p3 = p0;
    p3[v] = p2[v];
    // p0 → p1 → p2 → p3 turns counter-clockwise about +e_a.
    let corners = if face.is_positive() {
        [p0, p1, p2, p3]
    } else {
        [p0, p3, p2, p1]
    };
    Quad {
        face,
        palette_index,
        corners,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::assets::voxel::{StationScene, load_station_scene};
    use crate::config::{phase1_fixture_root, station_scene_path};

    /// Unit faces of a quad as (face, voxel position) → palette index; panics on a face covered
    /// twice by the same quad, which cannot happen for a rectangle.
    fn unit_faces(quad: &Quad) -> Vec<((Face, [i32; 3]), u16)> {
        let a = quad.face.axis();
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        let lo = |axis: usize| quad.corners.iter().map(|c| c[axis]).min().unwrap();
        let hi = |axis: usize| quad.corners.iter().map(|c| c[axis]).max().unwrap();
        assert_eq!(lo(a), hi(a), "quad is not planar");
        let plane = lo(a);
        let mut faces = Vec::new();
        for pu in lo(u)..hi(u) {
            for pv in lo(v)..hi(v) {
                let mut voxel = [0; 3];
                voxel[a] = if quad.face.is_positive() {
                    plane - 1
                } else {
                    plane
                };
                voxel[u] = pu;
                voxel[v] = pv;
                faces.push(((quad.face, voxel), quad.palette_index));
            }
        }
        faces
    }

    /// The faces that must be emitted for a set of voxels: every face whose neighbour in the same
    /// set is empty.
    fn exposed_faces(voxels: &BTreeMap<[i32; 3], u16>) -> BTreeMap<(Face, [i32; 3]), u16> {
        let mut faces = BTreeMap::new();
        for (&position, &palette) in voxels {
            for face in Face::ALL {
                let n = face.normal();
                let neighbour = [position[0] + n[0], position[1] + n[1], position[2] + n[2]];
                if !voxels.contains_key(&neighbour) {
                    faces.insert((face, position), palette);
                }
            }
        }
        faces
    }

    fn cross(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn sub(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    /// Checks that the quads cover exactly the exposed faces, each once, with the right palette
    /// entry and outward winding.
    fn assert_watertight(name: &str, voxels: &BTreeMap<[i32; 3], u16>, quads: &[Quad]) {
        let mut covered = BTreeMap::new();
        for quad in quads {
            // Right-hand rule about the outward normal, and a proper rectangle.
            let normal = cross(
                sub(quad.corners[1], quad.corners[0]),
                sub(quad.corners[3], quad.corners[0]),
            );
            let outward = quad.face.normal();
            let along: i32 = (0..3).map(|i| normal[i] * outward[i]).sum();
            assert!(along > 0, "{name}: {quad:?} is wound inwards or degenerate");
            assert_eq!(
                sub(quad.corners[2], quad.corners[1]),
                sub(quad.corners[3], quad.corners[0]),
                "{name}: {quad:?} is not a parallelogram"
            );
            for (key, palette) in unit_faces(quad) {
                assert!(
                    covered.insert(key, palette).is_none(),
                    "{name}: face {key:?} emitted more than once"
                );
            }
        }
        let expected = exposed_faces(voxels);
        let missing: BTreeSet<_> = expected
            .keys()
            .filter(|k| !covered.contains_key(k))
            .collect();
        let extra: BTreeSet<_> = covered
            .keys()
            .filter(|k| !expected.contains_key(k))
            .collect();
        assert!(
            missing.is_empty(),
            "{name}: exposed faces not emitted: {missing:?}"
        );
        assert!(
            extra.is_empty(),
            "{name}: internal or phantom faces emitted: {extra:?}"
        );
        assert_eq!(covered, expected, "{name}: wrong palette entry on a face");
    }

    fn scene_from(text: &str) -> StationScene {
        StationScene::from_json(text, std::path::Path::new("test.json")).unwrap()
    }

    /// Phase-1 doc 05 "Testing": the greedy mesher produces watertight output for each module.
    /// The expected faces come straight from the JSON, independently of the loader's chunks.
    #[test]
    fn every_module_meshes_watertight() {
        let path = station_scene_path(&phase1_fixture_root());
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let scene = load_station_scene(&path).unwrap();
        let mut total_quads = 0;
        let mut total_faces = 0;
        for (module, raw_module) in scene.modules.iter().zip(raw["modules"].as_array().unwrap()) {
            assert_eq!(raw_module["id"], module.id.as_str());
            let voxels: BTreeMap<[i32; 3], u16> = raw_module["voxels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    let n = |i: usize| i32::try_from(v[i].as_i64().unwrap()).unwrap();
                    ([n(0), n(1), n(2)], u16::try_from(n(3)).unwrap())
                })
                .collect();
            let quads = greedy_mesh(&module.chunk);
            assert_watertight(&module.id, &voxels, &quads);
            // The unmerged reference mesh (GPU hole test) is watertight with one quad per face.
            let unmerged = unmerged_mesh(&module.chunk);
            assert_watertight(&module.id, &voxels, &unmerged);
            assert_eq!(unmerged.len(), exposed_faces(&voxels).len());
            total_quads += quads.len();
            total_faces += unmerged.len();
        }
        // Merging must actually happen; doc 05 expects about 20k faces after greedy meshing.
        assert!(
            total_quads < total_faces,
            "{total_quads} quads for {total_faces} faces"
        );
        assert!(total_quads < 25_000, "{total_quads} quads");
    }

    #[test]
    fn solid_box_becomes_six_quads() {
        let mut voxels = Vec::new();
        for x in 0..3 {
            for y in -2..0 {
                for z in 5..9 {
                    voxels.push([x, y, z, 0]);
                }
            }
        }
        let text = serde_json::json!({
            "format": "voxel-rts/scene@1",
            "shading": { "top": 1.0, "left": 0.77, "right": 0.48 },
            "palette": [{ "rgb": "#858c9a", "emissive": false }],
            "modules": [{ "id": "box", "bounds": { "min": [0, -2, 5], "max": [2, -1, 8] }, "voxels": voxels }]
        })
        .to_string();
        let scene = scene_from(&text);
        let quads = greedy_mesh(&scene.modules[0].chunk);
        assert_eq!(quads.len(), 6);
        let top = quads.iter().find(|q| q.face == Face::PosZ).unwrap();
        assert_eq!(top.corners, [[0, -2, 9], [3, -2, 9], [3, 0, 9], [0, 0, 9]]);
        let bottom = quads.iter().find(|q| q.face == Face::NegZ).unwrap();
        assert_eq!(
            bottom.corners,
            [[0, -2, 5], [0, 0, 5], [3, 0, 5], [3, -2, 5]]
        );
    }

    #[test]
    fn different_palette_entries_are_not_merged_and_hollows_are_closed() {
        // A 3×3×3 cube with a hollow centre and one differently coloured voxel on top.
        let mut voxels = Vec::new();
        for x in 0..3 {
            for y in 0..3 {
                for z in 0..3 {
                    if [x, y, z] != [1, 1, 1] {
                        let palette = i32::from([x, y, z] == [1, 1, 2]);
                        voxels.push([x, y, z, palette]);
                    }
                }
            }
        }
        let expected: BTreeMap<[i32; 3], u16> = voxels
            .iter()
            .map(|v| ([v[0], v[1], v[2]], u16::try_from(v[3]).unwrap()))
            .collect();
        let text = serde_json::json!({
            "format": "voxel-rts/scene@1",
            "shading": { "top": 1.0, "left": 0.77, "right": 0.48 },
            "palette": [{ "rgb": "#858c9a", "emissive": false }, { "rgb": "#ff6a55", "emissive": true }],
            "modules": [{ "id": "cube", "bounds": { "min": [0, 0, 0], "max": [2, 2, 2] }, "voxels": voxels }]
        })
        .to_string();
        let scene = scene_from(&text);
        let quads = greedy_mesh(&scene.modules[0].chunk);
        assert_watertight("cube", &expected, &quads);
        // The cavity's six inner faces are exposed too, so the hollow is closed.
        assert!(
            quads
                .iter()
                .any(|q| q.face == Face::NegZ && q.corners[0][2] == 2)
        );
    }
}
