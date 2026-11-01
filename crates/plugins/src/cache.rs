//! Compiled-plugin cache (FR-6.7): Cranelift output is stored as
//! `<cache>/plugins/<sha256>.cwasm`, so a plugin starts without recompiling. The name hashes the
//! component bytes together with the Mumcord version, so a new package or a new Mumcord simply
//! misses the cache; [`prune`] removes files nobody uses any more.
//!
//! Trust: a `.cwasm` file is native code that wasmtime loads without re-checking it. Anyone who
//! can write to the user's cache directory could therefore run code as the user. That is the
//! same trust level as the rest of the user's profile (config, plugins folder), so it is
//! accepted; the files are only ever written by this module.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use semver::Version;
use sha2::{Digest as _, Sha256};
use wasmtime::Engine;
use wasmtime::component::Component;

const DIR: &str = "plugins";
const EXTENSION: &str = "cwasm";

/// The cache key for a component: hex SHA-256 over the component's own SHA-256 and the app
/// version.
pub fn key(wasm: &[u8], app_version: &Version) -> String {
    let mut hasher = Sha256::new();
    hasher.update(Sha256::digest(wasm));
    hasher.update(app_version.to_string().as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn path(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join(DIR).join(format!("{key}.{EXTENSION}"))
}

/// The cached component, or `None` when there is none or it cannot be used (e.g. written by a
/// different wasmtime build); an unusable file is removed.
pub(crate) fn load(engine: &Engine, path: &Path) -> Option<Component> {
    if !path.is_file() {
        return None;
    }
    // SAFETY: the file was written by `store` from `Component::serialize` of the same key; see the
    // module comment for why the cache directory is trusted. wasmtime still checks that it was
    // built for this engine configuration.
    match unsafe { Component::deserialize_file(engine, path) } {
        Ok(component) => Some(component),
        Err(error) => {
            tracing::warn!(%error, "dropping an unusable compiled plugin from the cache");
            if let Err(error) = std::fs::remove_file(path) {
                tracing::warn!(%error, "could not remove the unusable cache file");
            }
            None
        }
    }
}

/// Writes a compiled component atomically (temp file + rename), so a crash never leaves half a
/// file under the final name.
pub(crate) fn store(path: &Path, component: &Component) -> Result<(), String> {
    let bytes = component.serialize().map_err(|e| format!("{e:#}"))?;
    let dir = path.parent().ok_or("cache path has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    temp.write_all(&bytes).map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Removes every cached file whose key is not in `keep` (uninstalled plugins, old versions, an
/// older Mumcord).
pub fn prune(cache_dir: &Path, keep: &[String]) -> std::io::Result<usize> {
    let dir = cache_dir.join(DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0;
    for entry in entries {
        let path = entry?.path();
        let is_cache_file = path.extension().is_some_and(|e| e == EXTENSION);
        let kept = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| keep.iter().any(|k| k == stem));
        if is_cache_file && !kept {
            std::fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_changes_with_the_component_and_the_app_version() {
        let v1 = Version::new(0, 7, 0);
        let k = key(b"component", &v1);
        assert_eq!(k.len(), 64);
        assert_eq!(k, key(b"component", &v1));
        assert_ne!(k, key(b"component2", &v1));
        assert_ne!(k, key(b"component", &Version::new(0, 7, 1)));
    }

    #[test]
    fn prune_keeps_only_the_listed_keys() {
        let root = tempfile::tempdir().expect("temp dir");
        assert_eq!(prune(root.path(), &[]).expect("no folder yet"), 0);
        let dir = root.path().join(DIR);
        std::fs::create_dir_all(&dir).expect("mkdir");
        for name in ["aa.cwasm", "bb.cwasm", "notes.txt"] {
            std::fs::write(dir.join(name), b"x").expect("write");
        }
        assert_eq!(prune(root.path(), &["aa".to_owned()]).expect("prunes"), 1);
        assert!(dir.join("aa.cwasm").exists());
        assert!(!dir.join("bb.cwasm").exists());
        assert!(dir.join("notes.txt").exists());
    }

    #[test]
    fn unusable_file_is_a_miss_and_is_removed() {
        let root = tempfile::tempdir().expect("temp dir");
        let file = path(root.path(), "broken");
        std::fs::create_dir_all(file.parent().expect("dir")).expect("mkdir");
        std::fs::write(&file, b"not a compiled component").expect("write");
        let engine = Engine::default();
        assert!(load(&engine, &file).is_none());
        assert!(!file.exists());
    }
}
