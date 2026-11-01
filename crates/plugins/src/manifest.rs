//! `manifest.json` (FR-6.11): who a plugin is, which API it needs, what it asks for and how it is
//! shown. The manifest comes from a stranger, so parsing is strict and every value is capped; the
//! JSON schema in `schema/manifest.schema.json` describes the same rules for the CLI and the
//! registry CI.

use std::collections::BTreeSet;

use semver::{Op, Version, VersionReq};
use serde::Deserialize;

use crate::permissions;

/// The only manifest format version this host reads.
pub const MANIFEST_FORMAT: u32 = 1;
/// Larger manifests are refused before parsing.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
/// Plugins may lower their memory limit in the manifest, never raise it above this.
pub const MAX_MEMORY_MIB: u32 = 32;
/// Storage quota bounds (FR-6.20).
pub const DEFAULT_STORAGE_QUOTA_KIB: u32 = 256;
pub const MAX_STORAGE_QUOTA_KIB: u32 = 1024;
/// Plugin bytes per message after Mumcord's 3-byte envelope in a 1000-byte plugin data packet
/// (FR-6.34).
pub const MAX_MESSAGE_PAYLOAD: u32 = 997;
/// Ids starting with these are kept for plugins published by the Mumcord team.
pub const RESERVED_ID_PREFIXES: &[&str] = &["mumcord-", "core-"];
/// Exact user text when a non-official package asks for an official-only permission (FR-6.12).
pub const OFFICIAL_ONLY_TEXT: &str = "This permission is only available to official plugins";

const MAX_NAME_CHARS: usize = 32;
const MAX_DESCRIPTION_CHARS: usize = 140;
const MAX_SHORT_TEXT_CHARS: usize = 64;
const MAX_PATH_CHARS: usize = 128;
const MAX_AUTHORS: usize = 8;
const MAX_CATEGORIES: usize = 3;

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

/// Where a package or plugin folder comes from. Decides which permissions and ids it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// From the official index with valid signatures (FR-6.51).
    Official,
    /// From a third-party repository (store `source` = `repo:<repository_id>`).
    Repo { repository_id: i64 },
    /// An unpacked folder loaded in Developer mode.
    Dev,
}

/// A manifest that passed the structural checks of [`Manifest::parse`]. Source-dependent rules
/// are checked separately by [`Manifest::validate`].
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Version,
    /// Range of host API versions (`mumcord:plugin@x.y.z`) the plugin works with.
    pub api: VersionReq,
    /// Optional range of Mumcord app versions, for plugins that depend on a bug fix.
    pub mumcord: Option<VersionReq>,
    pub authors: Vec<String>,
    /// SPDX licence expression.
    pub license: String,
    pub categories: Vec<String>,
    pub permissions: Permissions,
    /// Memory limit in MiB, at most [`MAX_MEMORY_MIB`].
    pub memory_mib: u32,
    pub storage_quota_kib: u32,
    pub messaging: Messaging,
    pub commands: Vec<Command>,
    pub net: Option<Net>,
    /// Path of the icon under `assets/`.
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Permissions {
    pub required: Vec<String>,
    pub optional: Vec<OptionalPermission>,
}

/// An optional permission; `suggested` ones start switched on in the install prompt (FR-6.29).
#[derive(Debug, Clone, PartialEq)]
pub struct OptionalPermission {
    pub id: String,
    pub suggested: bool,
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
    /// Preselected in the install prompt (FR-6.33).
    pub default_visibility: Option<Visibility>,
}

/// A slash command (FR-6.43).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub usage: Option<String>,
}

/// `net` (FR-6.37): which hosts `net.http` may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Net {
    pub hosts: NetHosts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum NetHosts {
    /// Only hosts the user typed into a form of the plugin.
    #[serde(rename = "user-configured")]
    UserConfigured,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("The manifest is larger than 16 KiB")]
    TooLarge,
    #[error("The manifest is not valid: {0}")]
    Syntax(String),
    #[error("Manifest format {0} is not supported (this Mumcord reads format {MANIFEST_FORMAT})")]
    UnsupportedFormat(u32),
    #[error(
        "The plugin id \"{0}\" is not valid (3 to 32 lowercase letters, digits or dashes, starting with a letter)"
    )]
    BadId(String),
    #[error("The plugin id \"{0}\" uses a prefix reserved for Mumcord")]
    ReservedId(String),
    #[error("{field} is longer than {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{0} is empty or contains control characters")]
    BadText(&'static str),
    #[error("{field} is not a valid {kind}: \"{value}\"")]
    BadValue {
        field: &'static str,
        kind: &'static str,
        value: String,
    },
    #[error("The API range \"{0}\" must have the form ^MAJOR.MINOR, e.g. ^1.0")]
    BadApiRange(String),
    #[error("The category \"{0}\" is not one of the known categories")]
    UnknownCategory(String),
    #[error("Unknown permission \"{0}\"")]
    UnknownPermission(String),
    #[error("The permission \"{0}\" is listed twice")]
    DuplicatePermission(String),
    #[error("{OFFICIAL_ONLY_TEXT}")]
    OfficialOnly(String),
    #[error("{field} needs the permission \"{permission}\"")]
    MissingPermission {
        field: &'static str,
        permission: &'static str,
    },
    #[error("The command \"{0}\" is declared twice")]
    DuplicateCommand(String),
    #[error("The path \"{0}\" is not a safe relative path inside the package")]
    BadPath(String),
    #[error("{field} must be between {min} and {max}")]
    OutOfRange {
        field: &'static str,
        min: u32,
        max: u32,
    },
}

/// The manifest as written; [`Manifest::parse`] turns it into a [`Manifest`]. Unknown fields are
/// ignored (FR-6.12) so newer manifests still load on older hosts.
#[derive(Deserialize)]
struct RawManifest {
    manifest: u32,
    id: String,
    name: String,
    description: String,
    version: String,
    api: String,
    mumcord: Option<String>,
    authors: Vec<String>,
    license: String,
    categories: Vec<String>,
    #[serde(default)]
    permissions: RawPermissions,
    #[serde(default)]
    limits: RawLimits,
    #[serde(default)]
    storage: RawStorage,
    #[serde(default)]
    messaging: RawMessaging,
    #[serde(default)]
    commands: Vec<Command>,
    net: Option<Net>,
    icon: Option<String>,
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

#[derive(Deserialize, Default)]
struct RawLimits {
    memory_mib: Option<u32>,
}

#[derive(Deserialize, Default)]
struct RawStorage {
    quota_kib: Option<u32>,
}

#[derive(Deserialize, Default)]
struct RawMessaging {
    max_payload: Option<u32>,
    default_visibility: Option<Visibility>,
}

impl Manifest {
    /// Parses `manifest.json` and checks everything that does not depend on where it came from.
    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::TooLarge);
        }
        let raw: RawManifest =
            serde_json::from_slice(bytes).map_err(|e| ManifestError::Syntax(e.to_string()))?;
        raw.check()
    }

    /// The rules that depend on the source: only official plugins may use official-only
    /// permissions (FR-6.12, FR-6.73) and the reserved id prefixes (FR-6.11).
    pub fn validate(&self, source: Source) -> Result<(), ManifestError> {
        if source == Source::Official {
            return Ok(());
        }
        if RESERVED_ID_PREFIXES
            .iter()
            .any(|prefix| self.id.starts_with(prefix))
        {
            return Err(ManifestError::ReservedId(self.id.clone()));
        }
        match self
            .all_permissions()
            .find(|id| permissions::get(id).is_some_and(|p| p.official_only))
        {
            Some(id) => Err(ManifestError::OfficialOnly(id.to_owned())),
            None => Ok(()),
        }
    }

    /// Every permission the manifest names, required first.
    pub fn all_permissions(&self) -> impl Iterator<Item = &str> {
        self.permissions
            .required
            .iter()
            .map(String::as_str)
            .chain(self.permissions.optional.iter().map(|p| p.id.as_str()))
    }

    pub fn has_permission(&self, id: &str) -> bool {
        self.all_permissions().any(|p| p == id)
    }
}

impl RawManifest {
    fn check(self) -> Result<Manifest, ManifestError> {
        if self.manifest != MANIFEST_FORMAT {
            return Err(ManifestError::UnsupportedFormat(self.manifest));
        }
        if !is_valid_id(&self.id) {
            return Err(ManifestError::BadId(self.id));
        }
        text("name", &self.name, MAX_NAME_CHARS)?;
        text("description", &self.description, MAX_DESCRIPTION_CHARS)?;
        count("authors", self.authors.len(), MAX_AUTHORS)?;
        for author in &self.authors {
            text("authors", author, MAX_SHORT_TEXT_CHARS)?;
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

        count("categories", self.categories.len(), MAX_CATEGORIES)?;
        let mut seen = BTreeSet::new();
        for category in &self.categories {
            if !CATEGORIES.contains(&category.as_str()) || !seen.insert(category) {
                return Err(ManifestError::UnknownCategory(category.clone()));
            }
        }

        let permissions = check_permissions(self.permissions)?;
        let has = |id: &str| {
            permissions.required.iter().any(|p| p == id)
                || permissions.optional.iter().any(|p| p.id == id)
        };

        let mut names = BTreeSet::new();
        for command in &self.commands {
            if !is_command_name(&command.name) {
                return Err(bad_value(
                    "commands.name",
                    "command name (2 to 16 of a-z, 0-9, -)",
                    &command.name,
                ));
            }
            if !names.insert(command.name.as_str()) {
                return Err(ManifestError::DuplicateCommand(command.name.clone()));
            }
            text(
                "commands.description",
                &command.description,
                MAX_DESCRIPTION_CHARS,
            )?;
            if let Some(usage) = &command.usage {
                text("commands.usage", usage, MAX_SHORT_TEXT_CHARS)?;
            }
        }
        if !self.commands.is_empty() && !has("commands") {
            return Err(ManifestError::MissingPermission {
                field: "commands",
                permission: "commands",
            });
        }
        if self.net.is_some() != has("net.http") {
            return Err(ManifestError::MissingPermission {
                field: if self.net.is_some() {
                    "net"
                } else {
                    "net.http"
                },
                permission: if self.net.is_some() {
                    "net.http"
                } else {
                    "net"
                },
            });
        }

        if let Some(icon) = &self.icon {
            asset_path(icon)?;
        }

        let memory_mib = in_range(
            "limits.memory_mib",
            self.limits.memory_mib.unwrap_or(MAX_MEMORY_MIB),
            1,
            MAX_MEMORY_MIB,
        )?;
        let storage_quota_kib = in_range(
            "storage.quota_kib",
            self.storage.quota_kib.unwrap_or(DEFAULT_STORAGE_QUOTA_KIB),
            1,
            MAX_STORAGE_QUOTA_KIB,
        )?;
        let messaging = Messaging {
            max_payload: in_range(
                "messaging.max_payload",
                self.messaging.max_payload.unwrap_or(MAX_MESSAGE_PAYLOAD),
                1,
                MAX_MESSAGE_PAYLOAD,
            )?,
            default_visibility: self.messaging.default_visibility,
        };

        Ok(Manifest {
            id: self.id,
            name: self.name,
            description: self.description,
            version,
            api,
            mumcord,
            authors: self.authors,
            license: self.license,
            categories: self.categories,
            permissions,
            memory_mib,
            storage_quota_kib,
            messaging,
            commands: self.commands,
            net: self.net,
            icon: self.icon,
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

/// `^[a-z0-9-]{2,16}$` (FR-6.43)
fn is_command_name(name: &str) -> bool {
    (2..=16).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
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

/// The icon: under `assets/`, a raster or SVG image.
fn asset_path(path: &str) -> Result<(), ManifestError> {
    relative_path(path)?;
    let image = [".png", ".webp", ".svg"]
        .iter()
        .any(|ext| path.ends_with(ext));
    if path.starts_with("assets/") && image {
        Ok(())
    } else {
        Err(bad_value(
            "icon",
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

fn check_permissions(raw: RawPermissions) -> Result<Permissions, ManifestError> {
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
        if permissions::get(id).is_none() {
            return Err(ManifestError::UnknownPermission(
                id.chars().take(MAX_SHORT_TEXT_CHARS).collect(),
            ));
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

/// Lists that must have at least one and at most `max` entries.
fn count(field: &'static str, len: usize, max: usize) -> Result<(), ManifestError> {
    if (1..=max).contains(&len) {
        Ok(())
    } else {
        Err(ManifestError::OutOfRange {
            field,
            min: 1,
            max: max as u32,
        })
    }
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
        value: value.chars().take(MAX_SHORT_TEXT_CHARS).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest using every field.
    pub(crate) const EXAMPLE: &str = r#"{
      "manifest": 1,
      "id": "echolings",
      "name": "Echolings",
      "description": "Virtual pets that grow while you hang out in voice.",
      "version": "1.2.0",
      "api": "^1.0",
      "mumcord": ">=0.7.0",
      "authors": ["Mumcord team"],
      "license": "AGPL-3.0-only",
      "categories": ["fun", "games"],
      "icon": "assets/icon.svg",
      "permissions": {
        "required": ["ui.panel", "ui.member-badge", "ui.profile-card", "events.voice", "events.presence", "events.self", "commands"],
        "optional": ["peers.messages", "ui.chat-card", "notify", { "id": "events.chat-activity", "suggested": true }]
      },
      "limits": { "memory_mib": 16 },
      "storage": { "quota_kib": 512 },
      "messaging": { "max_payload": 256, "default_visibility": "friends" },
      "commands": [{ "name": "pet", "description": "Pet your Echoling", "usage": "/pet [name]" }]
    }"#;

    fn with(change: impl FnOnce(&mut serde_json::Value)) -> Result<Manifest, ManifestError> {
        let mut value: serde_json::Value = serde_json::from_str(EXAMPLE).expect("example is JSON");
        change(&mut value);
        Manifest::parse(value.to_string().as_bytes())
    }

    fn remove(v: &mut serde_json::Value, field: &str) {
        v.as_object_mut().expect("object").remove(field);
    }

    #[test]
    fn accepts_the_example() {
        let m = Manifest::parse(EXAMPLE.as_bytes()).expect("example is valid");
        assert_eq!(m.id, "echolings");
        assert_eq!(m.version, Version::new(1, 2, 0));
        assert_eq!(m.memory_mib, 16);
        assert_eq!(m.storage_quota_kib, 512);
        assert_eq!(m.authors, ["Mumcord team"]);
        assert_eq!(m.categories, ["fun", "games"]);
        assert_eq!(m.icon.as_deref(), Some("assets/icon.svg"));
        assert_eq!(m.commands[0].usage.as_deref(), Some("/pet [name]"));
        assert_eq!(m.messaging.default_visibility, Some(Visibility::Friends));
        assert_eq!(m.all_permissions().count(), 11);
        assert!(m.permissions.optional[3].suggested);
        assert!(!m.permissions.optional[0].suggested);
    }

    #[test]
    fn ignores_unknown_fields_and_fills_defaults() {
        let m = with(|v| {
            v["future_field"] = serde_json::json!({ "anything": true });
            for field in ["storage", "limits", "messaging", "icon", "mumcord"] {
                remove(v, field);
            }
        })
        .expect("valid");
        assert_eq!(m.memory_mib, MAX_MEMORY_MIB);
        assert_eq!(m.storage_quota_kib, DEFAULT_STORAGE_QUOTA_KIB);
        assert_eq!(m.messaging.max_payload, MAX_MESSAGE_PAYLOAD);
        assert_eq!(m.messaging.default_visibility, None);
    }

    #[test]
    fn requires_the_spec_fields() {
        for field in [
            "manifest",
            "id",
            "name",
            "description",
            "version",
            "api",
            "authors",
            "license",
            "categories",
        ] {
            assert!(
                matches!(with(|v| remove(v, field)), Err(ManifestError::Syntax(_))),
                "{field}"
            );
        }
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
    fn unknown_permission_names_the_id() {
        let err = with(|v| v["permissions"]["required"] = serde_json::json!(["network"]))
            .expect_err("unknown");
        assert_eq!(err, ManifestError::UnknownPermission("network".into()));
        assert_eq!(err.to_string(), "Unknown permission \"network\"");
        assert_eq!(
            with(|v| v["permissions"]["optional"] = serde_json::json!(["ui.panel"])),
            Err(ManifestError::DuplicatePermission("ui.panel".into()))
        );
    }

    #[test]
    fn official_only_permissions_need_an_official_source() {
        let m = with(|v| {
            v["permissions"]["optional"] = serde_json::json!(["ui.webview", "secrets"]);
        })
        .expect("valid");
        assert!(m.validate(Source::Official).is_ok());
        for source in [Source::Repo { repository_id: 1 }, Source::Dev] {
            let err = m.validate(source).expect_err("refused");
            assert_eq!(err, ManifestError::OfficialOnly("ui.webview".into()));
            assert_eq!(
                err.to_string(),
                "This permission is only available to official plugins"
            );
        }
        let plain = Manifest::parse(EXAMPLE.as_bytes()).expect("valid");
        assert!(plain.validate(Source::Dev).is_ok());
    }

    #[test]
    fn reserved_prefixes_are_official_only() {
        for id in ["mumcord-pets", "core-tools"] {
            let m = with(|v| v["id"] = id.into()).expect("valid");
            assert!(m.validate(Source::Official).is_ok());
            assert_eq!(
                m.validate(Source::Repo { repository_id: 3 }),
                Err(ManifestError::ReservedId(id.into()))
            );
            assert!(m.validate(Source::Dev).is_err());
        }
    }

    #[test]
    fn commands_are_checked() {
        for name in ["x", "a-very-long-command", "Roll", "ro ll", "/roll"] {
            assert!(
                with(|v| v["commands"][0]["name"] = name.into()).is_err(),
                "{name}"
            );
        }
        assert!(matches!(
            with(|v| {
                let c = v["commands"][0].clone();
                v["commands"].as_array_mut().expect("array").push(c);
            }),
            Err(ManifestError::DuplicateCommand(_))
        ));
        assert!(with(|v| remove(&mut v["commands"][0], "usage")).is_ok());
        assert!(with(|v| v["commands"][0]["description"] = "".into()).is_err());
        assert_eq!(
            with(|v| v["permissions"]["required"] = serde_json::json!(["ui.panel"])),
            Err(ManifestError::MissingPermission {
                field: "commands",
                permission: "commands"
            })
        );
    }

    #[test]
    fn net_and_net_http_go_together() {
        let net =
            |v: &mut serde_json::Value| v["net"] = serde_json::json!({"hosts": "user-configured"});
        let perm = |v: &mut serde_json::Value| {
            v["permissions"]["optional"] = serde_json::json!(["net.http"]);
        };
        let m = with(|v| {
            net(v);
            perm(v);
        })
        .expect("valid");
        assert_eq!(
            m.net,
            Some(Net {
                hosts: NetHosts::UserConfigured
            })
        );
        assert!(matches!(
            with(net),
            Err(ManifestError::MissingPermission { .. })
        ));
        assert!(matches!(
            with(perm),
            Err(ManifestError::MissingPermission { .. })
        ));
        assert!(matches!(
            with(|v| {
                perm(v);
                v["net"] = serde_json::json!({"hosts": "any"});
            }),
            Err(ManifestError::Syntax(_))
        ));
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
            with(|v| v["categories"] = serde_json::json!(["malware"])),
            Err(ManifestError::UnknownCategory(_))
        ));
        assert!(with(|v| v["categories"] = serde_json::json!([])).is_err());
        assert!(with(|v| v["categories"] = serde_json::json!(["fun", "fun"])).is_err());
        assert!(
            with(|v| v["categories"] = serde_json::json!(["fun", "games", "chat", "voice"]))
                .is_err()
        );
        assert!(with(|v| v["authors"] = serde_json::json!([])).is_err());
        assert!(with(|v| v["authors"] = serde_json::json!([""])).is_err());
        assert!(with(|v| v["authors"] = serde_json::json!([{"name": "x"}])).is_err());
        assert!(matches!(
            with(|v| v["manifest"] = 2.into()),
            Err(ManifestError::UnsupportedFormat(2))
        ));
        for icon in ["icon.png", "assets/icon.gif", "assets/../icon.png"] {
            assert!(with(|v| v["icon"] = icon.into()).is_err(), "{icon}");
        }
        assert!(with(|v| v["license"] = "MIT OR".into()).is_err());
        assert!(with(|v| v["license"] = "Apache-2.0 WITH LLVM-exception".into()).is_ok());
        assert!(matches!(
            with(|v| v["limits"]["memory_mib"] = 64.into()),
            Err(ManifestError::OutOfRange { .. })
        ));
        assert!(with(|v| v["messaging"]["max_payload"] = 998.into()).is_err());
        assert!(with(|v| v["storage"]["quota_kib"] = 2048.into()).is_err());
        assert!(with(|v| v["messaging"]["default_visibility"] = "all".into()).is_err());
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
        let ids: Vec<&str> = permissions::CATALOGUE.iter().map(|p| p.id).collect();
        assert_eq!(strings(&schema["$defs"]["permission"]["enum"]), ids);
        let props = &schema["properties"];
        assert_eq!(strings(&props["categories"]["items"]["enum"]), CATEGORIES);
        assert_eq!(props["categories"]["maxItems"], MAX_CATEGORIES);
        assert_eq!(props["authors"]["maxItems"], MAX_AUTHORS);
        assert_eq!(props["name"]["maxLength"], MAX_NAME_CHARS);
        assert_eq!(props["description"]["maxLength"], MAX_DESCRIPTION_CHARS);
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
        for field in example.as_object().expect("object").keys() {
            assert!(props.get(field).is_some(), "{field} missing in schema");
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
