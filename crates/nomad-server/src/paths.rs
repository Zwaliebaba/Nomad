//! The data directory, Universe directories and the Universe lock (N2 plan §4.2; doc 20 §6;
//! D-088, D-107).
//!
//! Layout: `<data>/universes/<id>/universe.db`, with `universe.lock` beside it. The data
//! directory is `--data-dir`, else `NOMAD_DATA`, else the per-user local data directory
//! (`%LOCALAPPDATA%\Nomad` on Windows, `~/.local/share/Nomad` on Linux).

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

use nomad_types::UniverseId;

use crate::error::ServerError;
use crate::ids::{parse_universe_id, universe_id_text};

/// Environment variable that overrides the default data directory.
pub const DATA_DIR_ENV: &str = "NOMAD_DATA";

/// File name of a Universe's database.
pub const DATABASE_FILE: &str = "universe.db";

/// File name of a Universe's lock.
pub const LOCK_FILE: &str = "universe.lock";

/// The server's data directory: holds one directory per Universe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// A data directory at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The data directory from `--data-dir` (`explicit`), else `NOMAD_DATA`, else the
    /// per-user default.
    pub fn resolve(explicit: Option<PathBuf>) -> Result<Self, ServerError> {
        Self::resolve_with(explicit, std::env::var_os(DATA_DIR_ENV))
    }

    fn resolve_with(explicit: Option<PathBuf>, env: Option<OsString>) -> Result<Self, ServerError> {
        if let Some(root) = explicit {
            return Ok(Self::new(root));
        }
        if let Some(root) = env.filter(|value| !value.is_empty()) {
            return Ok(Self::new(root));
        }
        let base = directories::BaseDirs::new().ok_or(ServerError::NoDataDir)?;
        Ok(Self::new(base.data_local_dir().join("Nomad")))
    }

    /// The data directory itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory that holds the Universes.
    pub fn universes(&self) -> PathBuf {
        self.root.join("universes")
    }

    /// The directory of Universe `id`.
    pub fn universe(&self, id: UniverseId) -> PathBuf {
        self.universes().join(universe_id_text(id))
    }

    /// Every Universe in the data directory, in identity order. Directories whose name is
    /// not a Universe ID, or that hold no database, are skipped.
    pub fn list(&self) -> Result<Vec<(UniverseId, PathBuf)>, ServerError> {
        let dir = self.universes();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(ServerError::io(format!("cannot list {}", dir.display()), e));
            }
        };
        let mut found = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|e| ServerError::io(format!("cannot list {}", dir.display()), e))?;
            let path = entry.path();
            let id = entry
                .file_name()
                .to_str()
                .and_then(|name| parse_universe_id(name).ok());
            if let Some(id) = id
                && path.join(DATABASE_FILE).is_file()
            {
                found.push((id, path));
            }
        }
        found.sort_by_key(|&(id, _)| id);
        Ok(found)
    }

    /// The directory of the Universe named by `id`, or of the only Universe if `id` is
    /// `None`.
    pub fn select(&self, id: Option<UniverseId>) -> Result<PathBuf, ServerError> {
        if let Some(id) = id {
            let path = self.universe(id);
            return if path.join(DATABASE_FILE).is_file() {
                Ok(path)
            } else {
                Err(ServerError::UniverseNotFound { path })
            };
        }
        let mut all = self.list()?;
        match all.len() {
            0 => Err(ServerError::UniverseNotFound {
                path: self.universes(),
            }),
            1 => Ok(all.remove(0).1),
            count => Err(ServerError::UniverseAmbiguous {
                count,
                data_dir: self.root.clone(),
            }),
        }
    }
}

/// The exclusive lock on one Universe, held for as long as a server runs it (N2 plan
/// §4.2). Released when dropped, or by the operating system if the process dies.
#[derive(Debug)]
pub struct UniverseLock {
    _file: File,
}

impl UniverseLock {
    /// Takes the lock of the Universe in `dir`, or fails at once with
    /// [`ServerError::Locked`] if another server holds it.
    pub fn acquire(dir: &Path) -> Result<Self, ServerError> {
        let path = dir.join(LOCK_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| ServerError::io(format!("cannot open {}", path.display()), e))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(ServerError::Locked {
                dir: dir.to_owned(),
                lock: path,
            }),
            Err(TryLockError::Error(e)) => Err(ServerError::io(
                format!("cannot lock {}", path.display()),
                e,
            )),
        }
    }

    /// Whether a server currently holds the lock of the Universe in `dir`. Takes and drops
    /// the lock if it is free; never waits.
    pub fn is_held(dir: &Path) -> Result<bool, ServerError> {
        match Self::acquire(dir) {
            Ok(_lock) => Ok(false),
            Err(ServerError::Locked { .. }) => Ok(true),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_beats_environment_beats_default() {
        let explicit = DataDir::resolve_with(Some("a".into()), Some("b".into())).unwrap();
        assert_eq!(explicit.root(), Path::new("a"));
        let env = DataDir::resolve_with(None, Some("b".into())).unwrap();
        assert_eq!(env.root(), Path::new("b"));
        // An empty variable counts as unset.
        if let Ok(default) = DataDir::resolve_with(None, Some(OsString::new())) {
            assert!(default.root().ends_with("Nomad"));
        }
    }

    #[test]
    fn universe_paths_use_the_text_id() {
        let data = DataDir::new("data");
        let path = data.universe(UniverseId(0x12_3456_789a));
        assert_eq!(
            path,
            Path::new("data")
                .join("universes")
                .join(universe_id_text(UniverseId(0x12_3456_789a)))
        );
    }
}
