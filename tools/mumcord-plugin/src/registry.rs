//! Shared pieces for writing registry files (wire format: `docs/plugin-registry-format.md`), used
//! by `serve-registry`, `site-data` and `publish-pr`.

use std::io::{Cursor, Read};

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest::Manifest;
use mumcord_plugins::package::{Digest, MANIFEST_FILE};
use mumcord_plugins::permissions::{CATALOGUE, Risk};
use mumcord_plugins::registry::{
    FORMAT_VERSION, IndexPlugin, IndexVersion, KeyRotation, MAX_ICON_BYTES, RepositoryInfo,
    Revocation,
};
use semver::VersionReq;
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

    pub fn index_version(&self, url: String) -> Result<IndexVersion> {
        Ok(IndexVersion {
            version: self.manifest.version.to_string(),
            api: api_major(&self.manifest.api)?,
            url,
            size: self.bytes.len() as u64,
            sha256: self.sha256.clone(),
            permissions: self.permissions(),
        })
    }
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

/// The API major a manifest range targets (`^1.0` → 1); the index lists it per version.
pub fn api_major(range: &VersionReq) -> Result<u32> {
    match range.comparators.first() {
        Some(c) => Ok(u32::try_from(c.major)?),
        None => bail!("the api range names no version"),
    }
}

#[derive(Serialize)]
struct IndexFile<'a> {
    format: u32,
    sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<&'a RepositoryInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_rotation: Option<&'a KeyRotation>,
    plugins: &'a [IndexPlugin],
}

pub fn index_json(
    sequence: u64,
    repository: Option<&RepositoryInfo>,
    key_rotation: Option<&KeyRotation>,
    plugins: &[IndexPlugin],
) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(&IndexFile {
        format: FORMAT_VERSION,
        sequence,
        repository,
        key_rotation,
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

/// `v1/permissions.json` (FR-6.80): the shared permission texts for the website.
pub fn permissions_json() -> Result<Vec<u8>> {
    let permissions: serde_json::Map<String, serde_json::Value> = CATALOGUE
        .iter()
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

    use super::*;

    /// The PNG signature plus filler: enough for the registry, which never decodes images.
    pub const TEST_PNG: &[u8] = b"\x89PNG\r\n\x1a\n-test-icon";

    pub fn manifest(id: &str, version: &str, permissions: &[&str]) -> String {
        json!({
            "manifest": 1,
            "id": id,
            "name": "Test plugin",
            "description": "A plugin for tests.",
            "version": version,
            "api": "^1.0",
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
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let manifest = manifest(id, version, permissions);
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
        let version = info
            .index_version("https://x/dice.mcplugin".into())
            .expect("test step");
        assert_eq!(version.api, 1);
        assert_eq!(version.permissions, ["chat.send"]);
        assert_eq!(version.sha256.len(), 64);
        assert_eq!(info.icon_png.as_deref(), Some(TEST_PNG));
    }

    #[test]
    fn permissions_json_lists_the_catalogue() {
        let value: serde_json::Value =
            serde_json::from_slice(&permissions_json().expect("test step")).expect("test step");
        let permissions = value["permissions"].as_object().expect("object");
        assert_eq!(permissions.len(), CATALOGUE.len());
        assert_eq!(permissions["net.http"]["official_only"], true);
    }
}
