//! Shared pieces for writing registry files (wire format: `docs/plugin-registry-format.md`), used
//! by `serve-registry`, `site-data` and `publish-pr`.
//!
//! The registry publishes two indexes (FR-9.14): `v1/index.json` in format 1 for 0.6.x clients,
//! listing only API 1.0 versions, and `v1.1/index.json` in format 2 listing every version.

use std::io::{Cursor, Read};

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest::Manifest;
use mumcord_plugins::package::{Digest, MANIFEST_FILE};
use mumcord_plugins::permissions::{CATALOGUE, Risk};
use mumcord_plugins::registry::{
    FORMAT_VERSION, FORMAT_VERSION_2, IndexPlugin, IndexVersion, KeyRotation, MAX_ICON_BYTES,
    RepositoryInfo, Revocation,
};
use mumcord_plugins::version::{self, ApiMinor, Release};
use serde::Serialize;
use serde_json::json;

/// A package file with the facts a registry lists about it.
pub struct PackageInfo {
    pub bytes: Vec<u8>,
    pub manifest: Manifest,
    pub sha256: String,
    /// The store icon: a PNG from the package (the client refuses SVG store images).
    pub icon_png: Option<Vec<u8>>,
}

impl PackageInfo {
    /// Reads only the manifest, without the source-dependent install rules, so a fixture registry
    /// can also serve packages the client is expected to refuse.
    pub fn read(bytes: Vec<u8>) -> Result<Self> {
        let mut archive =
            zip::ZipArchive::new(Cursor::new(&bytes)).context("the package is not a zip file")?;
        let mut manifest = Vec::new();
        archive
            .by_name(MANIFEST_FILE)
            .context("the package has no manifest.json")?
            .read_to_end(&mut manifest)?;
        let manifest = Manifest::parse(&manifest).context("manifest.json is invalid")?;
        let sha256 = Digest::of(&bytes).sha256_hex();
        let icon_png = icon_png(&mut archive, &manifest);
        Ok(Self {
            bytes,
            manifest,
            sha256,
            icon_png,
        })
    }

    pub fn file_name(&self) -> String {
        package_file_name(&self.manifest.id, &self.manifest.version.to_string())
    }

    pub fn permissions(&self) -> Vec<String> {
        self.manifest.all_permissions().map(str::to_owned).collect()
    }

    /// The origins the manifest declares; the format-2 index lists them for the score.
    pub fn origins(&self) -> Vec<String> {
        origins_of(&self.manifest)
    }

    pub fn listed(&self, url: String, published: String) -> ListedVersion {
        ListedVersion {
            version: self.manifest.version.to_string(),
            api: version::api_of(&self.manifest),
            url,
            size: self.bytes.len() as u64,
            sha256: self.sha256.clone(),
            permissions: self.permissions(),
            published,
            min_mumcord: None,
            origins: self.origins(),
            notes_url: None,
        }
    }
}

pub fn origins_of(manifest: &Manifest) -> Vec<String> {
    manifest
        .net
        .iter()
        .flat_map(|net| &net.origins)
        .map(|o| o.origin.clone())
        .collect()
}

/// One version as the registry knows it; written into both indexes.
#[derive(Debug, Clone)]
pub struct ListedVersion {
    pub version: String,
    pub api: ApiMinor,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub permissions: Vec<String>,
    /// `YYYY-MM-DD`; the newest listed version sets the plugin's `updated`.
    pub published: String,
    pub min_mumcord: Option<String>,
    pub origins: Vec<String>,
    pub notes_url: Option<String>,
}

impl ListedVersion {
    fn format_1(&self) -> Result<IndexVersion1> {
        Ok(IndexVersion1 {
            version: self.version.clone(),
            api: u32::try_from(self.api.major)?,
            url: self.url.clone(),
            size: self.size,
            sha256: self.sha256.clone(),
            permissions: self.permissions.clone(),
        })
    }

    /// `modules` and `reproducible` are left out: the registry has no verified value for them
    /// yet.
    fn format_2(&self) -> IndexVersion {
        IndexVersion {
            version: self.version.clone(),
            api: self.api,
            url: self.url.clone(),
            size: self.size,
            sha256: self.sha256.clone(),
            permissions: self.permissions.clone(),
            min_mumcord: self.min_mumcord.clone(),
            origins: self.origins.clone(),
            notes_url: self.notes_url.clone(),
            modules: None,
            reproducible: None,
        }
    }
}

/// A format-1 version entry, frozen for 0.6.x clients: `api` is the integer major. The client
/// reads it into [`IndexVersion`].
#[derive(Serialize)]
struct IndexVersion1 {
    version: String,
    api: u32,
    url: String,
    size: u64,
    sha256: String,
    permissions: Vec<String>,
}

/// Rejects a version whose `min_mumcord` is below the first release supporting its API
/// (FR-9.20). Only major.minor count (FR-9.13).
pub fn check_min_mumcord(api: ApiMinor, min_mumcord: Option<&str>) -> Result<()> {
    let first = version::first_release(api).with_context(|| format!("API {api} is not known"))?;
    let Some(text) = min_mumcord else {
        return Ok(());
    };
    let release = Release::parse(text).with_context(|| format!("min_mumcord {text} is invalid"))?;
    if release < first {
        bail!(
            "min_mumcord {text} is lower than {}.{}, the first release with API {api}",
            first.major,
            first.minor
        );
    }
    Ok(())
}

/// The manifest icon as PNG: the icon itself, or for an SVG (or WebP) the PNG next to it
/// (`icon.svg` → `icon-128.png`, `icon.png`, `icon-512.png`). No rasteriser: without a PNG the
/// plugin simply has no store icon. Files over the client's size cap are skipped.
fn icon_png(
    archive: &mut zip::ZipArchive<Cursor<&Vec<u8>>>,
    manifest: &Manifest,
) -> Option<Vec<u8>> {
    let icon = manifest.icon.as_deref()?;
    let stem = icon.rsplit_once('.').map_or(icon, |(stem, _)| stem);
    let candidates = [
        icon.to_owned(),
        format!("{stem}-128.png"),
        format!("{stem}.png"),
        format!("{stem}-512.png"),
    ];
    candidates
        .iter()
        .filter(|name| name.ends_with(".png"))
        .find_map(|name| {
            let mut file = archive.by_name(name).ok()?;
            let mut body = Vec::new();
            file.by_ref()
                .take(MAX_ICON_BYTES as u64 + 1)
                .read_to_end(&mut body)
                .ok()?;
            (body.len() <= MAX_ICON_BYTES && body.starts_with(b"\x89PNG")).then_some(body)
        })
}

pub fn package_file_name(id: &str, version: &str) -> String {
    format!("{id}-{version}.mcplugin")
}

#[derive(Serialize)]
struct IndexFile<'a, P> {
    format: u32,
    sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<&'a RepositoryInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_rotation: Option<&'a KeyRotation>,
    plugins: Vec<P>,
}

/// A plugin with its versions; `plugin.versions` and `plugin.updated` are filled per index.
pub struct ListedPlugin {
    pub plugin: IndexPlugin,
    pub versions: Vec<ListedVersion>,
}

/// The header fields both index files share.
pub struct IndexHead<'a> {
    pub sequence: u64,
    pub repository: Option<&'a RepositoryInfo>,
    pub key_rotation: Option<&'a KeyRotation>,
}

/// `v1/index.json`: format 1 with only API 1.0 versions; a plugin without one is left out
/// (FR-9.14).
pub fn index_v1_json(head: &IndexHead<'_>, plugins: &[ListedPlugin]) -> Result<Vec<u8>> {
    let mut listed = Vec::new();
    for entry in plugins {
        let versions = sorted(entry.versions.iter().filter(|v| v.api == ApiMinor::V1_0));
        let Some(newest) = versions.last() else {
            continue;
        };
        let mut plugin = entry.plugin.clone();
        plugin.updated = newest.published.clone();
        plugin.versions = Vec::new();
        let mut value = serde_json::to_value(&plugin)?;
        let format_1 = versions
            .iter()
            .map(|v| v.format_1())
            .collect::<Result<Vec<_>>>()?;
        value["versions"] = serde_json::to_value(format_1)?;
        listed.push(value);
    }
    index_file(FORMAT_VERSION, head, listed)
}

/// `v1.1/index.json`: format 2 with every version (FR-9.14, FR-9.15).
pub fn index_v1_1_json(head: &IndexHead<'_>, plugins: &[ListedPlugin]) -> Result<Vec<u8>> {
    let mut listed = Vec::new();
    for entry in plugins {
        let versions = sorted(entry.versions.iter());
        let Some(newest) = versions.last() else {
            continue;
        };
        let mut plugin = entry.plugin.clone();
        plugin.updated = newest.published.clone();
        plugin.versions = versions.iter().map(|v| v.format_2()).collect();
        listed.push(plugin);
    }
    index_file(FORMAT_VERSION_2, head, listed)
}

/// Oldest first, like format 1 (the client takes the last entry as the newest).
fn sorted<'a>(versions: impl Iterator<Item = &'a ListedVersion>) -> Vec<&'a ListedVersion> {
    let mut versions: Vec<_> = versions.collect();
    versions.sort_by_cached_key(|v| semver::Version::parse(&v.version).ok());
    versions
}

fn index_file<P: Serialize>(format: u32, head: &IndexHead<'_>, plugins: Vec<P>) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(&IndexFile {
        format,
        sequence: head.sequence,
        repository: head.repository,
        key_rotation: head.key_rotation,
        plugins,
    })?)
}

pub fn revoked_json(sequence: u64, revoked: &[Revocation]) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(&json!({
        "format": FORMAT_VERSION,
        "sequence": sequence,
        "revoked": revoked,
    }))?)
}

/// `v1/permissions.json` (FR-6.80) and `v1.1/permissions.json` (FR-9.14): the shared permission
/// texts for the website, limited to the permissions the API minor knows.
pub fn permissions_json(api: ApiMinor) -> Result<Vec<u8>> {
    let permissions: serde_json::Map<String, serde_json::Value> = CATALOGUE
        .iter()
        .filter(|p| p.min_api <= (api.major, api.minor))
        .map(|p| {
            let risk = match p.risk {
                Risk::Green => "green",
                Risk::Yellow => "yellow",
                Risk::Red => "red",
            };
            let entry = json!({
                "risk": risk,
                "official_only": p.official_only,
                "text": { "en": p.text_en },
            });
            (p.id.to_owned(), entry)
        })
        .collect();
    Ok(serde_json::to_vec_pretty(&json!({
        "format": FORMAT_VERSION,
        "permissions": permissions,
    }))?)
}

#[cfg(test)]
pub mod tests {
    use std::io::Write;

    use mumcord_plugins::registry::{Index, IndexKind};

    use super::*;

    /// The PNG signature plus filler: enough for the registry, which never decodes images.
    pub const TEST_PNG: &[u8] = b"\x89PNG\r\n\x1a\n-test-icon";

    pub fn manifest(id: &str, version: &str, permissions: &[&str]) -> String {
        manifest_for_api(id, version, permissions, "^1.0")
    }

    pub fn manifest_for_api(id: &str, version: &str, permissions: &[&str], api: &str) -> String {
        json!({
            "manifest": 1,
            "id": id,
            "name": "Test plugin",
            "description": "A plugin for tests.",
            "version": version,
            "api": api,
            "authors": ["Tester"],
            "categories": ["fun"],
            "icon": "assets/icon.svg",
            "license": "Apache-2.0",
            "permissions": { "required": permissions },
        })
        .to_string()
    }

    /// A small package; the wasm bytes are not a real component (registries never load them).
    pub fn package(id: &str, version: &str, permissions: &[&str]) -> Vec<u8> {
        package_for_api(id, version, permissions, "^1.0")
    }

    pub fn package_for_api(id: &str, version: &str, permissions: &[&str], api: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let manifest = manifest_for_api(id, version, permissions, api);
        let files: [(&str, &[u8]); 7] = [
            (MANIFEST_FILE, manifest.as_bytes()),
            ("plugin.wasm", b"\0asm"),
            ("i18n/en.json", b"{}"),
            ("README.md", b"# Test"),
            ("LICENSE", b"Apache-2.0"),
            (
                "assets/icon.svg",
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            ),
            ("assets/icon-128.png", TEST_PNG),
        ];
        for (name, body) in files {
            zip.start_file(name, options).expect("test step");
            zip.write_all(body).expect("test step");
        }
        zip.finish().expect("test step").into_inner()
    }

    #[test]
    fn reads_package_facts() {
        let info = PackageInfo::read(package("dice", "1.2.0", &["chat.send"])).expect("test step");
        assert_eq!(info.file_name(), "dice-1.2.0.mcplugin");
        let version = info.listed("https://x/dice.mcplugin".into(), "2026-11-03".into());
        assert_eq!(version.api, ApiMinor::V1_0);
        assert_eq!(version.format_1().expect("test step").api, 1);
        assert_eq!(version.permissions, ["chat.send"]);
        assert_eq!(version.sha256.len(), 64);
        assert!(version.origins.is_empty());
        assert_eq!(info.icon_png.as_deref(), Some(TEST_PNG));
    }

    #[test]
    fn permissions_json_lists_the_catalogue_of_the_api() {
        let list = |api| {
            let value: serde_json::Value =
                serde_json::from_slice(&permissions_json(api).expect("test step"))
                    .expect("test step");
            value["permissions"].as_object().expect("object").clone()
        };
        let full = list(ApiMinor::V1_1);
        assert_eq!(full.len(), CATALOGUE.len());
        assert_eq!(full["net.http"]["official_only"], true);
        assert!(full.contains_key("ui.stage"));
        let old = list(ApiMinor::V1_0);
        assert!(old.contains_key("net.http"));
        assert!(!old.contains_key("ui.stage"));
        assert!(old.len() < full.len());
    }

    fn version(v: &str, api: ApiMinor, published: &str) -> ListedVersion {
        ListedVersion {
            version: v.into(),
            api,
            url: format!("https://x/{v}"),
            size: 1,
            sha256: "ab".repeat(32),
            permissions: Vec::new(),
            published: published.into(),
            min_mumcord: (api == ApiMinor::V1_1).then(|| "0.7".into()),
            origins: vec!["https://api.example".into()],
            notes_url: None,
        }
    }

    fn listed(id: &str, versions: Vec<ListedVersion>) -> ListedPlugin {
        let index = json!({
            "id": id, "name": {"en": id}, "summary": {"en": id},
            "author_keys": ["RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3"],
            "updated": "2000-01-01", "versions": [],
        });
        ListedPlugin {
            plugin: serde_json::from_value(index).expect("test step"),
            versions,
        }
    }

    #[test]
    fn v1_lists_only_api_1_0_versions_and_v1_1_lists_all() {
        let plugins = [
            listed(
                "dice",
                vec![
                    version("1.1.0", ApiMinor::V1_1, "2026-11-02"),
                    version("1.0.0", ApiMinor::V1_0, "2026-10-01"),
                ],
            ),
            listed(
                "stage",
                vec![version("1.0.0", ApiMinor::V1_1, "2026-11-01")],
            ),
        ];
        let head = IndexHead {
            sequence: 5,
            repository: None,
            key_rotation: None,
        };
        let v1 = index_v1_json(&head, &plugins).expect("test step");
        let index = Index::parse(&v1, &IndexKind::Official).expect("0.6 clients read it");
        assert!(index.ignored.is_empty(), "{:?}", index.ignored);
        assert_eq!(index.plugins.len(), 1, "a 1.1-only plugin is left out");
        let dice = index.plugin("dice").expect("listed");
        assert_eq!(dice.versions.len(), 1);
        assert_eq!(dice.latest().version, "1.0.0");
        assert_eq!(dice.updated, "2026-10-01");

        let v2: serde_json::Value =
            serde_json::from_slice(&index_v1_1_json(&head, &plugins).expect("test step"))
                .expect("json");
        assert_eq!(v2["format"], 2);
        assert_eq!(v2["sequence"], 5);
        let dice = &v2["plugins"][0];
        assert_eq!(dice["updated"], "2026-11-02");
        assert_eq!(dice["versions"][0]["api"], "1.0");
        assert!(dice["versions"][0].get("min_mumcord").is_none());
        assert_eq!(dice["versions"][1]["version"], "1.1.0");
        assert_eq!(dice["versions"][1]["api"], "1.1");
        assert_eq!(dice["versions"][1]["min_mumcord"], "0.7");
        assert_eq!(dice["versions"][1]["origins"][0], "https://api.example");
        assert_eq!(v2["plugins"][1]["id"], "stage");
    }

    #[test]
    fn min_mumcord_must_not_undercut_the_api() {
        assert!(check_min_mumcord(ApiMinor::V1_1, None).is_ok());
        assert!(check_min_mumcord(ApiMinor::V1_1, Some("0.7")).is_ok());
        assert!(check_min_mumcord(ApiMinor::V1_1, Some("0.8.2")).is_ok());
        assert!(check_min_mumcord(ApiMinor::V1_0, Some("0.6")).is_ok());
        let error = check_min_mumcord(ApiMinor::V1_1, Some("0.6.9")).expect_err("too low");
        assert!(
            error.to_string().contains("first release with API 1.1"),
            "{error}"
        );
        assert!(check_min_mumcord(ApiMinor::V1_1, Some("seven")).is_err());
        assert!(check_min_mumcord(ApiMinor { major: 2, minor: 0 }, None).is_err());
    }
}
