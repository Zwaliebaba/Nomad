//! Reading and writing MagicaVoxel `.vox` files (format 200, with the scene-graph extension).
//!
//! # Coordinates
//!
//! MagicaVoxel and the game share axes: right-handed, `+z` up, one `.vox` voxel per game voxel.
//! A file's world origin is the game model's origin, and the ground plane MagicaVoxel draws is
//! `z = 0`. MagicaVoxel stores each object as voxels at `0..size` plus a translation `_t` to
//! the object's centre, and places voxel `v` at `t + v − size / 2` (integer division per axis).
//! Writing inverts that, so a model keeps its coordinates through any number of round trips,
//! even when an object is moved or resized in the editor.
//!
//! # What is read
//!
//! Every visible object in the scene graph, merged into one model: translations of nested
//! groups add up; objects and layers marked hidden are skipped, so a file can carry reference
//! geometry (a ship for scale, the slot outline) that never reaches the game. Rotated or
//! mirrored objects are refused, not guessed at: MagicaVoxel's half-voxel pivot under rotation
//! is easy to get wrong by one voxel, and a silent one-voxel shift is worse than an error.
//! Overlapping objects are refused too. Animation frames other than the first are ignored.
//!
//! # Colours
//!
//! A voxel's colour is its palette entry; alpha is ignored. A palette entry whose material is
//! *Emit* (`_type` = `_emit`) is emissive (phase-1 doc 01 §2), every other material is not.
//! The material id is the colour's 1-based file index, as MagicaVoxel writes it.
//!
//! The file is external input: a malformed one is an error, never a panic (`CODING_STANDARDS.md`
//! §5). In particular the `_r` rotation is checked here as text, because `dot_vox`'s own
//! decoder asserts on an invalid byte.

use std::io;

use dot_vox::{Color, DotVoxData, Frame, Layer, Material, SceneNode, ShapeModel, Size};

use crate::model::{Colour, MAX_ABS_COORD, Pos, VoxelModel};

/// One `.vox` object holds at most this many voxels per axis.
pub const MAX_OBJECT_SIZE: i32 = 256;
/// A `.vox` palette has 256 slots, and voxels can use indices 1 to 255.
pub const MAX_COLOURS: usize = 255;
/// The file version MagicaVoxel 0.99.6 and later write.
const VOX_VERSION: u32 = 200;
/// The `_r` value of an unrotated, unmirrored object (`dot_vox::Rotation::IDENTITY`).
const IDENTITY_ROTATION: u8 = 0b0000_0100;
/// Deepest scene graph accepted; real files are a few levels deep. Also stops reference cycles.
const MAX_DEPTH: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum VoxError {
    #[error("not a readable MagicaVoxel file ({0})")]
    Parse(&'static str),
    #[error("the scene graph is malformed: {0}")]
    SceneGraph(String),
    #[error(
        "object `{object}` is rotated or mirrored; rotate its voxels in Edit mode instead, \
         or reset its rotation in World mode"
    )]
    Rotated { object: String },
    #[error("two voxels at {0:?}: objects overlap")]
    Overlap(Pos),
    #[error("a voxel lies outside ±{MAX_ABS_COORD} voxels")]
    OutOfRange,
    #[error("no visible voxels")]
    Empty,
    #[error("{0} colours, but a MagicaVoxel palette holds at most {MAX_COLOURS}")]
    TooManyColours(usize),
    #[error(
        "the model spans {0:?} voxels, but one MagicaVoxel object holds at most {MAX_OBJECT_SIZE} per axis"
    )]
    TooLarge([i32; 3]),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Reads a `.vox` file into one model (see the module docs for what is read).
pub fn read(bytes: &[u8]) -> Result<VoxelModel, VoxError> {
    let data = dot_vox::load_bytes(bytes).map_err(VoxError::Parse)?;
    let mut emissive = [false; 256];
    for material in &data.materials {
        if material.material_type() == Some("_emit")
            && let Ok(id) = usize::try_from(material.id)
            && let Some(slot) = emissive.get_mut(id)
        {
            *slot = true;
        }
    }
    let mut reader = Reader {
        data: &data,
        emissive,
        model: VoxelModel::new(),
    };
    if data.scenes.is_empty() {
        // Files older than the scene graph (version 150): every model at the origin.
        for index in 0..data.models.len() {
            reader.place(index, [0, 0, 0])?;
        }
    } else {
        reader.walk(0, [0, 0, 0], 0, "root")?;
    }
    if reader.model.is_empty() {
        return Err(VoxError::Empty);
    }
    Ok(reader.model)
}

struct Reader<'a> {
    data: &'a DotVoxData,
    /// Indexed by 1-based file colour index.
    emissive: [bool; 256],
    model: VoxelModel,
}

impl Reader<'_> {
    fn walk(&mut self, node: u32, offset: Pos, depth: usize, object: &str) -> Result<(), VoxError> {
        if depth > MAX_DEPTH {
            return Err(VoxError::SceneGraph(format!(
                "deeper than {MAX_DEPTH} levels (a cycle?)"
            )));
        }
        let entry = usize::try_from(node)
            .ok()
            .and_then(|i| self.data.scenes.get(i))
            .ok_or_else(|| VoxError::SceneGraph(format!("node {node} does not exist")))?;
        match entry {
            SceneNode::Transform {
                attributes,
                frames,
                child,
                layer_id,
            } => {
                if attributes.get("_hidden").is_some_and(|v| v == "1")
                    || self.layer_hidden(*layer_id)
                {
                    return Ok(());
                }
                let name = attributes.get("_name").map_or(object, String::as_str);
                let mut offset = offset;
                if let Some(frame) = frames.first() {
                    if let Some(r) = frame.attributes.get("_r")
                        && r.trim().parse::<u8>() != Ok(IDENTITY_ROTATION)
                    {
                        return Err(VoxError::Rotated {
                            object: name.to_owned(),
                        });
                    }
                    if let Some(t) = frame.attributes.get("_t") {
                        offset = add(offset, parse_translation(t, name)?)?;
                    }
                }
                self.walk(*child, offset, depth + 1, name)
            }
            SceneNode::Group { children, .. } => {
                for child in children {
                    self.walk(*child, offset, depth + 1, object)?;
                }
                Ok(())
            }
            SceneNode::Shape { models, .. } => match models.first() {
                // The first entry is frame 0; later entries are animation frames.
                Some(shape) => {
                    let index = usize::try_from(shape.model_id).map_err(|_| {
                        VoxError::SceneGraph(format!("model {} does not exist", shape.model_id))
                    })?;
                    self.place(index, offset)
                }
                None => Ok(()),
            },
        }
    }

    fn layer_hidden(&self, layer_id: u32) -> bool {
        usize::try_from(layer_id)
            .ok()
            .and_then(|i| self.data.layers.get(i))
            .is_some_and(Layer::hidden)
    }

    /// Adds model `index` with its centre at `centre` (`t + v − size / 2`).
    fn place(&mut self, index: usize, centre: Pos) -> Result<(), VoxError> {
        let model = self
            .data
            .models
            .get(index)
            .ok_or_else(|| VoxError::SceneGraph(format!("model {index} does not exist")))?;
        let half = |s: u32| i32::try_from(s / 2).map_err(|_| VoxError::OutOfRange);
        let half = [
            half(model.size.x)?,
            half(model.size.y)?,
            half(model.size.z)?,
        ];
        for v in &model.voxels {
            let local = [
                i32::from(v.x) - half[0],
                i32::from(v.y) - half[1],
                i32::from(v.z) - half[2],
            ];
            let pos = add(centre, local)?;
            let colour = self.colour(v.i)?;
            self.model.insert(pos, colour).map_err(VoxError::Overlap)?;
        }
        Ok(())
    }

    /// The colour of a voxel's 0-based palette index (`dot_vox` subtracts one from the file's).
    fn colour(&self, index: u8) -> Result<Colour, VoxError> {
        let c = self
            .data
            .palette
            .get(usize::from(index))
            .ok_or(VoxError::Parse(
                "a voxel's colour is missing from the palette",
            ))?;
        Ok(Colour {
            emissive: self.emissive[usize::from(index) + 1],
            rgb: [c.r, c.g, c.b],
        })
    }
}

fn parse_translation(text: &str, object: &str) -> Result<Pos, VoxError> {
    let bad = || {
        VoxError::SceneGraph(format!(
            "object `{object}` has a malformed position `{text}`"
        ))
    };
    let mut parts = text.split_whitespace().map(str::parse::<i32>);
    let mut next = || parts.next().and_then(Result::ok).ok_or_else(bad);
    let pos = [next()?, next()?, next()?];
    if parts.next().is_some() {
        return Err(bad());
    }
    Ok(pos)
}

fn add(a: Pos, b: Pos) -> Result<Pos, VoxError> {
    let mut out = [0; 3];
    for i in 0..3 {
        out[i] = a[i]
            .checked_add(b[i])
            .filter(|c| c.abs() <= MAX_ABS_COORD)
            .ok_or(VoxError::OutOfRange)?;
    }
    Ok(out)
}

/// Writes a model as a `.vox` file holding one object named `name`, placed so that reading the
/// file gives the model back unchanged. The model's colours take the first palette slots in
/// [`Colour`] order, emissive ones with an *Emit* material; the remaining slots keep
/// MagicaVoxel's default palette, so there are colours to paint with.
pub fn write(model: &VoxelModel, name: &str) -> Result<Vec<u8>, VoxError> {
    let (min, max) = model.bounds().ok_or(VoxError::Empty)?;
    let span = [0, 1, 2].map(|a| max[a] - min[a] + 1);
    if span.iter().any(|&s| s > MAX_OBJECT_SIZE) {
        return Err(VoxError::TooLarge(span));
    }
    let colours: Vec<Colour> = model.colours().into_iter().collect();
    if colours.len() > MAX_COLOURS {
        return Err(VoxError::TooManyColours(colours.len()));
    }

    let mut palette: Vec<Color> = dot_vox::DEFAULT_PALETTE.to_vec();
    let mut materials = Vec::new();
    for (slot, colour) in colours.iter().enumerate() {
        let [r, g, b] = colour.rgb;
        palette[slot] = Color { r, g, b, a: 255 };
        if colour.emissive {
            materials.push(emit_material(slot));
        }
    }

    let byte = |v: i32| u8::try_from(v).map_err(|_| VoxError::TooLarge(span));
    let mut voxels = Vec::with_capacity(model.len());
    for (pos, colour) in model.iter() {
        let slot = colours
            .binary_search(&colour)
            .expect("invariant: every colour of the model is in its sorted colour list");
        voxels.push(dot_vox::Voxel {
            x: byte(pos[0] - min[0])?,
            y: byte(pos[1] - min[1])?,
            z: byte(pos[2] - min[2])?,
            i: byte(i32::try_from(slot).map_err(|_| VoxError::TooManyColours(colours.len()))?)?,
        });
    }

    let dim = |s: i32| u32::try_from(s).map_err(|_| VoxError::TooLarge(span));
    let size = Size {
        x: dim(span[0])?,
        y: dim(span[1])?,
        z: dim(span[2])?,
    };
    let centre = [0, 1, 2].map(|a| min[a] + span[a] / 2);
    let data = DotVoxData {
        version: VOX_VERSION,
        index_map: dot_vox::DEFAULT_INDEX_MAP.to_vec(),
        models: vec![dot_vox::Model { size, voxels }],
        palette,
        materials,
        scenes: scene_graph(name, centre),
        layers: vec![
            Layer {
                attributes: dot_vox::Dict::default(),
            };
            8
        ],
    };
    let mut bytes = Vec::new();
    data.write_vox(&mut bytes)?;
    Ok(bytes)
}

/// Root transform → group → the object's transform (name, position) → its shape, as
/// MagicaVoxel writes a file with one object.
fn scene_graph(name: &str, centre: Pos) -> Vec<SceneNode> {
    let dict = |pairs: &[(&str, String)]| -> dot_vox::Dict {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    };
    vec![
        SceneNode::Transform {
            attributes: dot_vox::Dict::default(),
            frames: vec![Frame::new(dot_vox::Dict::default())],
            child: 1,
            layer_id: u32::MAX,
        },
        SceneNode::Group {
            attributes: dot_vox::Dict::default(),
            children: vec![2],
        },
        SceneNode::Transform {
            attributes: dict(&[("_name", name.to_owned())]),
            frames: vec![Frame::new(dict(&[(
                "_t",
                format!("{} {} {}", centre[0], centre[1], centre[2]),
            )]))],
            child: 3,
            layer_id: 0,
        },
        SceneNode::Shape {
            attributes: dot_vox::Dict::default(),
            models: vec![ShapeModel {
                model_id: 0,
                attributes: dot_vox::Dict::default(),
            }],
        },
    ]
}

/// MagicaVoxel's *Emit* material for 0-based palette slot `slot` (material id = file index).
fn emit_material(slot: usize) -> Material {
    let properties = [
        ("_type", "_emit"),
        ("_weight", "1"),
        ("_emit", "1"),
        ("_flux", "0"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    Material {
        id: u32::try_from(slot + 1).expect("invariant: at most 255 palette slots are used"),
        properties,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(v: u8) -> Colour {
        Colour {
            emissive: false,
            rgb: [v, v, v],
        }
    }

    const GLOW: Colour = Colour {
        emissive: true,
        rgb: [0x7f, 0xe6, 0xff],
    };

    fn sample() -> VoxelModel {
        let mut m = VoxelModel::new();
        // Odd span in x (7), even in y (4) and z (2), negative and positive coordinates.
        for x in -3..=3 {
            for y in -2..=1 {
                m.insert([x, y, 5], grey(100)).unwrap();
            }
        }
        m.insert([0, 0, 6], GLOW).unwrap();
        m.insert([3, 1, 6], grey(40)).unwrap();
        m
    }

    #[test]
    fn write_then_read_gives_the_same_model() {
        let m = sample();
        let bytes = write(&m, "pylon").unwrap();
        assert_eq!(read(&bytes).unwrap(), m);
    }

    #[test]
    fn odd_and_even_spans_keep_their_origin_for_every_offset() {
        for size in 1..=6 {
            for start in -4..=4 {
                let mut m = VoxelModel::new();
                m.insert([start, 0, 0], grey(1)).unwrap();
                m.insert([start + size - 1, size - 1, size], grey(2))
                    .unwrap();
                let back = read(&write(&m, "box").unwrap()).unwrap();
                assert_eq!(back, m, "size {size}, start {start}");
            }
        }
    }

    /// The same object, rewritten with the given transform attributes on the object node.
    fn with_object_attributes(
        attributes: &[(&str, &str)],
        frame: &[(&str, &str)],
    ) -> Result<VoxelModel, VoxError> {
        let bytes = write(&sample(), "obj").unwrap();
        let mut data = dot_vox::load_bytes(&bytes).unwrap();
        if let SceneNode::Transform {
            attributes: a,
            frames,
            ..
        } = &mut data.scenes[2]
        {
            for (k, v) in attributes {
                a.insert((*k).to_owned(), (*v).to_owned());
            }
            for (k, v) in frame {
                frames[0]
                    .attributes
                    .insert((*k).to_owned(), (*v).to_owned());
            }
        }
        let mut out = Vec::new();
        data.write_vox(&mut out).unwrap();
        read(&out)
    }

    #[test]
    fn rotated_and_mirrored_objects_are_refused_without_panicking() {
        // 4 is the identity; 0xff is not even a valid rotation byte (dot_vox would assert).
        assert!(with_object_attributes(&[], &[("_r", "4")]).is_ok());
        for r in ["20", "36", "0", "255", "x"] {
            let err = with_object_attributes(&[], &[("_r", r)]).unwrap_err();
            assert!(
                matches!(err, VoxError::Rotated { ref object } if object == "obj"),
                "{r}: {err}"
            );
        }
    }

    #[test]
    fn hidden_objects_and_malformed_positions() {
        assert!(matches!(
            with_object_attributes(&[("_hidden", "1")], &[]),
            Err(VoxError::Empty)
        ));
        assert!(matches!(
            with_object_attributes(&[], &[("_t", "1 2")]),
            Err(VoxError::SceneGraph(_))
        ));
        let moved = with_object_attributes(&[], &[("_t", "10 0 0")]).unwrap();
        // The object's centre was [0, 0, 6] (min + span / 2); it moves to [10, 0, 0].
        assert_eq!(moved.bounds().unwrap().0, [7, -2, -1]);
    }

    #[test]
    fn hidden_layers_are_skipped_and_objects_merge() {
        let bytes = write(&sample(), "a").unwrap();
        let mut data = dot_vox::load_bytes(&bytes).unwrap();
        // A second object, the same model moved 20 voxels along x, on layer 1.
        data.scenes.push(SceneNode::Transform {
            attributes: dot_vox::Dict::default(),
            frames: vec![Frame::new(
                [("_t".to_owned(), "20 0 0".to_owned())]
                    .into_iter()
                    .collect(),
            )],
            child: 5,
            layer_id: 1,
        });
        data.scenes.push(SceneNode::Shape {
            attributes: dot_vox::Dict::default(),
            models: vec![ShapeModel {
                model_id: 0,
                attributes: dot_vox::Dict::default(),
            }],
        });
        if let SceneNode::Group { children, .. } = &mut data.scenes[1] {
            children.push(4);
        }
        let encode = |data: &DotVoxData| {
            let mut out = Vec::new();
            data.write_vox(&mut out).unwrap();
            out
        };
        let both = read(&encode(&data)).unwrap();
        assert_eq!(both.len(), 2 * sample().len());

        data.layers[1]
            .attributes
            .insert("_hidden".to_owned(), "1".to_owned());
        assert_eq!(read(&encode(&data)).unwrap(), sample());

        // Moved onto the first object instead: they overlap.
        data.layers[1].attributes.clear();
        if let SceneNode::Transform { frames, .. } = &mut data.scenes[4] {
            frames[0]
                .attributes
                .insert("_t".to_owned(), "0 0 0".to_owned());
        }
        if let SceneNode::Transform { frames, .. } = &mut data.scenes[2] {
            frames[0]
                .attributes
                .insert("_t".to_owned(), "0 0 0".to_owned());
        }
        assert!(matches!(read(&encode(&data)), Err(VoxError::Overlap(_))));
    }

    #[test]
    fn too_many_colours_or_too_large_is_an_error() {
        let mut m = VoxelModel::new();
        for i in 0..256_i32 {
            let v = u8::try_from(i).unwrap();
            m.insert(
                [i, 0, 0],
                Colour {
                    emissive: false,
                    rgb: [v, 0, 0],
                },
            )
            .unwrap();
        }
        assert!(matches!(write(&m, "x"), Err(VoxError::TooManyColours(256))));
        let mut m = VoxelModel::new();
        m.insert([0, 0, 0], grey(1)).unwrap();
        m.insert([256, 0, 0], grey(1)).unwrap();
        assert!(matches!(
            write(&m, "x"),
            Err(VoxError::TooLarge([257, 1, 1]))
        ));
    }

    #[test]
    fn junk_is_a_parse_error() {
        assert!(matches!(read(b"not a vox file"), Err(VoxError::Parse(_))));
        assert!(matches!(read(&[]), Err(VoxError::Parse(_))));
        let mut truncated = write(&sample(), "a").unwrap();
        truncated.truncate(40);
        assert!(read(&truncated).is_err());
    }
}
