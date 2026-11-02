//! The registry source in `Mumcord/plugins` (FR-6.54): `plugins/<id>/plugin.toml`,
//! `plugins/<id>/versions/<ver>.toml` and `revoked.toml`. Documented in
//! `docs/plugin-registry-format.md`, section "Registry source".

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest;
use mumcord_plugins::registry::Revocation;
use serde::{Deserialize, Serialize};

use crate::date;

pub const PLUGINS_DIR: &str = "plugins";
pub const PLUGIN_FILE: &str = "plugin.toml";
pub const VERSIONS_DIR: &str = "versions";
pub const REVOKED_FILE: &str = "revoked.toml";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginFile {
    /// Language tag → name; `en` required.
    pub name: BTreeMap<String, String>,
    /// Language tag → one-line summary; `en` required.
    pub summary: BTreeMap<String, String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    /// Minisign public keys (base64 lines) allowed to sign this plugin's packages.
    pub author_keys: Vec<String>,
    /// Set by a maintainer after review.
    #[serde(default)]
    pub reviewed: bool,
    /// Where the source code lives (`http(s)` URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// SPDX licence expression, e.g. `MIT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Lowest Mumcord version the plugin needs, e.g. `0.7`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_mumcord: Option<String>,
    /// Day a maintainer reviewed the plugin, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_on: Option<String>,
    /// Day the plugin was added to the index, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_on: Option<String>,
    /// Markdown file next to plugin.toml, published as the long description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Image file next to plugin.toml.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<ScreenshotFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotFile {
    pub file: String,
    pub alt: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionFile {
    pub version: String,
    pub api: u32,
    pub size: u64,
    pub sha256: String,
    pub permissions: Vec<String>,
    /// `YYYY-MM-DD`.
    pub published: String,
    /// The author's detached minisign signature over the package.
    pub signature: String,
    pub source: SourceRef,
}

/// Where the package is rebuilt from for the reproducibility check (FR-6.56).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub repository: String,
    pub commit: String,
    /// Folder of the plugin project inside the repository ("" for the root).
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokedFile {
    #[serde(default)]
    revoked: Vec<Revocation>,
}

pub struct SourcePlugin {
    pub id: String,
    pub dir: PathBuf,
    pub plugin: PluginFile,
    pub versions: Vec<VersionFile>,
}

pub fn plugin_dir(repo: &Path, id: &str) -> PathBuf {
    repo.join(PLUGINS_DIR).join(id)
}

pub fn version_path(repo: &Path, id: &str, version: &str) -> PathBuf {
    plugin_dir(repo, id)
        .join(VERSIONS_DIR)
        .join(format!("{version}.toml"))
}

pub fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("{} is invalid", path.display()))
}

/// Reads every plugin of a registry checkout, sorted by id, and checks the files.
pub fn load_plugins(repo: &Path) -> Result<Vec<SourcePlugin>> {
    let root = repo.join(PLUGINS_DIR);
    let mut ids: Vec<String> = fs::read_dir(&root)
        .with_context(|| format!("{} has no {PLUGINS_DIR}/ folder", repo.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    ids.sort();
    ids.into_iter().map(|id| load_plugin(repo, id)).collect()
}

fn load_plugin(repo: &Path, id: String) -> Result<SourcePlugin> {
    if !manifest::is_valid_id(&id) {
        bail!("plugins/{id}: not a valid plugin id");
    }
    let dir = plugin_dir(repo, &id);
    let plugin: PluginFile = read_toml(&dir.join(PLUGIN_FILE))?;
    for file in plugin
        .description
        .iter()
        .chain(&plugin.icon)
        .chain(plugin.screenshots.iter().map(|s| &s.file))
    {
        manifest::relative_path(file)
            .with_context(|| format!("plugins/{id}: bad file path {file}"))?;
    }
    let mut versions = Vec::new();
    let versions_dir = dir.join(VERSIONS_DIR);
    if versions_dir.is_dir() {
        for entry in fs::read_dir(&versions_dir)? {
            let path = entry?.path();
            if path.extension().is_none_or(|x| x != "toml") {
                continue;
            }
            let version: VersionFile = read_toml(&path)?;
            if path.file_stem().and_then(|s| s.to_str()) != Some(version.version.as_str()) {
                bail!("{}: the file name must be the version", path.display());
            }
            if !date::is_date(&version.published) {
                bail!("{}: published must be YYYY-MM-DD", path.display());
            }
            versions.push(version);
        }
    }
    if versions.is_empty() {
        bail!("plugins/{id}: no versions");
    }
    Ok(SourcePlugin {
        id,
        dir,
        plugin,
        versions,
    })
}

pub fn load_revoked(repo: &Path) -> Result<Vec<Revocation>> {
    let path = repo.join(REVOKED_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(read_toml::<RevokedFile>(&path)?.revoked)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn write_plugin(repo: &Path, id: &str, versions: &[&str]) {
        let dir = plugin_dir(repo, id);
        fs::create_dir_all(dir.join(VERSIONS_DIR)).expect("test step");
        let plugin = PluginFile {
            name: BTreeMap::from([("en".into(), "Dice".into()), ("de".into(), "Würfel".into())]),
            summary: BTreeMap::from([("en".into(), "Roll dice".into())]),
            categories: vec!["games".into()],
            authors: vec!["Mumcord".into()],
            author_keys: vec!["RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3".into()],
            reviewed: true,
            source_url: Some("https://github.com/Mumcord/plugins".into()),
            license: Some("MIT".into()),
            min_mumcord: Some("0.7".into()),
            reviewed_on: Some("2026-10-20".into()),
            added_on: Some("2026-09-01".into()),
            description: Some("README.md".into()),
            icon: Some("icon-128.png".into()),
            screenshots: vec![ScreenshotFile {
                file: "shot-1.png".into(),
                alt: BTreeMap::from([("en".into(), "Two dice".into())]),
            }],
        };
        fs::write(
            dir.join(PLUGIN_FILE),
            toml::to_string(&plugin).expect("test step"),
        )
        .expect("test step");
        for name in ["README.md", "icon-128.png", "shot-1.png"] {
            fs::write(dir.join(name), name).expect("test step");
        }
        for version in versions {
            let file = VersionFile {
                version: (*version).into(),
                api: 1,
                size: 100,
                sha256: "ab".repeat(32),
                permissions: vec!["chat.send".into()],
                published: format!(
                    "2026-10-0{}",
                    versions.iter().position(|v| v == version).unwrap_or(0) + 1
                ),
                signature: "untrusted comment: x\nRW\ntrusted comment: y\nZZ\n".into(),
                source: SourceRef {
                    repository: "https://github.com/Mumcord/plugins".into(),
                    commit: "0".repeat(40),
                    path: id.into(),
                },
            };
            fs::write(
                version_path(repo, id, version),
                toml::to_string(&file).expect("test step"),
            )
            .expect("test step");
        }
    }

    #[test]
    fn loads_and_checks_a_registry_checkout() {
        let repo = tempfile::tempdir().expect("test step");
        write_plugin(repo.path(), "dice", &["1.0.0", "1.2.0"]);
        let plugins = load_plugins(repo.path()).expect("test step");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].versions.len(), 2);

        fs::write(
            version_path(repo.path(), "dice", "9.9.9"),
            fs::read(version_path(repo.path(), "dice", "1.0.0")).expect("test step"),
        )
        .expect("test step");
        assert!(
            load_plugins(repo.path()).is_err(),
            "file name must match the version"
        );
    }

    #[test]
    fn reads_revoked_toml() {
        let repo = tempfile::tempdir().expect("test step");
        assert!(load_revoked(repo.path()).expect("test step").is_empty());
        fs::write(repo.path().join(REVOKED_FILE), "# Empty for now.\n").expect("test step");
        assert!(load_revoked(repo.path()).expect("test step").is_empty());
        fs::write(
            repo.path().join(REVOKED_FILE),
            "[[revoked]]\nid = \"dice\"\nversions = [\"1.0.0\"]\nseverity = \"disable\"\nsince = \"2026-10-20\"\nreason = { en = \"Broken\" }\n",
        )
        .expect("test step");
        assert_eq!(load_revoked(repo.path()).expect("test step")[0].id, "dice");
    }
}
