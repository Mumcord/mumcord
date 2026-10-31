//! `manifest.json`: who a plugin is, which API it needs, what it asks for and how it is shown
//! (#269). The manifest comes from a stranger, so parsing is strict and every value is capped;
//! the JSON schema in `schema/manifest.schema.json` describes the same rules for the CLI and the
//! registry CI.

use std::collections::{BTreeMap, BTreeSet};

use semver::{Op, Version, VersionReq};
use serde::Deserialize;

/// The only manifest format version this host reads.
pub const MANIFEST_FORMAT: u32 = 1;
/// Larger manifests are refused before parsing.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
/// Plugins may lower their memory limit in the manifest, never raise it above this.
pub const MAX_MEMORY_MIB: u32 = 32;
/// Storage quota bounds (#270).
pub const DEFAULT_STORAGE_QUOTA_KIB: u32 = 256;
pub const MAX_STORAGE_QUOTA_KIB: u32 = 1024;
/// Plugin bytes per message after Mumcord's 3-byte envelope in a 1000-byte plugin data packet (#271).
pub const MAX_MESSAGE_PAYLOAD: u32 = 997;

const MAX_NAME_CHARS: usize = 32;
const MAX_DESCRIPTION_CHARS: usize = 140;
const MAX_SHORT_TEXT_CHARS: usize = 64;
const MAX_URL_CHARS: usize = 256;
const MAX_PATH_CHARS: usize = 128;
const MAX_SCREENSHOTS: usize = 8;
const MAX_LANGUAGES: usize = 64;

/// Permission ids of API 1.0 (#275). An unknown permission makes the load fail, so a plugin never
/// runs without something it depends on.
pub const PERMISSIONS: &[&str] = &[
    "ui.panel",
    "ui.member-badge",
    "ui.profile-card",
    "ui.chat-card",
    "events.self",
    "events.voice",
    "events.presence",
    "events.chat-activity",
    "peers.messages",
    "notify",
];

/// Categories in the browse view.
pub const CATEGORIES: &[&str] = &[
    "fun",
    "games",
    "chat",
    "voice",
    "accessibility",
    "looks",
    "tools",
];

/// Icon sizes in pixels a manifest may provide.
pub const ICON_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256, 512];

/// A validated manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: Version,
    /// Range of host API versions (`mumcord:plugin@x.y.z`) the plugin works with.
    pub api: VersionReq,
    /// Optional range of Mumcord app versions, for plugins that depend on a bug fix.
    pub mumcord: Option<VersionReq>,
    pub author: Author,
    pub description: Option<String>,
    pub category: String,
    /// SPDX licence expression.
    pub license: String,
    pub source: Option<String>,
    /// Path of the WebAssembly component inside the package.
    pub entry: String,
    /// Icon paths by size in pixels.
    pub icons: BTreeMap<u32, String>,
    pub screenshots: Vec<String>,
    pub i18n: I18n,
    pub permissions: Permissions,
    pub storage: Storage,
    pub messaging: Option<Messaging>,
    /// Memory limit in MiB, at most [`MAX_MEMORY_MIB`].
    pub memory_mib: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Author {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct I18n {
    pub default: String,
    pub available: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Permissions {
    pub required: Vec<String>,
    pub optional: Vec<OptionalPermission>,
}

/// An optional permission; `suggested` ones start switched on in the install prompt (#275).
#[derive(Debug, Clone, PartialEq)]
pub struct OptionalPermission {
    pub id: String,
    pub suggested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageScope {
    /// Separate data per identity.
    Identity,
    /// One set of data for all identities.
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Storage {
    pub scope: StorageScope,
    pub quota_kib: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Everyone,
    Friends,
    Nobody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Messaging {
    pub max_payload: u32,
    pub default_visibility: Option<Visibility>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("the manifest is larger than {MAX_MANIFEST_BYTES} bytes")]
    TooLarge,
    #[error("the manifest is not valid: {0}")]
    Syntax(String),
    #[error("manifest format {0} is not supported (this Mumcord reads format {MANIFEST_FORMAT})")]
    UnsupportedFormat(u32),
    #[error(
        "the plugin id {0:?} is not valid (3 to 32 lowercase letters, digits or dashes, starting with a letter)"
    )]
    BadId(String),
    #[error("{field} is longer than {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{0} is empty or contains control characters")]
    BadText(&'static str),
    #[error("{field} is not a valid {kind}: {value:?}")]
    BadValue {
        field: &'static str,
        kind: &'static str,
        value: String,
    },
    #[error("the API range {0:?} must have the form ^MAJOR.MINOR, e.g. ^1.0")]
    BadApiRange(String),
    #[error("the category {0:?} is not one of the known categories")]
    UnknownCategory(String),
    #[error("the permission {0:?} is not known to this Mumcord")]
    UnknownPermission(String),
    #[error("the permission {0:?} is listed twice")]
    DuplicatePermission(String),
    #[error("the path {0:?} is not a safe relative path inside the package")]
    BadPath(String),
    #[error("the icon size {0:?} is not supported")]
    BadIconSize(String),
    #[error("the default language {0:?} is not in the list of available languages")]
    DefaultLanguageMissing(String),
    #[error("English (\"en\") must be one of the available languages")]
    EnglishMissing,
    #[error("{field} must be between {min} and {max}")]
    OutOfRange {
        field: &'static str,
        min: u32,
        max: u32,
    },
}

/// The manifest as written; [`Manifest::parse`] turns it into a validated [`Manifest`]. Unknown
/// fields are ignored so newer manifests still load on older hosts.
#[derive(Deserialize)]
struct RawManifest {
    manifest: u32,
    id: String,
    name: String,
    version: String,
    api: String,
    mumcord: Option<String>,
    author: Author,
    description: Option<String>,
    category: String,
    license: String,
    source: Option<String>,
    entry: String,
    #[serde(default)]
    icons: BTreeMap<String, String>,
    #[serde(default)]
    screenshots: Vec<String>,
    i18n: I18n,
    #[serde(default)]
    permissions: RawPermissions,
    storage: Option<RawStorage>,
    messaging: Option<RawMessaging>,
    #[serde(default)]
    limits: RawLimits,
}

#[derive(Deserialize, Default)]
struct RawPermissions {
    #[serde(default)]
    required: Vec<String>,
    #[serde(default)]
    optional: Vec<RawOptional>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawOptional {
    Id(String),
    Entry {
        id: String,
        #[serde(default)]
        suggested: bool,
    },
}

#[derive(Deserialize)]
struct RawStorage {
    scope: StorageScope,
    quota_kib: Option<u32>,
}

#[derive(Deserialize)]
struct RawMessaging {
    max_payload: u32,
    default_visibility: Option<Visibility>,
}

#[derive(Deserialize, Default)]
struct RawLimits {
    memory_mib: Option<u32>,
}

impl Manifest {
    /// Parses and validates `manifest.json`.
    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::TooLarge);
        }
        let raw: RawManifest =
            serde_json::from_slice(bytes).map_err(|e| ManifestError::Syntax(e.to_string()))?;
        raw.validate()
    }

    /// Every permission the manifest names, required first.
    pub fn all_permissions(&self) -> impl Iterator<Item = &str> {
        self.permissions
            .required
            .iter()
            .map(String::as_str)
            .chain(self.permissions.optional.iter().map(|p| p.id.as_str()))
    }
}

impl RawManifest {
    fn validate(self) -> Result<Manifest, ManifestError> {
        if self.manifest != MANIFEST_FORMAT {
            return Err(ManifestError::UnsupportedFormat(self.manifest));
        }
        if !is_valid_id(&self.id) {
            return Err(ManifestError::BadId(self.id));
        }
        text("name", &self.name, MAX_NAME_CHARS)?;
        if let Some(description) = &self.description {
            text("description", description, MAX_DESCRIPTION_CHARS)?;
        }
        text("author.name", &self.author.name, MAX_SHORT_TEXT_CHARS)?;
        if let Some(url) = &self.author.url {
            https_url("author.url", url)?;
        }
        if let Some(source) = &self.source {
            https_url("source", source)?;
        }
        text("license", &self.license, MAX_SHORT_TEXT_CHARS)?;
        if !is_spdx_expression(&self.license) {
            return Err(bad_value(
                "license",
                "SPDX licence expression",
                &self.license,
            ));
        }

        let version = Version::parse(&self.version)
            .map_err(|_| bad_value("version", "semantic version", &self.version))?;
        let api = parse_api_range(&self.api)?;
        let mumcord = self
            .mumcord
            .as_deref()
            .map(|r| VersionReq::parse(r).map_err(|_| bad_value("mumcord", "version range", r)))
            .transpose()?;

        if !CATEGORIES.contains(&self.category.as_str()) {
            return Err(ManifestError::UnknownCategory(self.category));
        }

        relative_path(&self.entry)?;
        if !self.entry.ends_with(".wasm") {
            return Err(bad_value("entry", "path to a .wasm file", &self.entry));
        }
        let mut icons = BTreeMap::new();
        for (size, path) in self.icons {
            let px = size
                .parse::<u32>()
                .ok()
                .filter(|px| ICON_SIZES.contains(px))
                .ok_or_else(|| ManifestError::BadIconSize(size.clone()))?;
            asset_path(&path)?;
            icons.insert(px, path);
        }
        if self.screenshots.len() > MAX_SCREENSHOTS {
            return Err(ManifestError::OutOfRange {
                field: "screenshots",
                min: 0,
                max: MAX_SCREENSHOTS as u32,
            });
        }
        for path in &self.screenshots {
            asset_path(path)?;
        }

        validate_i18n(&self.i18n)?;
        let permissions = validate_permissions(self.permissions)?;

        let storage = match self.storage {
            Some(s) => Storage {
                scope: s.scope,
                quota_kib: in_range(
                    "storage.quota_kib",
                    s.quota_kib.unwrap_or(DEFAULT_STORAGE_QUOTA_KIB),
                    1,
                    MAX_STORAGE_QUOTA_KIB,
                )?,
            },
            None => Storage {
                scope: StorageScope::Identity,
                quota_kib: DEFAULT_STORAGE_QUOTA_KIB,
            },
        };
        let messaging = self
            .messaging
            .map(|m| {
                Ok::<_, ManifestError>(Messaging {
                    max_payload: in_range(
                        "messaging.max_payload",
                        m.max_payload,
                        1,
                        MAX_MESSAGE_PAYLOAD,
                    )?,
                    default_visibility: m.default_visibility,
                })
            })
            .transpose()?;
        let memory_mib = in_range(
            "limits.memory_mib",
            self.limits.memory_mib.unwrap_or(MAX_MEMORY_MIB),
            1,
            MAX_MEMORY_MIB,
        )?;

        Ok(Manifest {
            id: self.id,
            name: self.name,
            version,
            api,
            mumcord,
            author: self.author,
            description: self.description,
            category: self.category,
            license: self.license,
            source: self.source,
            entry: self.entry,
            icons,
            screenshots: self.screenshots,
            i18n: self.i18n,
            permissions,
            storage,
            messaging,
            memory_mib,
        })
    }
}

/// `^[a-z][a-z0-9-]{2,31}$`
pub fn is_valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (3..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// A path inside the package: relative, `/`-separated, no `.` or `..` parts, a small safe
/// character set. The same rule guards zip entries, so nothing can land outside the plugin
/// directory on any platform.
pub fn relative_path(path: &str) -> Result<(), ManifestError> {
    let ok = !path.is_empty()
        && path.chars().count() <= MAX_PATH_CHARS
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if ok {
        Ok(())
    } else {
        Err(ManifestError::BadPath(path.to_owned()))
    }
}

/// Icons and screenshots: under `assets/`, a raster or SVG image.
fn asset_path(path: &str) -> Result<(), ManifestError> {
    relative_path(path)?;
    let image = [".png", ".webp", ".svg"]
        .iter()
        .any(|ext| path.ends_with(ext));
    if path.starts_with("assets/") && image {
        Ok(())
    } else {
        Err(bad_value(
            "icons/screenshots",
            "image under assets/ (.png, .webp, .svg)",
            path,
        ))
    }
}

/// `api` is a caret range on the host API: minor versions only add, so `^1.3` means "1.3 or any
/// newer 1.x" and a new major is a separate, explicit opt-in.
fn parse_api_range(range: &str) -> Result<VersionReq, ManifestError> {
    let bad = || ManifestError::BadApiRange(range.to_owned());
    // semver reads a bare "1.0" as a caret range too; the manifest spells it out.
    if !range.starts_with('^') {
        return Err(bad());
    }
    let req = VersionReq::parse(range).map_err(|_| bad())?;
    match req.comparators.as_slice() {
        [c] if c.op == Op::Caret && c.major >= 1 && c.minor.is_some() && c.pre.is_empty() => {
            Ok(req)
        }
        _ => Err(bad()),
    }
}

fn validate_i18n(i18n: &I18n) -> Result<(), ManifestError> {
    if i18n.available.len() > MAX_LANGUAGES {
        return Err(ManifestError::OutOfRange {
            field: "i18n.available",
            min: 1,
            max: MAX_LANGUAGES as u32,
        });
    }
    for tag in i18n.available.iter().chain(std::iter::once(&i18n.default)) {
        if !is_language_tag(tag) {
            return Err(bad_value("i18n", "language tag (e.g. en, de, pt-BR)", tag));
        }
    }
    if !i18n.available.contains(&i18n.default) {
        return Err(ManifestError::DefaultLanguageMissing(i18n.default.clone()));
    }
    if !i18n.available.iter().any(|tag| tag == "en") {
        return Err(ManifestError::EnglishMissing);
    }
    Ok(())
}

/// `en`, `de`, `pt-BR`: also the file name `i18n/<tag>.json`.
pub fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let language = parts.next().unwrap_or_default();
    let region = parts.next();
    (2..=3).contains(&language.len())
        && language.bytes().all(|b| b.is_ascii_lowercase())
        && region.is_none_or(|r| r.len() == 2 && r.bytes().all(|b| b.is_ascii_uppercase()))
        && parts.next().is_none()
}

fn validate_permissions(raw: RawPermissions) -> Result<Permissions, ManifestError> {
    let optional: Vec<OptionalPermission> = raw
        .optional
        .into_iter()
        .map(|p| match p {
            RawOptional::Id(id) => OptionalPermission {
                id,
                suggested: false,
            },
            RawOptional::Entry { id, suggested } => OptionalPermission { id, suggested },
        })
        .collect();
    let mut seen = BTreeSet::new();
    for id in raw.required.iter().chain(optional.iter().map(|p| &p.id)) {
        if !PERMISSIONS.contains(&id.as_str()) {
            return Err(ManifestError::UnknownPermission(id.clone()));
        }
        if !seen.insert(id.as_str()) {
            return Err(ManifestError::DuplicatePermission(id.clone()));
        }
    }
    Ok(Permissions {
        required: raw.required,
        optional,
    })
}

/// A loose but safe check of an SPDX expression such as `MIT`, `Apache-2.0 WITH LLVM-exception`
/// or `(MIT OR Apache-2.0)`. The registry CI checks the identifiers against the SPDX list.
fn is_spdx_expression(license: &str) -> bool {
    let tokens: Vec<&str> = license
        .split([' ', '(', ')'])
        .filter(|t| !t.is_empty())
        .collect();
    !tokens.is_empty()
        && tokens.iter().all(|t| {
            t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+' | b':'))
        })
        && !matches!(tokens.first(), Some(&("AND" | "OR" | "WITH")))
        && !matches!(tokens.last(), Some(&("AND" | "OR" | "WITH")))
}

fn text(field: &'static str, value: &str, max: usize) -> Result<(), ManifestError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(ManifestError::BadText(field));
    }
    if value.chars().count() > max {
        return Err(ManifestError::TooLong { field, max });
    }
    Ok(())
}

fn https_url(field: &'static str, url: &str) -> Result<(), ManifestError> {
    text(field, url, MAX_URL_CHARS)?;
    let host = url.strip_prefix("https://").unwrap_or_default();
    if host.is_empty() || host.starts_with('/') || url.contains(char::is_whitespace) {
        return Err(bad_value(field, "https URL", url));
    }
    Ok(())
}

fn in_range(field: &'static str, value: u32, min: u32, max: u32) -> Result<u32, ManifestError> {
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(ManifestError::OutOfRange { field, min, max })
    }
}

fn bad_value(field: &'static str, kind: &'static str, value: &str) -> ManifestError {
    ManifestError::BadValue {
        field,
        kind,
        value: value.chars().take(MAX_URL_CHARS).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example from #269.
    pub(crate) const EXAMPLE: &str = r#"{
      "manifest": 1,
      "id": "echolings",
      "name": "Echolings",
      "version": "1.2.0",
      "api": "^1.0",
      "mumcord": ">=0.7.0",
      "author": { "name": "Mumcord team", "url": "https://github.com/Mumcord" },
      "description": "Virtual pets that grow while you hang out in voice.",
      "category": "fun",
      "license": "MIT",
      "source": "https://github.com/Mumcord/echolings",
      "entry": "plugin.wasm",
      "icons": { "64": "assets/icon-64.png", "256": "assets/icon-256.png" },
      "screenshots": ["assets/shot-1.webp", "assets/shot-2.webp"],
      "i18n": { "default": "en", "available": ["en", "de", "es"] },
      "permissions": {
        "required": ["ui.panel", "ui.member-badge", "ui.profile-card", "events.voice", "events.presence", "events.self"],
        "optional": ["peers.messages", "ui.chat-card", "notify", "events.chat-activity"]
      },
      "storage": { "scope": "identity", "quota_kib": 512 },
      "messaging": { "max_payload": 256, "default_visibility": "friends" },
      "limits": { "memory_mib": 16 }
    }"#;

    fn with(change: impl FnOnce(&mut serde_json::Value)) -> Result<Manifest, ManifestError> {
        let mut value: serde_json::Value = serde_json::from_str(EXAMPLE).expect("example is JSON");
        change(&mut value);
        Manifest::parse(value.to_string().as_bytes())
    }

    #[test]
    fn accepts_the_example() {
        let m = Manifest::parse(EXAMPLE.as_bytes()).expect("example is valid");
        assert_eq!(m.id, "echolings");
        assert_eq!(m.version, Version::new(1, 2, 0));
        assert_eq!(m.memory_mib, 16);
        assert_eq!(
            m.icons.get(&64).map(String::as_str),
            Some("assets/icon-64.png")
        );
        assert_eq!(
            m.storage,
            Storage {
                scope: StorageScope::Identity,
                quota_kib: 512
            }
        );
        assert_eq!(m.all_permissions().count(), 10);
    }

    #[test]
    fn ignores_unknown_fields_and_fills_defaults() {
        let m = with(|v| {
            v["future_field"] = serde_json::json!({ "anything": true });
            let o = v.as_object_mut().expect("object");
            o.remove("storage");
            o.remove("limits");
            o.remove("messaging");
        })
        .expect("valid");
        assert_eq!(m.memory_mib, MAX_MEMORY_MIB);
        assert_eq!(m.storage.quota_kib, DEFAULT_STORAGE_QUOTA_KIB);
        assert_eq!(m.messaging, None);
    }

    #[test]
    fn reads_suggested_optional_permissions() {
        let m = with(|v| {
            v["permissions"]["optional"] =
                serde_json::json!(["notify", { "id": "peers.messages", "suggested": true }]);
        })
        .expect("valid");
        assert_eq!(
            m.permissions.optional,
            vec![
                OptionalPermission {
                    id: "notify".into(),
                    suggested: false
                },
                OptionalPermission {
                    id: "peers.messages".into(),
                    suggested: true
                },
            ]
        );
    }

    #[test]
    fn rejects_bad_ids() {
        for id in [
            "",
            "ab",
            "Echolings",
            "1pets",
            "pets_",
            "-pets",
            "a-very-long-plugin-id-that-is-too-long",
        ] {
            assert!(
                matches!(with(|v| v["id"] = id.into()), Err(ManifestError::BadId(_))),
                "{id}"
            );
        }
        assert!(with(|v| v["id"] = "dice-roller2".into()).is_ok());
    }

    #[test]
    fn rejects_oversize_and_control_text() {
        assert_eq!(
            with(|v| v["name"] = "x".repeat(33).into()),
            Err(ManifestError::TooLong {
                field: "name",
                max: 32
            })
        );
        assert_eq!(
            with(|v| v["description"] = "x".repeat(141).into()),
            Err(ManifestError::TooLong {
                field: "description",
                max: 140
            })
        );
        assert_eq!(
            with(|v| v["name"] = "Pets\u{7}".into()),
            Err(ManifestError::BadText("name"))
        );
        let big = format!("{{\"pad\": \"{}\"}}", "x".repeat(MAX_MANIFEST_BYTES));
        assert_eq!(
            Manifest::parse(big.as_bytes()),
            Err(ManifestError::TooLarge)
        );
    }

    #[test]
    fn rejects_unknown_and_duplicate_permissions() {
        assert_eq!(
            with(|v| v["permissions"]["required"] = serde_json::json!(["network"])),
            Err(ManifestError::UnknownPermission("network".into()))
        );
        assert_eq!(
            with(|v| v["permissions"]["optional"] = serde_json::json!(["ui.panel"])),
            Err(ManifestError::DuplicatePermission("ui.panel".into()))
        );
    }

    #[test]
    fn rejects_missing_or_unsafe_entry() {
        assert!(matches!(
            with(|v| {
                v.as_object_mut().expect("object").remove("entry");
            }),
            Err(ManifestError::Syntax(_))
        ));
        for entry in [
            "../plugin.wasm",
            "/plugin.wasm",
            "a//b.wasm",
            "C:\\x.wasm",
            "plugin.js",
        ] {
            assert!(with(|v| v["entry"] = entry.into()).is_err(), "{entry}");
        }
    }

    #[test]
    fn rejects_bad_versions_and_ranges() {
        assert!(with(|v| v["version"] = "1.2".into()).is_err());
        for api in [
            "1.0",
            ">=1.0",
            "*",
            "^0.3",
            "^1",
            "^1.0, <1.4",
            "^1.0.0-beta",
        ] {
            assert!(
                matches!(
                    with(|v| v["api"] = api.into()),
                    Err(ManifestError::BadApiRange(_))
                ),
                "{api}"
            );
        }
        assert!(with(|v| v["api"] = "^1.3.2".into()).is_ok());
        assert!(with(|v| v["mumcord"] = "not a range".into()).is_err());
    }

    #[test]
    fn rejects_other_bad_fields() {
        assert!(matches!(
            with(|v| v["category"] = "malware".into()),
            Err(ManifestError::UnknownCategory(_))
        ));
        assert!(matches!(
            with(|v| v["manifest"] = 2.into()),
            Err(ManifestError::UnsupportedFormat(2))
        ));
        assert!(matches!(
            with(|v| v["icons"] = serde_json::json!({"65": "assets/i.png"})),
            Err(ManifestError::BadIconSize(_))
        ));
        assert!(with(|v| v["icons"] = serde_json::json!({"64": "icon.png"})).is_err());
        assert!(with(|v| v["source"] = "http://example.com".into()).is_err());
        assert!(with(|v| v["license"] = "MIT OR".into()).is_err());
        assert!(with(|v| v["license"] = "Apache-2.0 WITH LLVM-exception".into()).is_ok());
        assert_eq!(
            with(|v| v["i18n"] = serde_json::json!({"default": "de", "available": ["de"]})),
            Err(ManifestError::EnglishMissing)
        );
        assert!(matches!(
            with(|v| v["i18n"]["default"] = "fr".into()),
            Err(ManifestError::DefaultLanguageMissing(_))
        ));
        assert!(matches!(
            with(|v| v["limits"]["memory_mib"] = 64.into()),
            Err(ManifestError::OutOfRange { .. })
        ));
        assert!(with(|v| v["messaging"]["max_payload"] = 998.into()).is_err());
        assert!(with(|v| v["storage"]["quota_kib"] = 2048.into()).is_err());
        assert!(matches!(
            with(|v| v["storage"]["scope"] = "server".into()),
            Err(ManifestError::Syntax(_))
        ));
    }

    /// The published schema must name the same permissions, categories and limits as the parser.
    #[test]
    fn json_schema_matches_the_parser() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../schema/manifest.schema.json"))
                .expect("schema is JSON");
        let strings = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .expect("array")
                .iter()
                .map(|s| s.as_str().expect("string").to_owned())
                .collect()
        };
        assert_eq!(strings(&schema["$defs"]["permission"]["enum"]), PERMISSIONS);
        assert_eq!(
            strings(&schema["properties"]["category"]["enum"]),
            CATEGORIES
        );
        let icon_sizes: Vec<String> = ICON_SIZES.iter().map(u32::to_string).collect();
        assert_eq!(
            strings(&schema["properties"]["icons"]["propertyNames"]["enum"]),
            icon_sizes
        );
        let props = &schema["properties"];
        assert_eq!(
            props["limits"]["properties"]["memory_mib"]["maximum"],
            MAX_MEMORY_MIB
        );
        assert_eq!(
            props["storage"]["properties"]["quota_kib"]["maximum"],
            MAX_STORAGE_QUOTA_KIB
        );
        assert_eq!(
            props["storage"]["properties"]["quota_kib"]["default"],
            DEFAULT_STORAGE_QUOTA_KIB
        );
        assert_eq!(
            props["messaging"]["properties"]["max_payload"]["maximum"],
            MAX_MESSAGE_PAYLOAD
        );
        assert_eq!(props["manifest"]["const"], MANIFEST_FORMAT);
        let example: serde_json::Value = serde_json::from_str(EXAMPLE).expect("example is JSON");
        for field in strings(&schema["required"]) {
            assert!(example.get(&field).is_some(), "{field}");
        }
    }

    #[test]
    fn language_tags() {
        for tag in ["en", "de", "pt-BR", "fil"] {
            assert!(is_language_tag(tag), "{tag}");
        }
        for tag in ["", "EN", "en-us", "en_US", "../x", "en-BR-x"] {
            assert!(!is_language_tag(tag), "{tag}");
        }
    }
}
