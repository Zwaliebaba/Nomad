//! `nomad-vox`: converts between the MagicaVoxel `.vox` art sources and the game's JSON (D-289).
//!
//! ```text
//! nomad-vox import [--check] [SET_DIR...]       # .vox -> JSON; --check only reports, exit 1 if stale
//! nomad-vox export [--force] [--from JSON] SET_DIR...   # JSON -> .vox (seeding, recovery)
//! ```
//!
//! With no `SET_DIR`, `import` handles every set under `art/voxels` (run from the repository
//! root). `export` refuses to overwrite `.vox` files without `--force`, because they are the
//! source of truth. `--from` exports from another JSON file than the set's own output.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nomad_vox::set::{self, Set};

const DEFAULT_ROOT: &str = "art/voxels";
const USAGE: &str = "usage: nomad-vox import [--check] [SET_DIR...]\n       \
                     nomad-vox export [--force] [--from JSON] SET_DIR...";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(io::stderr(), "nomad-vox: {message}");
            ExitCode::FAILURE
        }
    }
}

fn say(line: &str) {
    let _ = writeln!(io::stdout(), "{line}");
}

fn run(args: Vec<String>) -> Result<ExitCode, String> {
    let mut args = args.into_iter();
    let command = args.next().ok_or(USAGE)?;
    let mut check = false;
    let mut force = false;
    let mut from: Option<PathBuf> = None;
    let mut dirs = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" if command == "import" => check = true,
            "--force" if command == "export" => force = true,
            "--from" if command == "export" => {
                from = Some(PathBuf::from(args.next().ok_or("--from needs a path")?));
            }
            "-h" | "--help" => {
                say(USAGE);
                return Ok(ExitCode::SUCCESS);
            }
            a if a.starts_with('-') => return Err(format!("unknown option `{a}`\n{USAGE}")),
            a => dirs.push(PathBuf::from(a)),
        }
    }
    match command.as_str() {
        "import" => import(dirs, check),
        "export" => {
            if dirs.is_empty() {
                return Err(format!("export needs at least one SET_DIR\n{USAGE}"));
            }
            if from.is_some() && dirs.len() > 1 {
                return Err("--from takes exactly one SET_DIR".into());
            }
            for dir in &dirs {
                let set = Set::load(dir).map_err(|e| e.to_string())?;
                for path in set
                    .export(from.as_deref(), force)
                    .map_err(|e| e.to_string())?
                {
                    say(&format!("wrote {}", path.display()));
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        "-h" | "--help" => {
            say(USAGE);
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command `{other}`\n{USAGE}")),
    }
}

fn import(mut dirs: Vec<PathBuf>, check: bool) -> Result<ExitCode, String> {
    if dirs.is_empty() {
        dirs = set::discover(Path::new(DEFAULT_ROOT)).map_err(|e| e.to_string())?;
        if dirs.is_empty() {
            return Err(format!(
                "no sets under {DEFAULT_ROOT} (run from the repository root)"
            ));
        }
    }
    let mut stale = 0;
    for dir in &dirs {
        let set = Set::load(dir).map_err(|e| e.to_string())?;
        let out = set.output_path();
        if check {
            if set.is_current().map_err(|e| e.to_string())? {
                say(&format!("{}: up to date", set.name()));
            } else {
                stale += 1;
                say(&format!("{}: {} is out of date", set.name(), out.display()));
            }
        } else if set.import_and_write().map_err(|e| e.to_string())? {
            say(&format!("{}: wrote {}", set.name(), out.display()));
        } else {
            say(&format!("{}: {} unchanged", set.name(), out.display()));
        }
    }
    Ok(if stale > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
