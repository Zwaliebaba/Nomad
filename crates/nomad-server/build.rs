//! Embeds the build identity in `NOMAD_BUILD_ID` (N2 plan §4.6, D-048, D-049).
//!
//! The build identity is the git commit the server was built from. Snapshots and journal
//! entries record it, and a journal is only replayed by the build that wrote it. Packaging
//! can set `NOMAD_BUILD_ID` explicitly; without git the identity is `unknown`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let mut directives = vec![
        "cargo::rerun-if-changed=build.rs".to_owned(),
        "cargo::rerun-if-env-changed=NOMAD_BUILD_ID".to_owned(),
    ];

    let explicit = std::env::var("NOMAD_BUILD_ID")
        .ok()
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty());
    let build = if let Some(id) = explicit {
        id
    } else {
        directives.extend(
            git_watch_paths(&manifest_dir)
                .into_iter()
                .map(|path| format!("cargo::rerun-if-changed={}", path.display())),
        );
        git(&manifest_dir, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned())
    };
    directives.push(format!("cargo::rustc-env=NOMAD_BUILD_ID={build}"));

    let mut out = std::io::stdout().lock();
    for directive in directives {
        writeln!(out, "{directive}").expect("invariant: cargo captures build-script output");
    }
}

/// Runs git in `dir` and returns its trimmed output, or `None` if git is missing or fails.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The files whose change means HEAD moved: `HEAD` itself, the branch it points to and the
/// packed refs. Works in worktrees, where `HEAD` and the refs live in different directories.
fn git_watch_paths(dir: &Path) -> Vec<PathBuf> {
    let absolute = |path: String| {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            dir.join(path)
        }
    };
    let Some(git_dir) = git(dir, &["rev-parse", "--git-dir"]).map(absolute) else {
        return Vec::new();
    };
    let common_dir =
        git(dir, &["rev-parse", "--git-common-dir"]).map_or_else(|| git_dir.clone(), absolute);
    let mut paths = vec![git_dir.join("HEAD"), common_dir.join("packed-refs")];
    if let Some(branch) = git(dir, &["symbolic-ref", "-q", "HEAD"]) {
        paths.push(common_dir.join(branch));
    }
    // Cargo reruns the script on every build for a path that does not exist.
    paths.retain(|path| path.exists());
    paths
}
