//! `manifest.json` (FR-6.11): who a plugin is, which API it needs, what it asks for and how it is
//! shown. The manifest comes from a stranger, so parsing is strict and every value is capped; the
//! JSON schema in `schema/manifest.schema.json` describes the same rules for the CLI and the
//! registry CI.

use std::collections::BTreeSet;

use semver::{Op, Version, VersionReq};
use serde::Deserialize;

use crate::diag::LoadIssue;
use crate::origins::{self, HardStop};
use crate::patterns::{MAX_CHAT_PATTERNS, MAX_PATTERN_DESCRIPTION_CHARS};
use crate::permissions;

/// The only manifest format version this host reads.
pub const MANIFEST_FORMAT: u32 = 1;
/// Larger manifests are refused before parsing.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
/// Plugins may lower their memory limit in the manifest, never raise it above raw.
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
/// Apps-menu items per plugin (FR-9.53).
pub const MAX_APP_MENU_ITEMS: usize = 2;
/// Longest Apps-menu label.
pub const MAX_LABEL_CHARS: usize = 32;
/// `stage.max_participants` (FR-9.23).
pub const MAX_STAGE_PARTICIPANTS: u32 = 16;
/// Sound files per package (FR-9.65).
pub const MAX_SOUNDS: usize = 32;
/// Folder of bundled sounds inside the package.
pub const SOUNDS_DIR: &str = "sounds/";
/// At most this many chat trigger words per plugin.
pub const MAX_TRIGGERS: usize = 4;

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
    /// Chat trigger words such as `!poll` (permission `chat.triggers`); see [`is_trigger`].
    pub triggers: Vec<String>,
    /// Chat patterns (permission `chat.patterns`); see [`crate::patterns`].
    pub chat_patterns: Vec<ChatPattern>,
    pub net: Option<Net>,
    /// Items for the Apps menu (permission `ui.app-menu`, API 1.1).
    pub app_menu: Vec<AppMenuItem>,
    /// Media-area activity (permission `ui.stage`, API 1.1).
    pub stage: Option<StageDecl>,
    /// Where the plugin's web content may be shown (permission `ui.webview`, API 1.1).
    pub webview: Option<WebviewDecl>,
    /// Sounds bundled in the package (permission `audio.effects`, API 1.1).
    pub audio: Option<AudioDecl>,
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

/// A chat pattern: a regular expression anchored at the message start, and what it recognises
/// in words the install prompt shows (e.g. "Recognise EVE Online fits pasted in chat").
/// Translations come from the plugin's `i18n/<lang>.json`, key `chat_patterns.<n>.description`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChatPattern {
    pub pattern: String,
    pub description: String,
}

/// `net` (FR-6.37, FR-9.23): which hosts `net.http` may reach and which websites `net.origins`
/// lists.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Net {
    pub hosts: Option<NetHosts>,
    #[serde(default)]
    pub origins: Vec<NetOrigin>,
}

/// A website the plugin declares (FR-9.23). The rules for the origin string are checked by the
/// manifest validation, not here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NetOrigin {
    pub origin: String,
    /// Translation key of the reason shown next to the origin.
    pub purpose: String,
}

/// An Apps-menu entry (FR-9.23).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AppMenuItem {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
}

/// The media-area activity of a plugin (FR-9.23).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StageDecl {
    pub title: String,
    /// At most 16; checked by the manifest validation.
    pub max_participants: Option<u32>,
}

/// Where web content may be shown (FR-9.23).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WebviewDecl {
    #[serde(default)]
    pub places: Vec<WebviewPlace>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WebviewPlace {
    Stage,
    SidePanel,
    Dialog,
    Panel,
}

/// Sound files bundled in the package (FR-9.23, FR-9.65).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AudioDecl {
    #[serde(default)]
    pub sounds: Vec<String>,
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
    #[error("The permission \"{permission}\" needs API {needs} but the manifest says ^{has}")]
    ApiTooOld {
        permission: String,
        needs: String,
        has: String,
    },
    /// An origin breaking FR-9.25/FR-9.26: not installable, no score (FR-9.78).
    #[error("Cannot be installed: {0}")]
    Origin(#[from] HardStop),
    #[error("The website \"{0}\" is declared twice")]
    DuplicateOrigin(String),
    #[error("The Apps-menu item \"{0}\" is declared twice")]
    DuplicateAppMenuItem(String),
    #[error(
        "The Apps-menu label \"{0}\" is reserved: it is a Mumcord label or starts with \"Mumcord\""
    )]
    ReservedLabel(String),
    #[error("The command \"{0}\" is declared twice")]
    DuplicateCommand(String),
    #[error("The trigger \"{0}\" is declared twice")]
    DuplicateTrigger(String),
    #[error("The chat pattern \"{pattern}\" is not allowed: {reason}")]
    BadChatPattern {
        pattern: String,
        reason: crate::patterns::PatternError,
    },
    #[error("The chat pattern \"{0}\" is declared twice")]
    DuplicateChatPattern(String),
    #[error("The path \"{0}\" is not a safe relative path inside the package")]
    BadPath(String),
    #[error("{field} must be between {min} and {max}")]
    OutOfRange {
        field: &'static str,
        min: u32,
        max: u32,
    },
}

impl ManifestError {
    /// The Diagnostics entry (FR-9.90) a load refusal leaves, if it is one of the listed kinds.
    pub fn diagnostic(&self) -> Option<LoadIssue> {
        match self {
            ManifestError::MissingPermission { field, permission } => {
                Some(LoadIssue::FieldWithoutPermission { field, permission })
            }
            ManifestError::ReservedLabel(_) => Some(LoadIssue::RefusedAppMenuLabel),
            _ => None,
        }
    }
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
    #[serde(default)]
    triggers: Vec<String>,
    #[serde(default)]
    chat_patterns: Vec<ChatPattern>,
    net: Option<Net>,
    #[serde(default)]
    app_menu: Vec<AppMenuItem>,
    stage: Option<StageDecl>,
    webview: Option<WebviewDecl>,
    audio: Option<AudioDecl>,
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
        self.validate_origins(source == Source::Dev)?;
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

    /// The `net.origins` rules (FR-9.25, FR-9.26): every origin valid, no duplicates, at most 8.
    /// Only a Developer-mode folder may declare `http://localhost:<port>`.
    pub fn validate_origins(&self, developer: bool) -> Result<Vec<origins::Origin>, ManifestError> {
        let declared = self.net.as_ref().map_or(&[][..], |n| n.origins.as_slice());
        if declared.len() > origins::MAX_ORIGINS {
            return Err(HardStop::TooMany.into());
        }
        let mut parsed: Vec<origins::Origin> = Vec::with_capacity(declared.len());
        for declared in declared {
            let origin = origins::validate(&declared.origin, developer)?;
            if parsed.contains(&origin) {
                return Err(ManifestError::DuplicateOrigin(origin.ascii()));
            }
            parsed.push(origin);
        }
        Ok(parsed)
    }

    /// Every permission the manifest names, required first.
    pub fn all_permissions(&self) -> impl Iterator<Item = &str> {
        self.permissions
            .required
            .iter()
            .map(String::as_str)
            .chain(self.permissions.optional.iter().map(|p| p.id.as_str()))
    }

    /// The API minor `(major, minor)` the package is built against: the lower bound of `api`
    /// (FR-9.6). `^1` and `^1.0` are 1.0, `^1.1` is 1.1.
    pub fn api_minor(&self) -> (u64, u64) {
        // parse_api_range guarantees exactly one caret comparator with a minor.
        self.api
            .comparators
            .first()
            .map_or((1, 0), |c| (c.major, c.minor.unwrap_or(0)))
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
        if self.triggers.len() > MAX_TRIGGERS {
            return Err(ManifestError::OutOfRange {
                field: "triggers",
                min: 0,
                max: MAX_TRIGGERS as u32,
            });
        }
        let mut triggers = BTreeSet::new();
        for trigger in &self.triggers {
            if !is_trigger(trigger) {
                return Err(bad_value(
                    "triggers",
                    "trigger word (! then 2 to 16 of a-z, 0-9, -, starting with a letter)",
                    trigger,
                ));
            }
            if !triggers.insert(trigger.as_str()) {
                return Err(ManifestError::DuplicateTrigger(trigger.clone()));
            }
        }
        if !self.triggers.is_empty() && !has("chat.triggers") {
            return Err(ManifestError::MissingPermission {
                field: "triggers",
                permission: "chat.triggers",
            });
        }
        if self.chat_patterns.len() > MAX_CHAT_PATTERNS {
            return Err(ManifestError::OutOfRange {
                field: "chat_patterns",
                min: 0,
                max: MAX_CHAT_PATTERNS as u32,
            });
        }
        let mut chat_patterns = BTreeSet::new();
        for entry in &self.chat_patterns {
            crate::patterns::compile(&entry.pattern).map_err(|reason| {
                ManifestError::BadChatPattern {
                    pattern: entry.pattern.chars().take(MAX_SHORT_TEXT_CHARS).collect(),
                    reason,
                }
            })?;
            if !chat_patterns.insert(entry.pattern.as_str()) {
                return Err(ManifestError::DuplicateChatPattern(
                    entry.pattern.chars().take(MAX_SHORT_TEXT_CHARS).collect(),
                ));
            }
            text(
                "chat_patterns.description",
                &entry.description,
                MAX_PATTERN_DESCRIPTION_CHARS,
            )?;
        }
        if !self.chat_patterns.is_empty() && !has("chat.patterns") {
            return Err(ManifestError::MissingPermission {
                field: "chat_patterns",
                permission: "chat.patterns",
            });
        }
        // `net.origins` (the field) is checked against its permission by the manifest
        // validation, not here.
        let net_hosts = self.net.as_ref().is_some_and(|n| n.hosts.is_some());
        if net_hosts != has("net.http") {
            return Err(ManifestError::MissingPermission {
                field: if net_hosts { "net" } else { "net.http" },
                permission: if net_hosts { "net.http" } else { "net" },
            });
        }

        let has_api_1_1_field = !self.app_menu.is_empty()
            || self.stage.is_some()
            || self.webview.is_some()
            || self.audio.is_some()
            || self.net.as_ref().is_some_and(|n| !n.origins.is_empty());
        let api_minor = api_minor_of(&api);
        for id in permissions
            .required
            .iter()
            .chain(permissions.optional.iter().map(|p| &p.id))
        {
            let min = permissions::get(id).map_or(permissions::API_1_0, |p| p.min_api);
            if min > api_minor {
                return Err(ManifestError::ApiTooOld {
                    permission: id.clone(),
                    needs: format!("{}.{}", min.0, min.1),
                    has: format!("{}.{}", api_minor.0, api_minor.1),
                });
            }
        }
        // The 1.1 fields exist only from API 1.1 on; an older host ignores them, so a 1.0
        // package using them is a mistake worth a clear error.
        if has_api_1_1_field && api_minor < permissions::API_1_1 {
            return Err(ManifestError::ApiTooOld {
                permission: "the 1.1 manifest fields".into(),
                needs: "1.1".into(),
                has: format!("{}.{}", api_minor.0, api_minor.1),
            });
        }
        check_api_1_1_fields(
            &Fields {
                net: &self.net,
                app_menu: &self.app_menu,
                stage: &self.stage,
                webview: &self.webview,
                audio: &self.audio,
            },
            &has,
        )?;

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
            triggers: self.triggers,
            chat_patterns: self.chat_patterns,
            net: self.net,
            app_menu: self.app_menu,
            stage: self.stage,
            webview: self.webview,
            audio: self.audio,
            icon: self.icon,
        })
    }
}

/// Borrowed view of the 1.1 fields (the raw manifest is partly moved by then).
struct Fields<'a> {
    net: &'a Option<Net>,
    app_menu: &'a [AppMenuItem],
    stage: &'a Option<StageDecl>,
    webview: &'a Option<WebviewDecl>,
    audio: &'a Option<AudioDecl>,
}

/// Each 1.1 field needs its permission (FR-9.23) and obeys its limits.
fn check_api_1_1_fields(
    fields: &Fields<'_>,
    has: &dyn Fn(&str) -> bool,
) -> Result<(), ManifestError> {
    let need = |field: &'static str, permission: &'static str| {
        if has(permission) {
            Ok(())
        } else {
            Err(ManifestError::MissingPermission { field, permission })
        }
    };

    if let Some(origins) = fields
        .net
        .as_ref()
        .map(|n| &n.origins)
        .filter(|o| !o.is_empty())
    {
        need("net.origins", "net.origins")?;
        for origin in origins {
            text("net.origins.purpose", &origin.purpose, MAX_SHORT_TEXT_CHARS)?;
        }
    }

    if !fields.app_menu.is_empty() {
        need("app_menu", "ui.app-menu")?;
        count("app_menu", fields.app_menu.len(), MAX_APP_MENU_ITEMS)?;
        let mut ids = BTreeSet::new();
        for item in fields.app_menu {
            if !is_item_id(&item.id) {
                return Err(bad_value(
                    "app_menu.id",
                    "item id (1 to 32 of a-z, 0-9, -)",
                    &item.id,
                ));
            }
            if !ids.insert(item.id.as_str()) {
                return Err(ManifestError::DuplicateAppMenuItem(item.id.clone()));
            }
            text("app_menu.label", &item.label, MAX_LABEL_CHARS)?;
            if is_reserved_label(&item.label) {
                return Err(ManifestError::ReservedLabel(item.label.clone()));
            }
            if let Some(icon) = &item.icon {
                asset_path(icon)?;
            }
        }
    }

    if let Some(stage) = &fields.stage {
        need("stage", "ui.stage")?;
        text("stage.title", &stage.title, MAX_NAME_CHARS)?;
        if let Some(max) = stage.max_participants {
            in_range("stage.max_participants", max, 1, MAX_STAGE_PARTICIPANTS)?;
        }
    }

    if let Some(webview) = &fields.webview {
        need("webview", "ui.webview")?;
        if webview.places.contains(&WebviewPlace::SidePanel) {
            need("webview.places", "ui.side-panel")?;
        }
    }

    if let Some(audio) = &fields.audio {
        need("audio", "audio.effects")?;
        if audio.sounds.len() > MAX_SOUNDS {
            return Err(ManifestError::OutOfRange {
                field: "audio.sounds",
                min: 0,
                max: MAX_SOUNDS as u32,
            });
        }
        let mut seen = BTreeSet::new();
        for sound in &audio.sounds {
            relative_path(sound)?;
            let kind_ok = [".ogg", ".wav"].iter().any(|ext| sound.ends_with(ext));
            if !sound.starts_with(SOUNDS_DIR) || !kind_ok || !seen.insert(sound.as_str()) {
                return Err(ManifestError::BadPath(sound.clone()));
            }
        }
    }
    Ok(())
}

/// The API minor a `^MAJOR.MINOR` range starts at; `parse_api_range` guarantees the shape.
fn api_minor_of(api: &VersionReq) -> (u64, u64) {
    api.comparators
        .first()
        .map_or(permissions::API_1_0, |c| (c.major, c.minor.unwrap_or(0)))
}

/// `[a-z0-9-]{1,32}`: an Apps-menu item id.
fn is_item_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Lower-case, no whitespace, no trailing ellipsis: how labels are compared (FR-9.54). Must match
/// `normalize` in `scripts/gen-builtin-labels.mjs`.
pub fn normalize_label(label: &str) -> String {
    let squashed: String = label
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    squashed
        .strip_suffix('…')
        .or_else(|| squashed.strip_suffix("..."))
        .unwrap_or(&squashed)
        .to_owned()
}

/// A label a plugin may not use in the Apps menu: equal to a built-in menu or palette label in
/// any of the 24 locales (case- and space-insensitive) or starting with "Mumcord" (FR-9.54).
pub fn is_reserved_label(label: &str) -> bool {
    let normalized = normalize_label(label);
    normalized.starts_with("mumcord") || builtin_labels().contains(&normalized)
}

static BUILTIN_LABELS_JSON: &str = include_str!("../assets/builtin-labels.json");

fn builtin_labels() -> &'static [String] {
    static LABELS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    LABELS.get_or_init(|| {
        serde_json::from_str(BUILTIN_LABELS_JSON).expect("builtin-labels.json is a string list")
    })
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

/// `^![a-z][a-z0-9-]{1,15}$`, e.g. `!poll`.
pub fn is_trigger(word: &str) -> bool {
    let Some(name) = word.strip_prefix('!') else {
        return false;
    };
    (2..=16).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
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

    /// An API 1.1 manifest that also names the given (required) permissions.
    fn with11(
        permissions: &[&str],
        change: impl FnOnce(&mut serde_json::Value),
    ) -> Result<Manifest, ManifestError> {
        with(|v| {
            v["api"] = "^1.1".into();
            for p in permissions {
                v["permissions"]["required"]
                    .as_array_mut()
                    .expect("required")
                    .push((*p).into());
            }
            change(v);
        })
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
            // ui.webview is open to every plugin since API 1.1; secrets stays official-only.
        })
        .expect("valid");
        assert!(m.validate(Source::Official).is_ok());
        for source in [Source::Repo { repository_id: 1 }, Source::Dev] {
            let err = m.validate(source).expect_err("refused");
            assert_eq!(err, ManifestError::OfficialOnly("secrets".into()));
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
    fn triggers_are_checked() {
        let grant = |v: &mut serde_json::Value| {
            v["permissions"]["optional"] = serde_json::json!(["chat.triggers"]);
        };
        let m = with(|v| {
            grant(v);
            v["triggers"] = serde_json::json!(["!poll", "!p2-x"]);
        })
        .expect("valid");
        assert_eq!(m.triggers, ["!poll", "!p2-x"]);
        assert!(with(|_| {}).expect("valid").triggers.is_empty());
        for word in [
            "poll",
            "!p",
            "!Poll",
            "!2poll",
            "!po ll",
            "!a-very-long-trigger",
            "!",
        ] {
            assert!(
                with(|v| {
                    grant(v);
                    v["triggers"] = serde_json::json!([word]);
                })
                .is_err(),
                "{word}"
            );
        }
        assert_eq!(
            with(|v| {
                grant(v);
                v["triggers"] = serde_json::json!(["!poll", "!poll"]);
            }),
            Err(ManifestError::DuplicateTrigger("!poll".into()))
        );
        assert!(matches!(
            with(|v| {
                grant(v);
                v["triggers"] = serde_json::json!(["!a1", "!a2", "!a3", "!a4", "!a5"]);
            }),
            Err(ManifestError::OutOfRange {
                field: "triggers",
                ..
            })
        ));
        assert_eq!(
            with(|v| v["triggers"] = serde_json::json!(["!poll"])),
            Err(ManifestError::MissingPermission {
                field: "triggers",
                permission: "chat.triggers"
            })
        );
    }

    #[test]
    fn chat_patterns_are_checked() {
        let eft = r"^\[[^,\]\n]{1,64},[^\]\n]{0,100}\]\n";
        let with_patterns = |patterns: serde_json::Value| {
            with(|v| {
                v["permissions"]["optional"] = serde_json::json!(["chat.patterns"]);
                v["chat_patterns"] = patterns;
            })
        };
        let m = with_patterns(serde_json::json!([
            {"pattern": eft, "description": "Recognise EVE Online fits pasted in chat"}
        ]))
        .expect("valid");
        assert_eq!(m.chat_patterns[0].pattern, eft);
        assert!(with(|_| {}).expect("valid").chat_patterns.is_empty());

        assert_eq!(
            with_patterns(serde_json::json!([{"pattern": "\\[Rifter", "description": "x"}])),
            Err(ManifestError::BadChatPattern {
                pattern: "\\[Rifter".into(),
                reason: crate::patterns::PatternError::NotAnchored
            })
        );
        assert!(matches!(
            with_patterns(serde_json::json!([{"pattern": "^\\w{1000}", "description": "x"}])),
            Err(ManifestError::BadChatPattern {
                reason: crate::patterns::PatternError::TooLarge,
                ..
            })
        ));
        assert_eq!(
            with_patterns(serde_json::json!([
                {"pattern": "^a", "description": "x"},
                {"pattern": "^a", "description": "y"}
            ])),
            Err(ManifestError::DuplicateChatPattern("^a".into()))
        );
        assert!(matches!(
            with_patterns(serde_json::json!([
                {"pattern": "^a", "description": "x"},
                {"pattern": "^b", "description": "x"},
                {"pattern": "^c", "description": "x"}
            ])),
            Err(ManifestError::OutOfRange {
                field: "chat_patterns",
                ..
            })
        ));
        for description in ["", "  ", "a\u{7}", &"a".repeat(81)] {
            assert!(
                with_patterns(serde_json::json!([{"pattern": "^a", "description": description}]))
                    .is_err(),
                "{description}"
            );
        }
        assert!(
            with_patterns(serde_json::json!([{"pattern": "^a"}])).is_err(),
            "description is required"
        );
        assert_eq!(
            with(
                |v| v["chat_patterns"] = serde_json::json!([{"pattern": "^a", "description": "x"}])
            ),
            Err(ManifestError::MissingPermission {
                field: "chat_patterns",
                permission: "chat.patterns"
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
                hosts: Some(NetHosts::UserConfigured),
                origins: Vec::new(),
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
    fn api_1_1_fields_parse() {
        let m = with11(&["net.origins", "ui.app-menu", "ui.stage", "ui.webview", "ui.side-panel", "audio.effects"], |v| {
            v["net"] = serde_json::json!({"origins": [{"origin": "https://api.example", "purpose": "origins.api"}]});
            v["app_menu"] = serde_json::json!([{"id": "play", "label": "Play", "icon": "assets/p.svg"}, {"id": "b", "label": "B"}]);
            v["stage"] = serde_json::json!({"title": "Game", "max_participants": 4});
            v["webview"] = serde_json::json!({"places": ["stage", "side-panel", "dialog", "panel"]});
            v["audio"] = serde_json::json!({"sounds": ["sounds/ding.ogg"]});
        })
        .expect("valid");
        assert_eq!(m.api_minor(), (1, 1));
        let net = m.net.expect("net");
        assert_eq!(net.hosts, None);
        assert_eq!(
            net.origins,
            [NetOrigin {
                origin: "https://api.example".into(),
                purpose: "origins.api".into()
            }]
        );
        assert_eq!(m.app_menu.len(), 2);
        assert_eq!(m.app_menu[1].icon, None);
        assert_eq!(m.stage.expect("stage").max_participants, Some(4));
        assert_eq!(
            m.webview.expect("webview").places,
            [
                WebviewPlace::Stage,
                WebviewPlace::SidePanel,
                WebviewPlace::Dialog,
                WebviewPlace::Panel
            ]
        );
        assert_eq!(m.audio.expect("audio").sounds, ["sounds/ding.ogg"]);
    }

    #[test]
    fn api_1_1_fields_are_optional_and_unknown_ones_ignored() {
        let m = with(|v| v["future"] = serde_json::json!({"x": 1})).expect("valid");
        assert!(m.app_menu.is_empty());
        assert!(m.stage.is_none() && m.webview.is_none() && m.audio.is_none());
        assert_eq!(m.api_minor(), (1, 0));
        for api in ["^1.0", "^1.0.0"] {
            assert_eq!(
                with(|v| v["api"] = api.into()).expect("valid").api_minor(),
                (1, 0)
            );
        }
        let unknown = with11(&["ui.stage"], |v| {
            v["stage"] = serde_json::json!({"title": "G", "later": true});
        });
        assert!(unknown.is_ok());
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

    fn missing(field: &'static str, permission: &'static str) -> Result<Manifest, ManifestError> {
        Err(ManifestError::MissingPermission { field, permission })
    }

    #[test]
    fn each_1_1_field_needs_its_permission() {
        let origins = serde_json::json!({"origins": [{"origin": "https://api.example", "purpose": "origins.api"}]});
        assert_eq!(
            with11(&[], |v| v["net"] = origins.clone()),
            missing("net.origins", "net.origins")
        );
        assert_eq!(
            with11(&[], |v| v["app_menu"] =
                serde_json::json!([{"id": "a", "label": "Play"}])),
            missing("app_menu", "ui.app-menu")
        );
        assert_eq!(
            with11(&[], |v| v["stage"] = serde_json::json!({"title": "G"})),
            missing("stage", "ui.stage")
        );
        assert_eq!(
            with11(&[], |v| v["webview"] =
                serde_json::json!({"places": ["dialog"]})),
            missing("webview", "ui.webview")
        );
        assert_eq!(
            with11(&["ui.webview"], |v| v["webview"] =
                serde_json::json!({"places": ["side-panel"]})),
            missing("webview.places", "ui.side-panel")
        );
        assert_eq!(
            with11(&[], |v| v["audio"] =
                serde_json::json!({"sounds": ["sounds/a.wav"]})),
            missing("audio", "audio.effects")
        );
        assert!(
            with11(&["ui.webview"], |v| v["webview"] =
                serde_json::json!({"places": ["dialog"]}))
            .is_ok()
        );
    }

    #[test]
    fn missing_permission_leaves_a_diagnostics_entry() {
        let err =
            with11(&[], |v| v["stage"] = serde_json::json!({"title": "G"})).expect_err("refused");
        assert_eq!(
            err.diagnostic(),
            Some(LoadIssue::FieldWithoutPermission {
                field: "stage",
                permission: "ui.stage"
            })
        );
        let err = with11(&["ui.app-menu"], |v| {
            v["app_menu"] = serde_json::json!([{"id": "a", "label": "Mumcord Games"}]);
        })
        .expect_err("refused");
        assert_eq!(err.diagnostic(), Some(LoadIssue::RefusedAppMenuLabel));
        assert_eq!(ManifestError::TooLarge.diagnostic(), None);
    }

    #[test]
    fn stage_participants_are_capped_at_16() {
        let stage = |n: u32| {
            with11(&["ui.stage"], |v| {
                v["stage"] = serde_json::json!({"title": "G", "max_participants": n})
            })
        };
        assert!(stage(1).is_ok());
        assert!(stage(16).is_ok());
        for n in [0, 17, 1000] {
            assert!(
                matches!(stage(n), Err(ManifestError::OutOfRange { max: 16, .. })),
                "{n}"
            );
        }
    }

    #[test]
    fn min_api_is_checked_against_the_declared_api() {
        // A 1.0 manifest naming a 1.1 permission fails to load.
        let err =
            with(|v| v["permissions"]["required"] = serde_json::json!(["ui.stage", "commands"]))
                .expect_err("refused");
        assert_eq!(
            err,
            ManifestError::ApiTooOld {
                permission: "ui.stage".into(),
                needs: "1.1".into(),
                has: "1.0".into()
            }
        );
        let optional =
            with(|v| v["permissions"]["optional"] = serde_json::json!(["audio.effects"]));
        assert!(matches!(optional, Err(ManifestError::ApiTooOld { .. })));
        // 1.1 fields in a 1.0 manifest are refused too.
        let field = with(|v| v["stage"] = serde_json::json!({"title": "G"}));
        assert!(matches!(field, Err(ManifestError::ApiTooOld { .. })));
        assert!(with11(&["ui.stage"], |_| {}).is_ok());
    }

    #[test]
    fn the_api_range_needs_a_minor() {
        for api in ["^1", "1.0", "~1.0", ">=1.0", "^1.0, ^1.1", "^0.9", "*"] {
            assert!(
                matches!(
                    with(|v| v["api"] = api.into()),
                    Err(ManifestError::BadApiRange(_))
                ),
                "{api}"
            );
        }
        assert!(with(|v| v["api"] = "^1.1".into()).is_ok());
    }

    #[test]
    fn origins_are_validated_per_source() {
        let m = with11(&["net.origins"], |v| {
            v["net"] = serde_json::json!({"origins": [
                {"origin": "https://api.example", "purpose": "origins.api"},
                {"origin": "wss://live.example:8443", "purpose": "origins.live"}
            ]});
        })
        .expect("valid");
        assert_eq!(m.validate_origins(false).expect("ok").len(), 2);
        assert!(m.validate(Source::Official).is_ok());
        assert!(m.validate(Source::Repo { repository_id: 1 }).is_ok());

        let origin = |o: &str| {
            with11(&["net.origins"], |v| {
                v["net"] = serde_json::json!({"origins": [{"origin": o, "purpose": "origins.x"}]});
            })
            .expect("parses")
        };
        let bad = origin("https://1.2.3.4");
        for source in [
            Source::Official,
            Source::Repo { repository_id: 1 },
            Source::Dev,
        ] {
            assert_eq!(
                bad.validate(source),
                Err(ManifestError::Origin(HardStop::IpAddress))
            );
        }
        let dev = origin("http://localhost:5173");
        assert!(dev.validate(Source::Dev).is_ok());
        assert_eq!(
            dev.validate(Source::Repo { repository_id: 1 }),
            Err(ManifestError::Origin(HardStop::NotEncrypted))
        );
        assert_eq!(
            dev.validate(Source::Official),
            Err(ManifestError::Origin(HardStop::NotEncrypted))
        );
    }

    #[test]
    fn origins_are_capped_and_unique() {
        let many = |n: usize| {
            let list: Vec<_> = (0..n)
                .map(|i| serde_json::json!({"origin": format!("https://a{i}.example"), "purpose": "origins.x"}))
                .collect();
            with11(&["net.origins"], |v| {
                v["net"] = serde_json::json!({"origins": list})
            })
            .expect("parses")
        };
        assert!(many(8).validate(Source::Official).is_ok());
        assert_eq!(
            many(9).validate(Source::Official),
            Err(ManifestError::Origin(HardStop::TooMany))
        );
        let twice = with11(&["net.origins"], |v| {
            v["net"] = serde_json::json!({"origins": [
                {"origin": "https://a.example", "purpose": "origins.x"},
                {"origin": "https://A.example:443", "purpose": "origins.x"}
            ]});
        })
        .expect("parses");
        assert_eq!(
            twice.validate(Source::Official),
            Err(ManifestError::DuplicateOrigin("https://a.example".into()))
        );
    }

    #[test]
    fn app_menu_labels_follow_the_rules() {
        let item = |id: &str, label: &str| {
            with11(&["ui.app-menu"], |v| {
                v["app_menu"] = serde_json::json!([{"id": id, "label": label}]);
            })
        };
        assert!(item("play", "Play dice").is_ok());
        for label in [
            "Mumcord",
            "mumcord games",
            "  MUMCORD",
            "Plugins",
            "settings",
            "Settings…",
            "Do  not DISTURB",
            "Einstellungen",
            "Beenden",
            "Quit",
        ] {
            assert_eq!(
                item("a", label),
                Err(ManifestError::ReservedLabel(label.into())),
                "{label}"
            );
        }
        assert!(matches!(
            item("a", &"x".repeat(33)),
            Err(ManifestError::TooLong { .. })
        ));
        assert!(matches!(
            item("A!", "Play"),
            Err(ManifestError::BadValue { .. })
        ));
        let two = with11(&["ui.app-menu"], |v| {
            v["app_menu"] =
                serde_json::json!([{"id": "a", "label": "One"}, {"id": "a", "label": "Two"}]);
        });
        assert_eq!(two, Err(ManifestError::DuplicateAppMenuItem("a".into())));
        let three = with11(&["ui.app-menu"], |v| {
            v["app_menu"] = serde_json::json!([{"id": "a", "label": "A1"}, {"id": "b", "label": "B1"}, {"id": "c", "label": "C1"}]);
        });
        assert!(matches!(
            three,
            Err(ManifestError::OutOfRange { max: 2, .. })
        ));
    }

    #[test]
    fn sound_declarations_are_checked() {
        let sounds = |list: serde_json::Value| {
            with11(&["audio.effects"], |v| {
                v["audio"] = serde_json::json!({"sounds": list})
            })
        };
        assert!(sounds(serde_json::json!(["sounds/a.ogg", "sounds/b.wav"])).is_ok());
        for bad in ["a.ogg", "assets/a.ogg", "sounds/../a.ogg", "/sounds/a.ogg"] {
            assert!(
                matches!(
                    sounds(serde_json::json!([bad])),
                    Err(ManifestError::BadPath(_))
                ),
                "{bad}"
            );
        }
        assert!(matches!(
            sounds(serde_json::json!(["sounds/a.ogg", "sounds/a.ogg"])),
            Err(ManifestError::BadPath(_))
        ));
        let many: Vec<_> = (0..33).map(|i| format!("sounds/{i}.ogg")).collect();
        assert!(matches!(
            sounds(serde_json::json!(many)),
            Err(ManifestError::OutOfRange { max: 32, .. })
        ));
    }

    /// The committed list is what the generator would write from the key list (FR-9.54).
    #[test]
    fn builtin_labels_match_the_locale_files() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let keys: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("scripts/builtin-label-keys.json")).expect("key list"),
        )
        .expect("key list is JSON");
        let mut expected = BTreeSet::new();
        let mut locales = 0;
        for dir in std::fs::read_dir(root.join("ui/src/locales")).expect("locales") {
            let dir = dir.expect("entry").path();
            locales += 1;
            for entry in keys["keys"].as_array().expect("keys") {
                let file = dir.join(format!("{}.json", entry["namespace"].as_str().expect("ns")));
                let json: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&file).expect("locale file"))
                        .expect("JSON");
                let value = entry["key"]
                    .as_str()
                    .expect("key")
                    .split('.')
                    .try_fold(&json, |v, part| v.get(part))
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| panic!("{} lacks {}", file.display(), entry["key"]));
                expected.insert(normalize_label(value));
            }
        }
        assert_eq!(locales, 24);
        let committed: BTreeSet<String> = builtin_labels().iter().cloned().collect();
        assert_eq!(
            committed, expected,
            "run: node scripts/gen-builtin-labels.mjs"
        );
    }
}
