//! Profile directory that holds the final executables (`target/debug`, or
//! `target/<triple>/debug` when `--target` is set).
//!
//! Cargo 1.100 stores a build script's `OUT_DIR` at
//! `<profile>/build/<package>/<hash>/out`. Older Cargo uses
//! `<profile>/build/<package>-<hash>/out`. The executables stay in the profile
//! directory either way. Counting a fixed number of parents lands inside
//! `build/` on the new layout, so the ONNX Runtime library never sat next to
//! `mumcord` and a fresh E2E package could not find it.

use std::path::{Path, PathBuf};

/// The Cargo profile directory above `out_dir`.
///
/// Prefers the directory Cargo locks (`.cargo-lock` next to `build/`). Falls
/// back to the directory named `profile` (`debug` or `release`) when the lock
/// file is not there yet.
pub fn artifact_dir(out_dir: &Path, profile: &str) -> Option<PathBuf> {
    let mut cur = out_dir;
    let mut named = None;
    while let Some(parent) = cur.parent() {
        if parent.join(".cargo-lock").is_file() && parent.join("build").is_dir() {
            return Some(parent.to_path_buf());
        }
        if named.is_none()
            && parent.file_name().is_some_and(|name| name == profile)
            && parent.join("build").is_dir()
        {
            named = Some(parent.to_path_buf());
        }
        cur = parent;
    }
    named
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(rel: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join(rel);
        fs::create_dir_all(&out).expect("out dir");
        (dir, out)
    }

    #[test]
    fn old_layout_stops_at_the_profile_dir() {
        let (dir, out) = tree("target/debug/build/mumcord-effects-abc/out");
        fs::write(dir.path().join("target/debug/.cargo-lock"), "").unwrap();
        let found = artifact_dir(&out, "debug").unwrap();
        assert_eq!(found, dir.path().join("target/debug"));
    }

    #[test]
    fn cargo_1_100_layout_stops_at_the_profile_dir() {
        let (dir, out) = tree("target/debug/build/mumcord-effects/abc/out");
        fs::write(dir.path().join("target/debug/.cargo-lock"), "").unwrap();
        let found = artifact_dir(&out, "debug").unwrap();
        assert_eq!(found, dir.path().join("target/debug"));
    }

    #[test]
    fn target_triple_layout_stops_above_build() {
        let (dir, out) =
            tree("target/x86_64-unknown-linux-gnu/release/build/mumcord-effects/abc/out");
        let profile = dir.path().join("target/x86_64-unknown-linux-gnu/release");
        fs::write(profile.join(".cargo-lock"), "").unwrap();
        let found = artifact_dir(&out, "release").unwrap();
        assert_eq!(found, profile);
    }

    #[test]
    fn falls_back_to_the_profile_name_without_the_lock_file() {
        let (_dir, out) = tree("target/debug/build/mumcord-effects/abc/out");
        let found = artifact_dir(&out, "debug").unwrap();
        assert_eq!(found.file_name().unwrap(), "debug");
    }
}
