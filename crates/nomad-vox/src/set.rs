//! Asset sets: a directory of `.vox` files plus a `set.json` manifest naming the models and the
//! JSON file generated from them (D-289).
//!
//! ```jsonc
//! {
//!   "format": "nomad-vox/set@1",
//!   "kind": "models",                   // or "ships"
//!   "output": "ModuleCatalog.json",
//!   "axes": "+x away from the core, +z up; origin at the slot centre on the ops grid",
//!   "status": "...",                    // ships only: copied into the output
//!   "colour_names": { "#9aa1af": "hull" },  // ships only: palette role names
//!   "models": [ { "id": "refinery", "name": "Refinery" } ]  // file: <id>.vox
//! }
//! ```
//!
//! `output` is relative to the set's directory. The `.vox` files are the source of truth;
//! the output is generated and must not be edited by hand.
//! A model may specify `"file": "CommandCore.vox"` to keep artist-facing filenames separate
//! from its stable lowercase id. Without `file`, the source remains `<id>.vox`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::formats::{self, FormatError, NamedModel, ShipsHeader};
use crate::vox::{self, VoxError};

pub const SET_FORMAT: &str = "nomad-vox/set@1";
pub const MANIFEST: &str = "set.json";

#[derive(Debug, thiserror::Error)]
pub enum SetError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Manifest { path: PathBuf, message: String },
    #[error("{path}: {source}")]
    Vox { path: PathBuf, source: VoxError },
    #[error("{path}: {source}")]
    Format { path: PathBuf, source: FormatError },
    #[error(
        "{path} already exists; the .vox files are the source of truth (pass --force to overwrite)"
    )]
    Exists { path: PathBuf },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetKind {
    Models,
    Ships,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    kind: SetKind,
    output: String,
    axes: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    colour_names: BTreeMap<String, String>,
    models: Vec<ManifestModel>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestModel {
    id: String,
    name: String,
    /// Optional source filename, allowing artist-facing names without changing model ids.
    #[serde(default)]
    file: Option<String>,
}

/// A loaded, validated set.
#[derive(Debug)]
pub struct Set {
    dir: PathBuf,
    manifest: Manifest,
}

fn read_text(path: &Path) -> Result<String, SetError> {
    fs::read_to_string(path).map_err(|source| SetError::Io {
        path: path.to_owned(),
        source,
    })
}

impl Set {
    /// Loads `dir/set.json` and checks it.
    pub fn load(dir: &Path) -> Result<Set, SetError> {
        let path = dir.join(MANIFEST);
        let bad = |message: String| SetError::Manifest {
            path: path.clone(),
            message,
        };
        let manifest: Manifest =
            serde_json::from_str(&read_text(&path)?).map_err(|e| bad(e.to_string()))?;
        if manifest.format != SET_FORMAT {
            return Err(bad(format!(
                "format is `{}`, expected `{SET_FORMAT}`",
                manifest.format
            )));
        }
        if manifest.models.is_empty() {
            return Err(bad("lists no models".into()));
        }
        let mut ids = BTreeSet::new();
        let mut files = BTreeSet::new();
        for m in &manifest.models {
            let valid = !m.id.is_empty()
                && m.id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !valid {
                return Err(bad(format!(
                    "model id `{}` must be lower-case letters, digits and `_`",
                    m.id
                )));
            }
            if !ids.insert(m.id.as_str()) {
                return Err(bad(format!("model id `{}` is listed twice", m.id)));
            }
            let filename = m.file.clone().unwrap_or_else(|| format!("{}.vox", m.id));
            let valid_file = filename.strip_suffix(".vox").is_some_and(|stem| {
                !stem.is_empty()
                    && stem
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            });
            if !valid_file || !files.insert(filename.to_ascii_lowercase()) {
                return Err(bad(format!(
                    "source file `{filename}` must be a unique .vox filename without a directory"
                )));
            }
        }
        let mut names = BTreeSet::new();
        for (hex, name) in &manifest.colour_names {
            if crate::model::Colour::from_hex(hex, false).is_none() {
                return Err(bad(format!("colour_names key `{hex}` is not #rrggbb")));
            }
            if !names.insert(name) {
                return Err(bad(format!("colour name `{name}` is used twice")));
            }
        }
        if manifest.kind == SetKind::Models && !manifest.colour_names.is_empty() {
            return Err(bad("colour_names applies to ships sets only".into()));
        }
        Ok(Set {
            dir: dir.to_owned(),
            manifest,
        })
    }

    /// The set's name: its directory name.
    pub fn name(&self) -> String {
        self.dir.file_name().map_or_else(
            || self.dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub fn kind(&self) -> SetKind {
        self.manifest.kind
    }

    /// The generated JSON file.
    pub fn output_path(&self) -> PathBuf {
        normalise(&self.dir.join(&self.manifest.output))
    }

    pub fn vox_path(&self, id: &str) -> PathBuf {
        if let Some(filename) = self
            .manifest
            .models
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.file.as_deref())
        {
            return self.dir.join(filename);
        }
        self.dir.join(format!("{id}.vox"))
    }

    pub fn model_ids(&self) -> impl Iterator<Item = &str> {
        self.manifest.models.iter().map(|m| m.id.as_str())
    }

    fn generated_note(&self) -> String {
        format!(
            "Generated by nomad-vox from the .vox files of the `{}` set (D-289). Edit those in \
             MagicaVoxel and run `cargo run -p nomad-vox -- import`; do not edit this file.",
            self.name()
        )
    }

    /// Reads every `.vox` file of the set and returns the output file's text.
    pub fn import(&self) -> Result<String, SetError> {
        let mut models = Vec::new();
        for m in &self.manifest.models {
            let path = self.vox_path(&m.id);
            let bytes = fs::read(&path).map_err(|source| SetError::Io {
                path: path.clone(),
                source,
            })?;
            let model = vox::read(&bytes).map_err(|source| SetError::Vox { path, source })?;
            models.push(NamedModel {
                id: m.id.clone(),
                name: m.name.clone(),
                model,
            });
        }
        let generated = self.generated_note();
        match self.manifest.kind {
            SetKind::Models => Ok(formats::write_models(
                &generated,
                &self.manifest.axes,
                &models,
            )),
            SetKind::Ships => {
                let header = ShipsHeader {
                    generated: &generated,
                    status: self.manifest.status.as_deref().unwrap_or(""),
                    axes: &self.manifest.axes,
                    colour_names: &self.manifest.colour_names,
                };
                formats::write_ships(&header, &models).map_err(|source| SetError::Format {
                    path: self.output_path(),
                    source,
                })
            }
        }
    }

    /// Whether the output file matches what [`Set::import`] produces.
    pub fn is_current(&self) -> Result<bool, SetError> {
        let expected = self.import()?;
        match fs::read_to_string(self.output_path()) {
            Ok(found) => Ok(found == expected),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(SetError::Io {
                path: self.output_path(),
                source,
            }),
        }
    }

    /// Imports the set and writes the output file. Returns whether it changed.
    pub fn import_and_write(&self) -> Result<bool, SetError> {
        let text = self.import()?;
        let path = self.output_path();
        if fs::read_to_string(&path).is_ok_and(|old| old == text) {
            return Ok(false);
        }
        fs::write(&path, text).map_err(|source| SetError::Io { path, source })?;
        Ok(true)
    }

    /// Writes one `.vox` file per model from the output file (`source`, or the set's own
    /// output when `None`). Refuses to overwrite an existing `.vox` file unless `force`:
    /// the `.vox` files are the source of truth, so this is for seeding a set and for
    /// recovering a lost file.
    pub fn export(&self, source: Option<&Path>, force: bool) -> Result<Vec<PathBuf>, SetError> {
        let path = source.map_or_else(|| self.output_path(), Path::to_owned);
        let text = read_text(&path)?;
        let parsed = match self.manifest.kind {
            SetKind::Models => formats::read_models(&text),
            SetKind::Ships => formats::read_ships(&text).map(|(models, _)| models),
        };
        let models = parsed.map_err(|source| SetError::Format {
            path: path.clone(),
            source,
        })?;
        let by_id: BTreeMap<&str, &NamedModel> =
            models.iter().map(|m| (m.id.as_str(), m)).collect();

        let mut planned = Vec::new();
        for m in &self.manifest.models {
            let model = by_id.get(m.id.as_str()).ok_or_else(|| SetError::Manifest {
                path: self.dir.join(MANIFEST),
                message: format!("model `{}` is not in {}", m.id, path.display()),
            })?;
            let target = self.vox_path(&m.id);
            if !force && target.exists() {
                return Err(SetError::Exists { path: target });
            }
            let bytes = vox::write(&model.model, &m.id).map_err(|source| SetError::Vox {
                path: target.clone(),
                source,
            })?;
            planned.push((target, bytes));
        }
        // Write only once every model has converted, so a failure leaves no half-exported set.
        let mut written = Vec::new();
        for (target, bytes) in planned {
            fs::write(&target, bytes).map_err(|source| SetError::Io {
                path: target.clone(),
                source,
            })?;
            written.push(target);
        }
        Ok(written)
    }
}

/// Removes `name/..` pairs lexically, so messages show `Design/...` rather than
/// `art/voxels/modules/../../../Design/...`. A leading `..` is kept.
fn normalise(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(out.components().next_back(), Some(Component::Normal(_))) =>
            {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The set directories directly under `root` (those holding a `set.json`), sorted.
pub fn discover(root: &Path) -> Result<Vec<PathBuf>, SetError> {
    let entries = fs::read_dir(root).map_err(|source| SetError::Io {
        path: root.to_owned(),
        source,
    })?;
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SetError::Io {
            path: root.to_owned(),
            source,
        })?;
        let dir = entry.path();
        if dir.join(MANIFEST).is_file() {
            dirs.push(dir);
        }
    }
    dirs.sort();
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Colour, VoxelModel};

    /// A fresh scratch directory for this test process.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nomad-vox-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_manifest(dir: &Path, body: &str) {
        fs::write(dir.join(MANIFEST), body).unwrap();
    }

    #[test]
    fn seed_import_check_and_refuse_overwrite() {
        let dir = scratch("set");
        write_manifest(
            &dir,
            r#"{"format":"nomad-vox/set@1","kind":"models","output":"out.json","axes":"+z up",
                "models":[{"id":"a","name":"A"},{"id":"b","name":"B"}]}"#,
        );
        let set = Set::load(&dir).unwrap();
        let grey = Colour {
            emissive: false,
            rgb: [9, 9, 9],
        };
        let mut a = VoxelModel::new();
        a.insert([-2, 1, 0], grey).unwrap();
        let mut b = VoxelModel::new();
        b.insert(
            [5, 5, 5],
            Colour {
                emissive: true,
                ..grey
            },
        )
        .unwrap();
        let seed = formats::write_models(
            "seed",
            "+z up",
            &[
                NamedModel {
                    id: "a".into(),
                    name: "A".into(),
                    model: a,
                },
                NamedModel {
                    id: "b".into(),
                    name: "B".into(),
                    model: b,
                },
            ],
        );
        let seed_path = dir.join("seed.json");
        fs::write(&seed_path, &seed).unwrap();

        assert_eq!(set.export(Some(&seed_path), false).unwrap().len(), 2);
        assert!(matches!(
            set.export(Some(&seed_path), false),
            Err(SetError::Exists { .. })
        ));
        assert!(!set.is_current().unwrap());
        assert!(set.import_and_write().unwrap());
        assert!(set.is_current().unwrap());
        assert!(!set.import_and_write().unwrap());
        // The generated file holds the same models as the seed.
        let out = fs::read_to_string(set.output_path()).unwrap();
        assert_eq!(
            formats::read_models(&out).unwrap(),
            formats::read_models(&seed).unwrap()
        );
        assert!(discover(dir.parent().unwrap()).unwrap().contains(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_manifests_are_refused() {
        let dir = scratch("bad");
        for body in [
            r#"{"format":"x","kind":"models","output":"o","axes":"","models":[{"id":"a","name":"A"}]}"#,
            r#"{"format":"nomad-vox/set@1","kind":"models","output":"o","axes":"","models":[]}"#,
            r#"{"format":"nomad-vox/set@1","kind":"models","output":"o","axes":"","models":[{"id":"A b","name":"A"}]}"#,
            r#"{"format":"nomad-vox/set@1","kind":"models","output":"o","axes":"","models":[{"id":"a","name":"A"},{"id":"a","name":"B"}]}"#,
            r##"{"format":"nomad-vox/set@1","kind":"models","output":"o","axes":"","colour_names":{"#000000":"x"},"models":[{"id":"a","name":"A"}]}"##,
            r##"{"format":"nomad-vox/set@1","kind":"ships","output":"o","axes":"","colour_names":{"#000000":"x","#ffffff":"x"},"models":[{"id":"a","name":"A"}]}"##,
            r#"{"format":"nomad-vox/set@1","kind":"models","output":"o","axes":"","extra":1,"models":[{"id":"a","name":"A"}]}"#,
        ] {
            write_manifest(&dir, body);
            assert!(
                matches!(Set::load(&dir), Err(SetError::Manifest { .. })),
                "{body}"
            );
        }
    }

    #[test]
    fn pascal_case_source_names_preserve_model_ids_and_export_paths() {
        let dir = scratch("source-name");
        write_manifest(
            &dir,
            r#"{"format":"nomad-vox/set@1","kind":"models","output":"out.json","axes":"+z up",
            "models":[{"id":"command_core","name":"Core","file":"CommandCore.vox"}]}"#,
        );
        let set = Set::load(&dir).unwrap();
        let mut model = VoxelModel::new();
        model
            .insert(
                [0, 0, 0],
                Colour {
                    emissive: false,
                    rgb: [30, 40, 50],
                },
            )
            .unwrap();
        fs::write(
            dir.join("CommandCore.vox"),
            vox::write(&model, "Core").unwrap(),
        )
        .unwrap();
        assert!(set.import_and_write().unwrap());
        assert_eq!(
            formats::read_models(&fs::read_to_string(set.output_path()).unwrap()).unwrap()[0].id,
            "command_core"
        );
        fs::remove_file(dir.join("CommandCore.vox")).unwrap();
        assert_eq!(
            set.export(None, false).unwrap(),
            vec![dir.join("CommandCore.vox")]
        );
        assert!(!dir.join("command_core.vox").exists());
        for files in [
            r#""../CommandCore.vox""#,
            r#""C:\\CommandCore.vox""#,
            r#""CommandCore.VOX""#,
        ] {
            let body = format!(
                r#"{{"format":"nomad-vox/set@1","kind":"models","output":"out.json","axes":"","models":[{{"id":"core","name":"Core","file":{files}}}]}}"#
            );
            write_manifest(&dir, &body);
            assert!(Set::load(&dir).is_err());
        }
        write_manifest(
            &dir,
            r#"{"format":"nomad-vox/set@1","kind":"models","output":"out.json","axes":"",
            "models":[{"id":"a","name":"A","file":"Core.vox"},{"id":"b","name":"B","file":"core.vox"}]}"#,
        );
        assert!(Set::load(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
