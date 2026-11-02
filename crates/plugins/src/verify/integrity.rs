//! Re-hash of installed plugins once per Mumcord version (FR-6.52).
//!
//! Installed files differ from the package bytes (SVG assets are sanitised on install), so the
//! check hashes the installed version folder against a fingerprint recorded right after install.
//! Fingerprints live in `<plugins_dir>/integrity.json` together with the app version that last
//! checked them.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Shown on the plugin row; the store records the plugin as disabled with reason `tampered`.
pub const TAMPERED_TEXT: &str = "Files of this plugin were changed";
/// Development builds only: pretend to be this Mumcord version, so tests can trigger the re-hash
/// of a "newer build" (AC-6.21).
pub const TEST_APP_VERSION_ENV: &str = "MUMCORD_PLUGIN_TEST_APP_VERSION";

const FILE_NAME: &str = "integrity.json";

/// The Mumcord version the re-hash compares against: `real`, or in development builds the value
/// of [`TEST_APP_VERSION_ENV`] if set.
pub fn app_version(real: &str) -> String {
    #[cfg(debug_assertions)]
    if let Ok(fake) = std::env::var(TEST_APP_VERSION_ENV) {
        return fake;
    }
    real.to_owned()
}

/// SHA-256 over every entry below `dir`, sorted by relative path: each file contributes its path
/// and content hash, so added, removed, renamed and changed files all change the fingerprint.
/// Anything that is not a plain file or folder (e.g. a symlink planted later) counts as a change
/// too.
pub fn fingerprint(dir: &Path) -> io::Result<String> {
    let mut entries = BTreeMap::new();
    collect(dir, "", &mut entries)?;
    let mut hasher = Sha256::new();
    for (path, hash) in &entries {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(hash);
    }
    Ok(hex(&hasher.finalize()))
}

fn collect(dir: &Path, prefix: &str, entries: &mut BTreeMap<String, Vec<u8>>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}{name}");
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(&entry.path(), &format!("{path}/"), entries)?;
        } else if kind.is_file() {
            entries.insert(path, Sha256::digest(std::fs::read(entry.path())?).to_vec());
        } else {
            entries.insert(path, b"not a regular file".to_vec());
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The recorded fingerprints of installed plugin versions.
#[derive(Debug)]
pub struct Integrity {
    path: PathBuf,
    state: State,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// Mumcord version that last re-hashed the installed plugins.
    checked_version: Option<String>,
    /// `<id>@<version>` → fingerprint.
    fingerprints: BTreeMap<String, String>,
}

fn key(id: &str, version: &str) -> String {
    format!("{id}@{version}")
}

impl Integrity {
    /// Loads `<plugins_dir>/integrity.json`; a missing file means nothing is recorded yet.
    pub fn load(plugins_dir: &Path) -> io::Result<Self> {
        let path = plugins_dir.join(FILE_NAME);
        let state = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e),
        };
        Ok(Self { path, state })
    }

    /// Records the fingerprint of a freshly installed version folder.
    pub fn record(&mut self, id: &str, version: &str, dir: &Path) -> io::Result<()> {
        self.state
            .fingerprints
            .insert(key(id, version), fingerprint(dir)?);
        self.save()
    }

    /// Drops the fingerprint of a version folder that was deleted.
    pub fn forget(&mut self, id: &str, version: &str) -> io::Result<()> {
        if self.state.fingerprints.remove(&key(id, version)).is_some() {
            self.save()?;
        }
        Ok(())
    }

    /// At first start of a new Mumcord version (see [`app_version`]): re-hashes each installed
    /// `(id, version, folder)` and returns the ids whose files changed. The caller disables them
    /// with [`TAMPERED_TEXT`]. Same version as last time: does nothing. A version without a
    /// recorded fingerprint (installed before fingerprints existed) is recorded now.
    pub fn recheck(
        &mut self,
        app_version: &str,
        installed: &[(&str, &str, &Path)],
    ) -> io::Result<Vec<String>> {
        if self.state.checked_version.as_deref() == Some(app_version) {
            return Ok(Vec::new());
        }
        let mut tampered = Vec::new();
        for &(id, version, dir) in installed {
            // A missing or unreadable folder is a change as well.
            let now = fingerprint(dir).ok();
            match self.state.fingerprints.get(&key(id, version)) {
                Some(recorded) if now.as_ref() == Some(recorded) => {}
                Some(_) => tampered.push(id.to_owned()),
                None => match now {
                    Some(now) => {
                        self.state.fingerprints.insert(key(id, version), now);
                    }
                    None => tampered.push(id.to_owned()),
                },
            }
        }
        self.state.checked_version = Some(app_version.to_owned());
        self.save()?;
        Ok(tampered)
    }

    /// Written to a temporary file and renamed, so a crash never leaves a half-written file.
    fn save(&self) -> io::Result<()> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let tmp = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer_pretty(&tmp, &self.state).map_err(io::Error::other)?;
        tmp.persist(&self.path).map_err(|e| e.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(root: &Path) -> PathBuf {
        let dir = root.join("dice/1.0.0");
        std::fs::create_dir_all(dir.join("assets")).expect("test setup");
        std::fs::write(dir.join("plugin.wasm"), b"\0asm").expect("test setup");
        std::fs::write(dir.join("assets/icon.svg"), b"<svg/>").expect("test setup");
        dir
    }

    #[test]
    fn fingerprint_changes_with_any_byte_or_file() {
        let root = tempfile::tempdir().expect("test setup");
        let dir = plugin(root.path());
        let before = fingerprint(&dir).expect("test setup");
        assert_eq!(before, fingerprint(&dir).expect("test setup"));

        std::fs::write(dir.join("assets/icon.svg"), b"<svg/ >").expect("test setup");
        let changed = fingerprint(&dir).expect("test setup");
        assert_ne!(before, changed);

        std::fs::write(dir.join("extra"), b"").expect("test setup");
        assert_ne!(changed, fingerprint(&dir).expect("test setup"));
    }

    #[test]
    fn rehash_runs_once_per_app_version_and_flags_changed_files() {
        let root = tempfile::tempdir().expect("test setup");
        let dir = plugin(root.path());
        let mut integrity = Integrity::load(root.path()).expect("test setup");
        integrity.record("dice", "1.0.0", &dir).expect("test setup");
        let installed = [("dice", "1.0.0", dir.as_path())];
        assert!(
            integrity
                .recheck("0.5.0", &installed)
                .expect("test setup")
                .is_empty()
        );

        std::fs::write(dir.join("plugin.wasm"), b"\0asn").expect("test setup");
        // Same version again: no re-hash, also not after a reload.
        let mut integrity = Integrity::load(root.path()).expect("test setup");
        assert!(
            integrity
                .recheck("0.5.0", &installed)
                .expect("test setup")
                .is_empty()
        );
        // Newer build: the changed byte is found.
        assert_eq!(
            integrity.recheck("0.6.0", &installed).expect("test setup"),
            ["dice"]
        );
    }

    #[test]
    fn unknown_versions_are_recorded_and_missing_folders_flagged() {
        let root = tempfile::tempdir().expect("test setup");
        let dir = plugin(root.path());
        let gone = root.path().join("gone/1.0.0");
        let mut integrity = Integrity::load(root.path()).expect("test setup");
        let installed = [
            ("dice", "1.0.0", dir.as_path()),
            ("gone", "1.0.0", gone.as_path()),
        ];
        assert_eq!(
            integrity.recheck("0.5.0", &installed).expect("test setup"),
            ["gone"]
        );

        std::fs::write(dir.join("plugin.wasm"), b"changed").expect("test setup");
        assert_eq!(
            integrity.recheck("0.6.0", &installed).expect("test setup"),
            ["dice", "gone"]
        );

        integrity.forget("dice", "1.0.0").expect("test setup");
        let reloaded = Integrity::load(root.path()).expect("test setup");
        assert!(!reloaded.state.fingerprints.contains_key("dice@1.0.0"));
    }
}
