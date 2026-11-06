//! The HUD fonts, `JetBrains Mono` and Rajdhani (phase-1 doc 04 "Common style").
//!
//! Both are OFL-licensed and deliberately not bundled (phase-1 README, ground rule 4; AGENTS.md):
//! the user downloads them into `<asset root>/fonts/`, which is git-ignored. Every `.ttf`, `.otf`
//! and `.ttc` file there (and one folder level below, as in the Google Fonts downloads) is loaded
//! into a font database of its own; system fonts are never scanned, so text looks the same on
//! every machine. Missing or unreadable files are logged and skipped, never fatal: the client then
//! draws its tags without text.

use std::path::{Path, PathBuf};

use glyphon::fontdb;

/// Font files larger than this are skipped (the phase-1 fonts are well under 1 MB each).
const MAX_FONT_FILE_BYTES: u64 = 32 << 20;
/// At most this many font files are loaded.
const MAX_FONT_FILES: usize = 64;
/// How many folder levels below the font folder are searched.
const MAX_DEPTH: usize = 1;

/// Loads every font file under `dir` into a fresh database. Never fails: problems are logged.
pub fn load_fonts(dir: &Path) -> fontdb::Database {
    let mut db = fontdb::Database::new();
    let mut files = Vec::new();
    collect_font_files(dir, 0, &mut files);
    files.sort();
    if files.len() > MAX_FONT_FILES {
        tracing::warn!(
            dir = %dir.display(),
            found = files.len(),
            loaded = MAX_FONT_FILES,
            "too many font files; loading only the first ones"
        );
        files.truncate(MAX_FONT_FILES);
    }
    for path in files {
        match read_font_file(&path) {
            Ok(data) => {
                let before = db.len();
                db.load_font_data(data);
                if db.len() == before {
                    tracing::warn!(path = %path.display(), "not a usable font file; skipped");
                } else {
                    tracing::debug!(path = %path.display(), "font loaded");
                }
            }
            Err(reason) => tracing::warn!(path = %path.display(), %reason, "font skipped"),
        }
    }
    db
}

/// Whether the database holds a face of `family` (case-insensitive, as CSS matches families).
pub fn has_family(db: &fontdb::Database, family: &str) -> bool {
    db.faces().any(|face| {
        face.families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(family))
    })
}

fn is_font_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["ttf", "otf", "ttc"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}

fn collect_font_files(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            if depth == 0 {
                tracing::debug!(dir = %dir.display(), %error, "no font folder");
            }
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `file_type` does not follow symbolic links, so a link cannot make the walk loop.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                collect_font_files(&path, depth + 1, files);
            }
        } else if is_font_file(&path) {
            files.push(path);
        }
    }
}

fn read_font_file(path: &Path) -> Result<Vec<u8>, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_FONT_FILE_BYTES {
        return Err(format!(
            "{size} bytes is more than the {MAX_FONT_FILE_BYTES}-byte limit"
        ));
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_junk_fonts_are_skipped_without_panicking() {
        let dir = std::env::temp_dir().join(format!("nomad-fonts-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("static").join("deeper")).unwrap();
        std::fs::write(dir.join("junk.ttf"), b"definitely not a font").unwrap();
        std::fs::write(dir.join("static").join("empty.otf"), b"").unwrap();
        std::fs::write(dir.join("readme.txt"), b"fonts go here").unwrap();
        std::fs::write(dir.join("static").join("deeper").join("too-deep.ttf"), b"x").unwrap();

        let mut files = Vec::new();
        collect_font_files(&dir, 0, &mut files);
        files.sort();
        assert_eq!(
            files,
            [dir.join("junk.ttf"), dir.join("static").join("empty.otf")]
        );
        let db = load_fonts(&dir);
        assert_eq!(db.len(), 0);
        assert!(!has_family(&db, "JetBrains Mono"));

        assert_eq!(load_fonts(&dir.join("does-not-exist")).len(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
