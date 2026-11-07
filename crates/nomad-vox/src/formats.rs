//! The JSON files the game and the tools read, written from (and read back into) [`VoxelModel`]s.
//!
//! - `voxel-rts/models@1`: one model per module kind (D-278, K1), each at its own origin,
//!   sharing one indexed palette. Defined here (D-289); `tools/station-kit` reads it.
//! - `voxel-rts/ships@1`: the ship models with named palette roles (phase-1 doc 03).
//!
//! Both are written by hand rather than through `serde_json`'s pretty printer: one voxel per
//! line keeps the files small enough and their diffs readable, and the output is byte-for-byte
//! deterministic, which the up-to-date check relies on.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use serde::Deserialize;

use crate::model::{Colour, MAX_ABS_COORD, Pos, VoxelModel};

pub const MODELS_FORMAT: &str = "voxel-rts/models@1";
pub const SHIPS_FORMAT: &str = "voxel-rts/ships@1";

/// A model with its id and display name, in the order its set lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedModel {
    pub id: String,
    pub name: String,
    pub model: VoxelModel,
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not valid JSON for {format}: {source}")]
    Json {
        format: &'static str,
        source: serde_json::Error,
    },
    #[error("format is `{found}`, expected `{expected}`")]
    WrongFormat {
        found: String,
        expected: &'static str,
    },
    #[error("{0}")]
    Invalid(String),
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).expect("invariant: a string always serialises")
}

/// Writes `voxel-rts/models@1`. The palette holds every colour used, in [`Colour`] order, so
/// an edit to one model renumbers no other model's colours unless it adds or drops a colour.
pub fn write_models(generated: &str, axes: &str, models: &[NamedModel]) -> String {
    let palette: Vec<Colour> = models
        .iter()
        .flat_map(|m| m.model.colours())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let index: HashMap<Colour, usize> = palette.iter().enumerate().map(|(i, c)| (*c, i)).collect();

    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"format\": {},", json_str(MODELS_FORMAT));
    let _ = writeln!(out, "  \"generated\": {},", json_str(generated));
    let _ = writeln!(out, "  \"axes\": {},", json_str(axes));
    let _ = writeln!(out, "  \"palette\": [");
    for (i, c) in palette.iter().enumerate() {
        let comma = if i + 1 < palette.len() { "," } else { "" };
        let _ = writeln!(
            out,
            "    {{\"rgb\": \"{}\", \"emissive\": {}}}{comma}",
            c.hex(),
            c.emissive
        );
    }
    let _ = writeln!(out, "  ],");
    let _ = writeln!(out, "  \"models\": [");
    for (mi, m) in models.iter().enumerate() {
        let _ = writeln!(out, "    {{");
        let _ = writeln!(out, "      \"id\": {},", json_str(&m.id));
        let _ = writeln!(out, "      \"name\": {},", json_str(&m.name));
        let _ = writeln!(out, "      \"voxel_count\": {},", m.model.len());
        let _ = writeln!(out, "      \"voxels\": [");
        let n = m.model.len();
        for (vi, ([x, y, z], c)) in m.model.iter().enumerate() {
            let comma = if vi + 1 < n { "," } else { "" };
            let _ = writeln!(out, "        [{x}, {y}, {z}, {}]{comma}", index[&c]);
        }
        let _ = writeln!(out, "      ]");
        let comma = if mi + 1 < models.len() { "," } else { "" };
        let _ = writeln!(out, "    }}{comma}");
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
    out
}

#[derive(Deserialize)]
struct ModelsFile {
    format: String,
    palette: Vec<PaletteEntry>,
    models: Vec<ModelsEntry>,
}

#[derive(Deserialize)]
struct PaletteEntry {
    rgb: String,
    emissive: bool,
}

#[derive(Deserialize)]
struct ModelsEntry {
    id: String,
    name: String,
    voxels: Vec<(i32, i32, i32, usize)>,
}

/// Reads `voxel-rts/models@1` (the inverse of [`write_models`]).
pub fn read_models(text: &str) -> Result<Vec<NamedModel>, FormatError> {
    let file: ModelsFile = serde_json::from_str(text).map_err(|source| FormatError::Json {
        format: MODELS_FORMAT,
        source,
    })?;
    check_format(&file.format, MODELS_FORMAT)?;
    let palette = file
        .palette
        .iter()
        .map(|p| {
            Colour::from_hex(&p.rgb, p.emissive).ok_or_else(|| {
                FormatError::Invalid(format!("palette colour `{}` is not #rrggbb", p.rgb))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    file.models
        .into_iter()
        .map(|m| {
            let mut model = VoxelModel::new();
            for (x, y, z, i) in m.voxels {
                let colour = *palette.get(i).ok_or_else(|| {
                    FormatError::Invalid(format!(
                        "model `{}`: palette index {i} out of range",
                        m.id
                    ))
                })?;
                insert(&mut model, &m.id, [x, y, z], colour)?;
            }
            Ok(NamedModel {
                id: m.id,
                name: m.name,
                model,
            })
        })
        .collect()
}

/// The ships file's header fields, carried by the set manifest.
pub struct ShipsHeader<'a> {
    pub generated: &'a str,
    pub status: &'a str,
    pub axes: &'a str,
    /// Role names for colours, keyed by `#rrggbb` (`hull`, `eng`, ...).
    pub colour_names: &'a BTreeMap<String, String>,
}

/// Names every colour used by the ships: its role from `colour_names`, else `c_rrggbb`; an
/// emissive colour whose RGB is also used non-emissive gets `_emit` appended. Fails if two
/// colours would share a name.
fn ship_colour_names(
    colours: &BTreeSet<Colour>,
    colour_names: &BTreeMap<String, String>,
) -> Result<BTreeMap<Colour, String>, FormatError> {
    let mut names = BTreeMap::new();
    let mut taken = BTreeSet::new();
    for c in colours {
        let hex = c.hex();
        let mut name = colour_names
            .get(&hex)
            .cloned()
            .unwrap_or_else(|| format!("c_{}", &hex[1..]));
        if c.emissive
            && colours.contains(&Colour {
                emissive: false,
                rgb: c.rgb,
            })
        {
            name.push_str("_emit");
        }
        if !taken.insert(name.clone()) {
            return Err(FormatError::Invalid(format!(
                "two ship colours would both be named `{name}`; give them distinct names in colour_names"
            )));
        }
        names.insert(*c, name);
    }
    Ok(names)
}

/// Writes `voxel-rts/ships@1` (phase-1 doc 03), palette roles in [`Colour`] order.
pub fn write_ships(header: &ShipsHeader<'_>, models: &[NamedModel]) -> Result<String, FormatError> {
    let colours: BTreeSet<Colour> = models.iter().flat_map(|m| m.model.colours()).collect();
    let names = ship_colour_names(&colours, header.colour_names)?;

    let mut out = String::new();
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"format\": {},", json_str(SHIPS_FORMAT));
    let _ = writeln!(out, "  \"generated\": {},", json_str(header.generated));
    let _ = writeln!(out, "  \"status\": {},", json_str(header.status));
    let _ = writeln!(out, "  \"axes\": {},", json_str(header.axes));
    let _ = writeln!(out, "  \"palette\": {{");
    for (i, (c, name)) in names.iter().enumerate() {
        let comma = if i + 1 < names.len() { "," } else { "" };
        let _ = writeln!(out, "    {}: \"{}\"{comma}", json_str(name), c.hex());
    }
    let _ = writeln!(out, "  }},");
    let emissive: Vec<String> = names
        .iter()
        .filter(|(c, _)| c.emissive)
        .map(|(_, n)| json_str(n))
        .collect();
    let _ = writeln!(out, "  \"emissive\": [{}],", emissive.join(", "));
    let _ = writeln!(out, "  \"models\": {{");
    for (mi, m) in models.iter().enumerate() {
        let _ = writeln!(out, "    {}: {{", json_str(&m.id));
        let _ = writeln!(out, "      \"name\": {},", json_str(&m.name));
        let _ = writeln!(out, "      \"voxels\": [");
        let n = m.model.len();
        for (vi, ([x, y, z], c)) in m.model.iter().enumerate() {
            let comma = if vi + 1 < n { "," } else { "" };
            let _ = writeln!(
                out,
                "        [{x}, {y}, {z}, {}]{comma}",
                json_str(&names[&c])
            );
        }
        let _ = writeln!(out, "      ]");
        let comma = if mi + 1 < models.len() { "," } else { "" };
        let _ = writeln!(out, "    }}{comma}");
    }
    let _ = writeln!(out, "  }}");
    let _ = writeln!(out, "}}");
    Ok(out)
}

#[derive(Deserialize)]
struct ShipsFile {
    format: String,
    palette: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    emissive: Vec<String>,
    models: serde_json::Value,
}

#[derive(Deserialize)]
struct ShipEntry {
    name: String,
    voxels: Vec<(i32, i32, i32, String)>,
}

/// Reads `voxel-rts/ships@1`, returning the models (sorted by id) and the colour role names
/// keyed by `#rrggbb`.
pub fn read_ships(text: &str) -> Result<(Vec<NamedModel>, BTreeMap<String, String>), FormatError> {
    let file: ShipsFile = serde_json::from_str(text).map_err(|source| FormatError::Json {
        format: SHIPS_FORMAT,
        source,
    })?;
    check_format(&file.format, SHIPS_FORMAT)?;
    let mut roles = HashMap::new();
    let mut colour_names = BTreeMap::new();
    for (name, value) in &file.palette {
        let hex = value.as_str().ok_or_else(|| {
            FormatError::Invalid(format!("palette role `{name}` is not a string"))
        })?;
        let colour = Colour::from_hex(hex, file.emissive.contains(name)).ok_or_else(|| {
            FormatError::Invalid(format!("palette role `{name}`: `{hex}` is not #rrggbb"))
        })?;
        roles.insert(name.clone(), colour);
        colour_names
            .entry(colour.hex())
            .or_insert_with(|| name.clone());
    }
    let models: BTreeMap<String, ShipEntry> =
        serde_json::from_value(file.models).map_err(|source| FormatError::Json {
            format: SHIPS_FORMAT,
            source,
        })?;
    let mut out = Vec::new();
    for (id, entry) in models {
        let mut model = VoxelModel::new();
        for (x, y, z, role) in entry.voxels {
            let colour = *roles.get(&role).ok_or_else(|| {
                FormatError::Invalid(format!("ship `{id}`: palette role `{role}` is not defined"))
            })?;
            insert(&mut model, &id, [x, y, z], colour)?;
        }
        out.push(NamedModel {
            id,
            name: entry.name,
            model,
        });
    }
    Ok((out, colour_names))
}

fn check_format(found: &str, expected: &'static str) -> Result<(), FormatError> {
    if found == expected {
        Ok(())
    } else {
        Err(FormatError::WrongFormat {
            found: found.to_owned(),
            expected,
        })
    }
}

fn insert(model: &mut VoxelModel, id: &str, pos: Pos, colour: Colour) -> Result<(), FormatError> {
    if pos.iter().any(|c| c.abs() > MAX_ABS_COORD) {
        return Err(FormatError::Invalid(format!(
            "model `{id}`: voxel {pos:?} lies outside ±{MAX_ABS_COORD}"
        )));
    }
    model
        .insert(pos, colour)
        .map_err(|p| FormatError::Invalid(format!("model `{id}`: two voxels at {p:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(hex: &str, emissive: bool) -> Colour {
        Colour::from_hex(hex, emissive).unwrap()
    }

    fn models() -> Vec<NamedModel> {
        let mut a = VoxelModel::new();
        a.insert([-1, 0, 2], c("#8a909f", false)).unwrap();
        a.insert([0, 0, 2], c("#7fe6ff", true)).unwrap();
        let mut b = VoxelModel::new();
        b.insert([3, -4, 0], c("#8a909f", false)).unwrap();
        b.insert([3, -4, 1], c("#7fe6ff", false)).unwrap();
        vec![
            NamedModel {
                id: "pylon".into(),
                name: "Shield \"pylon\"".into(),
                model: a,
            },
            NamedModel {
                id: "turret".into(),
                name: "Turret".into(),
                model: b,
            },
        ]
    }

    #[test]
    fn models_round_trip_and_are_valid_json() {
        let text = write_models("gen", "axes", &models());
        let _: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(read_models(&text).unwrap(), models());
        // Non-emissive colours first, then by RGB.
        assert!(
            text.find("\"#7fe6ff\", \"emissive\": false").unwrap()
                < text.find("\"#8a909f\"").unwrap()
        );
    }

    #[test]
    fn ships_round_trip_with_role_names() {
        let names: BTreeMap<String, String> = [("#8a909f", "hull"), ("#7fe6ff", "eng")]
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let header = ShipsHeader {
            generated: "gen",
            status: "PLACEHOLDER",
            axes: "+x nose",
            colour_names: &names,
        };
        let text = write_ships(&header, &models()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        // #7fe6ff is used both ways, so the emissive one is `eng_emit`.
        assert_eq!(value["emissive"], serde_json::json!(["eng_emit"]));
        assert_eq!(value["palette"]["eng"], "#7fe6ff");
        let (back, _) = read_ships(&text).unwrap();
        assert_eq!(back, models());
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(matches!(
            read_models("{\"format\":\"x\",\"palette\":[],\"models\":[]}"),
            Err(FormatError::WrongFormat { .. })
        ));
        let bad_index = "{\"format\":\"voxel-rts/models@1\",\"palette\":[],\"models\":[{\"id\":\"a\",\"name\":\"a\",\"voxels\":[[0,0,0,0]]}]}";
        assert!(matches!(
            read_models(bad_index),
            Err(FormatError::Invalid(_))
        ));
        let dup = "{\"format\":\"voxel-rts/ships@1\",\"palette\":{\"h\":\"#000000\"},\"models\":{\"s\":{\"name\":\"s\",\"voxels\":[[0,0,0,\"h\"],[0,0,0,\"h\"]]}}}";
        assert!(matches!(read_ships(dup), Err(FormatError::Invalid(_))));
        assert!(read_models("[").is_err());
    }
}
