//! Plugin commands (spec 06, §8): thin wrappers around the plugin service. The UI never talks
//! to plugins or the network itself.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine as _;
use mumcord_engine::Engine;
use mumcord_engine::store::PluginVisibility;
use mumcord_plugins::manifest::Visibility as ManifestVisibility;
use mumcord_plugins::registry::{
    Client, Freshness, Index, IndexPlugin, MAX_ICON_BYTES, MAX_SCREENSHOT_BYTES, OFFLINE_TEXT,
    UrlPolicy, fetch_official, official_base,
};
use mumcord_plugins::service::{
    Approval, CommandOutcome, InstallRequest, PluginInfo, PluginService, ServiceError, Status,
    UiInput, fetch, load_repository,
};
use mumcord_plugins::verify::verify_package;
use mumcord_plugins::wit::events::{ActionSource, ActionValue};
use mumcord_plugins::wit::theme::Mode;
use mumcord_plugins::{Level, Manifest, Source};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use super::types::{ApiError, ApiResult, ErrorCode, ServerId, SessionId, to_engine_server_id};
use crate::plugins::ui::{ActionKind, PluginPanelVisibility, PluginSlot, PluginUi};
use crate::plugins::{self, Plugins, PreparedInstall, RepoIndex};

/// Who sees that the user has a plugin (FR-6.35).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginVisibilityChoice {
    Everyone,
    Friends,
    Nobody,
}

impl From<PluginVisibility> for PluginVisibilityChoice {
    fn from(v: PluginVisibility) -> Self {
        match v {
            PluginVisibility::Everyone => Self::Everyone,
            PluginVisibility::Friends => Self::Friends,
            PluginVisibility::Nobody => Self::Nobody,
        }
    }
}

impl From<PluginVisibilityChoice> for PluginVisibility {
    fn from(v: PluginVisibilityChoice) -> Self {
        match v {
            PluginVisibilityChoice::Everyone => Self::Everyone,
            PluginVisibilityChoice::Friends => Self::Friends,
            PluginVisibilityChoice::Nobody => Self::Nobody,
        }
    }
}

/// Where an installed plugin came from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginSourceInfo {
    Official,
    Repo {
        repository_id: u32,
        /// Shown as "Unofficial · <name>"; empty when the repository was removed.
        name: String,
    },
    /// "Developer plugin", loaded from `folder`.
    Dev {
        folder: String,
    },
}

/// Where a plugin is in its life.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginStatus {
    Running,
    /// Off; see `disabledReason`.
    Off,
    /// Crashed; starts again by itself.
    BackingOff {
        error: String,
    },
    /// "Turned off after repeated crashes".
    AutoOff {
        error: String,
    },
    /// "Turn off all plugins" is on.
    AllOff,
    /// Started with `--no-plugins`.
    NoPlugins,
    /// The installed files could not be loaded.
    NotLoaded {
        error: String,
    },
}

impl From<Status> for PluginStatus {
    fn from(s: Status) -> Self {
        match s {
            Status::Running => Self::Running,
            Status::Off => Self::Off,
            Status::BackingOff { error } => Self::BackingOff { error },
            Status::AutoOff { error } => Self::AutoOff { error },
            Status::AllOff => Self::AllOff,
            Status::NoPlugins => Self::NoPlugins,
            Status::NotLoaded { error } => Self::NotLoaded { error },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginOptionalPermission {
    pub id: String,
    /// Preselected in the install prompt.
    pub suggested: bool,
}

/// One installed plugin.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub id: String,
    /// Empty when the plugin could not be loaded.
    pub name: String,
    pub version: String,
    pub source: PluginSourceInfo,
    pub status: PluginStatus,
    /// Whether the user wants it on.
    pub enabled: bool,
    /// `crashes`, `revoked`, `tampered`, `user` or `permission`.
    pub disabled_reason: Option<String>,
    pub required: Vec<String>,
    pub optional: Vec<PluginOptionalPermission>,
    pub granted: Vec<String>,
    pub visibility: PluginVisibilityChoice,
    pub pinned: bool,
    pub notifications: bool,
    /// Unix milliseconds.
    pub installed_at: f64,
    /// This plugin's badge is the one shown next to names.
    pub badge: bool,
    /// The reason from Mumcord's revocation list for the installed version, in the UI language.
    /// With `disabledReason` `revoked` the plugin was turned off; otherwise it is a warning.
    pub revocation: Option<String>,
    /// The manifest icon (a path under `assets/`, loaded with `plugin_asset`).
    pub icon: Option<String>,
}

/// The name of a third-party repository; empty when it was removed.
fn repository_name(repositories: &[(i64, String)], id: i64) -> String {
    repositories
        .iter()
        .find(|(repo, _)| *repo == id)
        .map(|(_, name)| name.clone())
        .unwrap_or_default()
}

impl InstalledPlugin {
    /// `repositories` maps repository ids to names for the "Unofficial · <name>" label.
    fn new(p: PluginInfo, repositories: &[(i64, String)]) -> Self {
        let source = match (p.source, p.developer_folder) {
            (Source::Dev, folder) => PluginSourceInfo::Dev {
                folder: folder.map(|f| f.display().to_string()).unwrap_or_default(),
            },
            (Source::Repo { repository_id }, _) => PluginSourceInfo::Repo {
                repository_id: u32::try_from(repository_id).unwrap_or(u32::MAX),
                name: repository_name(repositories, repository_id),
            },
            (Source::Official, _) => PluginSourceInfo::Official,
        };
        Self {
            id: p.id,
            name: p.name,
            version: p.version,
            source,
            status: p.status.into(),
            enabled: p.enabled,
            disabled_reason: p.disabled_reason,
            required: p.required,
            optional: p
                .optional
                .into_iter()
                .map(|o| PluginOptionalPermission {
                    id: o.id,
                    suggested: o.suggested,
                })
                .collect(),
            granted: p.granted,
            visibility: p.visibility.into(),
            pinned: p.pinned,
            notifications: p.notifications,
            installed_at: p.installed_at as f64,
            badge: p.badge,
            revocation: p.revocation.map(|r| r.reason),
            icon: p.icon,
        }
    }
}

/// Settings → Plugins.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginsOverview {
    /// Started with `--no-plugins`: show the banner.
    pub no_plugins: bool,
    /// "Turn off all plugins" is on.
    pub all_off: bool,
    pub plugins: Vec<InstalledPlugin>,
}

/// How fresh the store's plugin list is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum IndexFreshness {
    Fresh,
    NotModified,
    /// The last known list (`notice` says so).
    Offline,
    /// A new list failed a check; the last known one is shown.
    Rejected,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexScreenshot {
    pub url: String,
    pub alt: String,
}

/// Which list a store plugin comes from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum IndexSource {
    Official,
    /// "Unofficial · <name>" (FR-6.61).
    Repo {
        repository_id: u32,
        name: String,
    },
}

/// One plugin of the store (official or a repository), texts in the asked language (English fallback).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexedPlugin {
    pub id: String,
    pub source: IndexSource,
    pub name: String,
    pub summary: String,
    pub description_md_url: Option<String>,
    pub icon: Option<String>,
    pub screenshots: Vec<IndexScreenshot>,
    pub categories: Vec<String>,
    pub authors: Vec<String>,
    pub reviewed: bool,
    pub updated: String,
    /// The latest version.
    pub version: String,
    /// Package size in bytes.
    pub size: f64,
    /// Every permission (required and optional) the version may ask for.
    pub permissions: Vec<String>,
}

impl IndexedPlugin {
    fn new(p: &IndexPlugin, source: IndexSource, language: &str) -> Self {
        let latest = p.latest();
        let alt = |texts: &std::collections::BTreeMap<String, String>| {
            texts
                .get(language)
                .or_else(|| texts.get("en"))
                .cloned()
                .unwrap_or_default()
        };
        Self {
            id: p.id.clone(),
            source,
            name: p.name(language).to_owned(),
            summary: p.summary(language).to_owned(),
            description_md_url: p.description_md_url.clone(),
            icon: p.icon.clone(),
            screenshots: p
                .screenshots
                .iter()
                .map(|s| IndexScreenshot {
                    url: s.url.clone(),
                    alt: alt(&s.alt),
                })
                .collect(),
            categories: p.categories.clone(),
            authors: p.authors.clone(),
            reviewed: p.reviewed,
            updated: p.updated.clone(),
            version: latest.version.clone(),
            size: latest.size as f64,
            permissions: latest.permissions.clone(),
        }
    }
}

/// The store's official plugin list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginIndex {
    pub freshness: IndexFreshness,
    /// A user-facing text when the list is not fresh.
    pub notice: Option<String>,
    pub plugins: Vec<IndexedPlugin>,
}

/// The value of a `ui-action` as the UI sends it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PluginActionValue {
    None,
    Boolean {
        value: bool,
    },
    Number {
        value: f32,
    },
    /// Also the typed text of a password field (it goes into the secret, never to the plugin).
    Text {
        value: String,
    },
    /// A list row moved from `from` to `to`.
    Reorder {
        from: u32,
        to: u32,
    },
    /// The path the user picked in a file picker.
    File {
        path: String,
    },
}

/// A user input for a plugin's UI other than an action.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PluginUiInput {
    ProfileCardOpened {
        session: SessionId,
    },
    PanelVisibility {
        visibility: PluginPanelVisibility,
    },
    UserMenuRequested {
        session: SessionId,
    },
    DialogClosed,
    /// A bridge message from the plugin's web view page (≤ 16 KiB, ≤ 60/s; dropped otherwise).
    WebviewMessage {
        text: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginLogLine {
    pub level: PluginLogLevel,
    pub text: String,
}

/// The Diagnostics section of a plugin (counts only, never payloads).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginDiagnostics {
    pub cpu_time_ms: f64,
    pub memory_bytes: f64,
    pub peak_memory_bytes: f64,
    pub dropped_events: f64,
    pub dropped_messages: f64,
    pub restarts: f64,
    pub messages_sent: f64,
    pub messages_rate_limited: f64,
    pub messages_expired: f64,
    pub messages_received: f64,
    pub incoming_rate_dropped: f64,
    pub incoming_invalid: f64,
    /// The plugin's own log lines (in memory only).
    pub log: Vec<PluginLogLine>,
}

pub(super) fn plugins_of(app: &AppHandle) -> ApiResult<Arc<Plugins>> {
    plugins::state(app)
}

fn overview(service: &mut PluginService) -> Result<PluginsOverview, ServiceError> {
    Ok(PluginsOverview {
        no_plugins: service.no_plugins(),
        all_off: service.all_off()?,
        plugins: {
            let repositories = service.repository_names()?;
            service
                .list()?
                .into_iter()
                .map(|p| InstalledPlugin::new(p, &repositories))
                .collect()
        },
    })
}

/// Every installed plugin, developer plugins included.
#[tauri::command]
#[specta::specta]
pub async fn plugins_list(app: AppHandle) -> ApiResult<PluginsOverview> {
    plugins_of(&app)?.call(overview).await
}

/// Loads the official plugin list (cached copy when offline, FR-6.55) followed by the plugins of
/// every switched-on repository (FR-6.61, FR-6.74). A repository that cannot be loaded is left
/// out; one whose key changed is paused and its plugins turned off (FR-6.75).
#[tauri::command]
#[specta::specta]
pub async fn plugins_index(
    app: AppHandle,
    engine: State<'_, Engine>,
    language: String,
) -> ApiResult<PluginIndex> {
    let plugins = plugins_of(&app)?;
    let settings = engine.settings()?;
    let client = registry_client(settings.developer_mode)?;
    let base = official_base(settings.plugins_mirror_url.as_deref()).map_err(registry_error)?;
    let cache = plugins.registry_cache().join("official");
    std::fs::create_dir_all(&cache).map_err(|e| ApiError::new(ErrorCode::Io, e.to_string()))?;
    let last_seen = u64::try_from(settings.plugins_last_official_sequence)
        .ok()
        .filter(|s| *s > 0);
    let loaded = fetch_official(&client, &cache, &base, last_seen)
        .await
        .map_err(registry_error)?;
    let (freshness, notice) = match &loaded.freshness {
        Freshness::Fresh => (IndexFreshness::Fresh, None),
        Freshness::NotModified => (IndexFreshness::NotModified, None),
        Freshness::Offline => (IndexFreshness::Offline, Some(OFFLINE_TEXT.to_owned())),
        Freshness::Rejected(e) => (IndexFreshness::Rejected, Some(e.to_string())),
    };
    if matches!(loaded.freshness, Freshness::Fresh) {
        let sequence = i64::try_from(loaded.index.sequence).unwrap_or(i64::MAX);
        if sequence > settings.plugins_last_official_sequence {
            let mut stored = engine.settings()?;
            stored.plugins_last_official_sequence = sequence;
            engine.update_settings(stored).await?;
        }
    }
    let repositories = {
        let store = engine.store_handle();
        let store = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store
            .plugin_repositories()
            .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
    };
    let official_ids: BTreeSet<String> =
        loaded.index.plugins.iter().map(|p| p.id.clone()).collect();
    let is_official_id = |id: &str| official_ids.contains(id);
    let mut repo_indexes = Vec::new();
    for repository in repositories
        .iter()
        .filter(|r| r.enabled && r.paused_reason.is_none())
    {
        let index = load_repository(
            &client,
            plugins.service(),
            plugins.registry_cache(),
            repository,
            &is_official_id,
        )
        .await
        .map_err(plugins::service_error)?;
        if let Some(index) = index {
            repo_indexes.push(RepoIndex {
                repository_id: repository.id,
                name: repository.name.clone(),
                index,
            });
        }
    }
    let mut listed: Vec<IndexedPlugin> = loaded
        .index
        .plugins
        .iter()
        .map(|p| IndexedPlugin::new(p, IndexSource::Official, &language))
        .collect();
    listed.extend(
        repo_plugins(Some(&loaded.index), &repo_indexes)
            .into_iter()
            .map(|(repo, p)| {
                let source = IndexSource::Repo {
                    repository_id: u32::try_from(repo.repository_id).unwrap_or(u32::MAX),
                    name: repo.name.clone(),
                };
                IndexedPlugin::new(p, source, &language)
            }),
    );
    *plugins.index() = Some(loaded.index);
    *plugins.repo_indexes() = repo_indexes;
    Ok(PluginIndex {
        freshness,
        notice,
        plugins: listed,
    })
}

/// The repository plugins Browse lists: an id the official list has, or an older repository
/// already lists, is skipped, so official plugins are never shadowed (FR-6.76).
fn repo_plugins<'a>(
    official: Option<&Index>,
    repositories: &'a [RepoIndex],
) -> Vec<(&'a RepoIndex, &'a IndexPlugin)> {
    let mut seen: BTreeSet<&str> = official
        .map(|i| i.plugins.iter().map(|p| p.id.as_str()).collect())
        .unwrap_or_default();
    let mut listed = Vec::new();
    for repository in repositories {
        for plugin in &repository.index.plugins {
            if seen.insert(plugin.id.as_str()) {
                listed.push((repository, plugin));
            }
        }
    }
    listed
}

/// A plugin of the loaded store lists with the source it installs from.
fn find_indexed(plugins: &Plugins, id: &str) -> ApiResult<(Source, IndexPlugin)> {
    let index = plugins.index();
    if let Some(plugin) = index.as_ref().and_then(|i| i.plugin(id)) {
        return Ok((Source::Official, plugin.clone()));
    }
    let repositories = plugins.repo_indexes();
    repo_plugins(index.as_ref(), &repositories)
        .into_iter()
        .find(|(_, p)| p.id == id)
        .map(|(repo, p)| {
            let source = Source::Repo {
                repository_id: repo.repository_id,
            };
            (source, p.clone())
        })
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "plugin not in the list"))
}

/// What the install prompt shows, read from the verified package's manifest (the index only
/// lists every permission, not which are required).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallOffer {
    pub id: String,
    pub version: String,
    pub authors: Vec<String>,
    pub required: Vec<String>,
    pub optional: Vec<PluginOptionalPermission>,
    /// Preselected in the visibility choice (shown when `peers.messages` is requested).
    pub default_visibility: PluginVisibilityChoice,
}

/// Which store image of an index plugin to load.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginIndexImage {
    Icon,
    Screenshot { index: u32 },
}

/// The source, latest version's index entry, download URL and size of a plugin in the loaded
/// lists.
fn indexed_latest(
    plugins: &Plugins,
    id: &str,
) -> ApiResult<(Source, mumcord_plugins::verify::IndexEntry, String, u64)> {
    let (source, plugin) = find_indexed(plugins, id)?;
    let latest = plugin.latest();
    Ok((
        source,
        plugin.entry(latest),
        latest.url.clone(),
        latest.size,
    ))
}

fn install_offer(id: &str, manifest: &Manifest) -> PluginInstallOffer {
    PluginInstallOffer {
        id: id.to_owned(),
        version: manifest.version.to_string(),
        authors: manifest.authors.clone(),
        required: manifest.permissions.required.clone(),
        optional: manifest
            .permissions
            .optional
            .iter()
            .map(|o| PluginOptionalPermission {
                id: o.id.clone(),
                suggested: o.suggested,
            })
            .collect(),
        default_visibility: match manifest.messaging.default_visibility {
            Some(ManifestVisibility::Friends) => PluginVisibilityChoice::Friends,
            Some(ManifestVisibility::Nobody) => PluginVisibilityChoice::Nobody,
            Some(ManifestVisibility::Everyone) | None => PluginVisibilityChoice::Everyone,
        },
    }
}

/// Downloads and verifies the latest version of a store plugin (official or from a repository)
/// before the install prompt
/// opens, so the prompt can show the manifest's required and optional permissions. Nothing is
/// unpacked or run; `plugin_install` installs this package once the user agrees.
#[tauri::command]
#[specta::specta]
pub async fn plugin_install_prepare(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: String,
) -> ApiResult<PluginInstallOffer> {
    let plugins = plugins_of(&app)?;
    let (source, entry, url, size) = indexed_latest(&plugins, &id)?;
    let client = registry_client(engine.settings()?.developer_mode)?;
    let events = plugins.call(|service| Ok(service.events())).await?;
    let download = fetch(&client, &events, &id, &url, size)
        .await
        .map_err(plugins::service_error)?;
    let (download, offer) = tauri::async_runtime::spawn_blocking(move || {
        let verified = verify_package(&download.bytes, &entry, &download.signature, source)
            .map_err(|e| plugins::service_error(e.into()))?;
        let offer = install_offer(&entry.id, &verified.package.manifest);
        Ok::<_, ApiError>((download, offer))
    })
    .await
    .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))??;
    *plugins.prepared() = Some(PreparedInstall {
        id,
        version: offer.version.clone(),
        download,
    });
    Ok(offer)
}

/// Loads a store image of a plugin in the loaded list as a data URL. It takes an index id, not
/// a URL, so the UI cannot use it to fetch anything else; SVG and oversized files are refused.
#[tauri::command]
#[specta::specta]
pub async fn plugin_index_image(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: String,
    image: PluginIndexImage,
) -> ApiResult<String> {
    let plugins = plugins_of(&app)?;
    let (_, plugin) = find_indexed(&plugins, &id)?;
    let (url, max) = image_url(&plugin, image)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "the plugin has no such image"))?;
    if let Some(cached) = plugins.cached_image(&url) {
        return Ok(cached);
    }
    let client = registry_client(engine.settings()?.developer_mode)?;
    let fetched = client
        .fetch_image(&url, max)
        .await
        .map_err(registry_error)?;
    let data_url = format!(
        "data:{};base64,{}",
        fetched.mime,
        base64::engine::general_purpose::STANDARD.encode(&fetched.bytes)
    );
    plugins.cache_image(&url, &data_url);
    Ok(data_url)
}

/// The URL and size cap of one store image.
fn image_url(plugin: &IndexPlugin, image: PluginIndexImage) -> Option<(String, usize)> {
    match image {
        PluginIndexImage::Icon => plugin.icon.clone().map(|url| (url, MAX_ICON_BYTES)),
        PluginIndexImage::Screenshot { index } => plugin
            .screenshots
            .get(usize::try_from(index).ok()?)
            .map(|s| (s.url.clone(), MAX_SCREENSHOT_BYTES)),
    }
}

/// Installs a plugin from the lists loaded by `plugins_index` (official or a repository) (latest version unless
/// `version` is given), with the optional permissions the user left on.
#[tauri::command]
#[specta::specta]
pub async fn plugin_install(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: String,
    version: Option<String>,
    optional: Vec<String>,
    visibility: PluginVisibilityChoice,
) -> ApiResult<()> {
    let plugins = plugins_of(&app)?;
    let (source, entry, url, size) = {
        let (source, plugin) = find_indexed(&plugins, &id)?;
        let chosen = match &version {
            Some(v) => plugin
                .version(v)
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "version not in the list"))?,
            None => plugin.latest(),
        };
        (
            source,
            plugin.entry(chosen),
            chosen.url.clone(),
            chosen.size,
        )
    };
    // The package the install prompt already verified, unless it was for another version.
    let prepared = plugins
        .prepared()
        .take()
        .filter(|p| p.id == id && p.version == entry.version);
    let download = match prepared {
        Some(prepared) => prepared.download,
        None => {
            let client = registry_client(engine.settings()?.developer_mode)?;
            let events = plugins.call(|service| Ok(service.events())).await?;
            fetch(&client, &events, &id, &url, size)
                .await
                .map_err(plugins::service_error)?
        }
    };
    let request = InstallRequest {
        source,
        entry,
        optional,
        visibility: visibility.into(),
    };
    plugins
        .call(move |service| service.install(&request, &download))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn plugin_set_enabled(app: AppHandle, id: String, enabled: bool) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.set_enabled(&id, enabled))
        .await
}

/// The granted optional permissions (required ones are always granted).
#[tauri::command]
#[specta::specta]
pub async fn plugin_set_permissions(
    app: AppHandle,
    id: String,
    granted: Vec<String>,
) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.set_permissions(&id, &granted))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn plugin_set_visibility(
    app: AppHandle,
    id: String,
    visibility: PluginVisibilityChoice,
) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.set_visibility(&id, visibility.into()))
        .await
}

/// The plugin's "notifications" switch.
#[tauri::command]
#[specta::specta]
pub async fn plugin_set_notifications(app: AppHandle, id: String, on: bool) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.set_notifications(&id, on))
        .await
}

/// Why an update waits for the user (FR-6.67).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginUpdateApproval {
    /// The new version asks for these permissions on top of what is granted.
    NewPermissions { permissions: Vec<String> },
    /// The new version is signed by a different author key.
    NewAuthorKey,
}

/// One update on offer in Settings → Plugins → Updates.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdate {
    pub id: String,
    /// From the signed list; untrusted text.
    pub name: String,
    pub from: String,
    pub to: String,
    pub source: PluginSourceInfo,
    /// Download size in bytes.
    pub size: u32,
    /// `None`: may install without asking.
    pub approval: Option<PluginUpdateApproval>,
    /// This version was installed, failed to start and was rolled back: the reason.
    pub failed: Option<String>,
}

/// The updates waiting for the user.
#[tauri::command]
#[specta::specta]
pub async fn plugin_updates(app: AppHandle) -> ApiResult<Vec<PluginUpdate>> {
    plugins_of(&app)?
        .call(|service| {
            let repositories = service.repository_names()?;
            Ok(service
                .updates()
                .into_iter()
                .map(|u| PluginUpdate {
                    id: u.id,
                    name: u.name,
                    from: u.from,
                    to: u.to,
                    source: match u.source {
                        Source::Repo { repository_id } => PluginSourceInfo::Repo {
                            repository_id: u32::try_from(repository_id).unwrap_or(u32::MAX),
                            name: repository_name(&repositories, repository_id),
                        },
                        _ => PluginSourceInfo::Official,
                    },
                    size: u32::try_from(u.size).unwrap_or(u32::MAX),
                    approval: u.approval.map(|a| match a {
                        Approval::NewPermissions(permissions) => {
                            PluginUpdateApproval::NewPermissions { permissions }
                        }
                        Approval::NewAuthorKey => PluginUpdateApproval::NewAuthorKey,
                    }),
                    failed: u.failed,
                })
                .collect())
        })
        .await
}

/// Installs the waiting update of `id`; `optional` are the optional permissions to keep on.
#[tauri::command]
#[specta::specta]
pub async fn plugin_apply_update(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: String,
    optional: Vec<String>,
) -> ApiResult<()> {
    let plugins = plugins_of(&app)?;
    let target = id.clone();
    let (url, size) = plugins
        .call(move |service| {
            service
                .updates()
                .into_iter()
                .find(|u| u.id == target)
                .map(|u| (u.url, u.size))
                .ok_or(ServiceError::NoUpdate(target))
        })
        .await?;
    let client = registry_client(engine.settings()?.developer_mode)?;
    let events = plugins.call(|service| Ok(service.events())).await?;
    let download = fetch(&client, &events, &id, &url, size)
        .await
        .map_err(plugins::service_error)?;
    plugins
        .call(move |service| service.apply_update(&id, &download, Some(&optional)))
        .await
}

/// "Pin version": a pinned plugin is not offered updates (FR-6.67).
#[tauri::command]
#[specta::specta]
pub async fn plugin_set_pinned(app: AppHandle, id: String, pinned: bool) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.set_pinned(&id, pinned))
        .await
}

/// Removes a plugin; its data is kept for 30 days unless `deleteData`.
#[tauri::command]
#[specta::specta]
pub async fn plugin_remove(app: AppHandle, id: String, delete_data: bool) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.remove(&id, delete_data))
        .await
}

/// The plugin's stored data as JSON (never secrets); the UI saves it with `save_text`.
#[tauri::command]
#[specta::specta]
pub async fn plugin_export_data(app: AppHandle, id: String) -> ApiResult<String> {
    plugins_of(&app)?
        .call(move |service| service.export_data(&id))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn plugin_reset_data(app: AppHandle, id: String) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.reset_data(&id))
        .await
}

/// Developer mode: loads an unpacked plugin folder and returns its id.
#[tauri::command]
#[specta::specta]
pub async fn plugin_dev_load(app: AppHandle, folder: String) -> ApiResult<String> {
    plugins_of(&app)?
        .call(move |service| service.load_dev(&PathBuf::from(folder)))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn plugin_dev_reload(app: AppHandle, id: String) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.reload_dev(&id))
        .await
}

/// Unloads a developer plugin and deletes its data.
#[tauri::command]
#[specta::specta]
pub async fn plugin_dev_unload(app: AppHandle, id: String) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.unload_dev(&id))
        .await
}

/// "Turn off all plugins" (FR-6.6); kept across restarts.
#[tauri::command]
#[specta::specta]
pub async fn plugins_set_all_off(app: AppHandle, all_off: bool) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.set_all_off(all_off))
        .await
}

/// Which plugin's member badge is shown (`None`: no badge).
#[tauri::command]
#[specta::specta]
pub async fn plugin_set_badge(app: AppHandle, id: Option<String>) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| service.set_badge_plugin(id))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn plugin_diagnostics(app: AppHandle, id: String) -> ApiResult<PluginDiagnostics> {
    plugins_of(&app)?
        .call(move |service| {
            let d = service.diagnostics(&id).unwrap_or_default();
            let m = service.messaging_counters(&id);
            let log = service
                .log_lines(&id)
                .into_iter()
                .map(|(level, text)| PluginLogLine {
                    level: match level {
                        Level::Debug => PluginLogLevel::Debug,
                        Level::Info => PluginLogLevel::Info,
                        Level::Warn => PluginLogLevel::Warn,
                        Level::Error => PluginLogLevel::Error,
                    },
                    text,
                })
                .collect();
            Ok(PluginDiagnostics {
                cpu_time_ms: d.cpu_time_ms as f64,
                memory_bytes: d.memory_bytes as f64,
                peak_memory_bytes: d.peak_memory_bytes as f64,
                dropped_events: d.dropped_events as f64,
                dropped_messages: d.dropped_messages as f64,
                restarts: d.restarts as f64,
                messages_sent: m.sent as f64,
                messages_rate_limited: m.rate_limited as f64,
                messages_expired: m.expired as f64,
                messages_received: m.received as f64,
                incoming_rate_dropped: m.incoming_rate_dropped as f64,
                incoming_invalid: m.incoming_invalid as f64,
                log,
            })
        })
        .await
}

/// Every plugin UI currently on screen, as `plugin://ui` changes; for a window that mounts
/// after the plugins started. Reload it after `plugins://changed` (stopped plugins lose
/// their UI).
#[tauri::command]
#[specta::specta]
pub fn plugins_ui_trees(app: AppHandle) -> ApiResult<Vec<PluginUi>> {
    Ok(plugins_of(&app)?.trees().snapshot())
}

/// An image from a plugin's `assets/` folder for the UI renderer (SVG already sanitised at
/// install). The UI turns it into a blob URL; it never reads plugin files itself.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginAsset {
    pub mime_type: String,
    /// The file's bytes, base64-encoded.
    pub base64: String,
}

/// The media type of an asset the package format allows (FR-6.42).
fn asset_mime(path: &str) -> Option<&'static str> {
    let (_, ext) = path.rsplit_once('.')?;
    match ext.to_ascii_lowercase().as_str() {
        "svg" => Some("image/svg+xml"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn plugin_asset(app: AppHandle, id: String, path: String) -> ApiResult<PluginAsset> {
    use base64::Engine as _;
    let mime = asset_mime(&path)
        .filter(|_| mumcord_plugins::api::ui::is_asset_path(&path))
        .ok_or_else(|| invalid("not an image inside assets/"))?;
    let dir = plugins_of(&app)?
        .call(move |service| Ok(service.dir(&id).map(PathBuf::from)))
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "plugin is not running"))?;
    let not_found = |_| ApiError::new(ErrorCode::NotFound, "no such asset");
    let assets = tokio::fs::canonicalize(dir.join("assets"))
        .await
        .map_err(not_found)?;
    let file = tokio::fs::canonicalize(dir.join(&path))
        .await
        .map_err(not_found)?;
    // A link must not lead out of the package's assets.
    if !file.starts_with(&assets) {
        return Err(invalid("not an image inside assets/"));
    }
    let bytes = tokio::fs::read(&file).await.map_err(not_found)?;
    Ok(PluginAsset {
        mime_type: mime.to_owned(),
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// The resolved app language and theme for plugins (the UI knows the system values).
#[tauri::command]
#[specta::specta]
pub async fn plugins_set_appearance(
    app: AppHandle,
    language: String,
    dark: bool,
    reduced_motion: bool,
) -> ApiResult<()> {
    plugins_of(&app)?
        .call(move |service| {
            service.set_language(&language);
            service.set_theme(if dark { Mode::Dark } else { Mode::Light }, reduced_motion);
            Ok(())
        })
        .await
}

/// The user acted in a plugin's UI (`slot`) or typed one of its slash commands (`slot`
/// `None`: `action` is the command, `value` the rest of the line as text). Actions must
/// belong to an enabled component on screen. Server addresses become reachable for the
/// plugin, password text goes into the secret, a picked file becomes a handle.
#[tauri::command]
#[specta::specta]
pub async fn plugin_ui_action(
    app: AppHandle,
    id: String,
    slot: Option<PluginSlot>,
    action: String,
    value: PluginActionValue,
) -> ApiResult<()> {
    let plugins = plugins_of(&app)?;
    let kind = match &slot {
        Some(slot) => plugins
            .trees()
            .action_kind(&id, slot, &action)
            .ok_or_else(|| ApiError::new(ErrorCode::InvalidInput, "no such action on screen"))?,
        None => ActionKind::Plain,
    };
    let source = slot.map_or(ActionSource::Command, |s| ActionSource::Slot(s.into()));
    let delivered = plugins
        .call_api(move |service| {
            let ctx = service.context();
            let value = match (kind, value) {
                (ActionKind::ServerAddress, PluginActionValue::Text { value }) => {
                    ctx.http.allow(&id, &value).map_err(invalid)?;
                    ActionValue::Text(value)
                }
                (ActionKind::Password { secret }, PluginActionValue::Text { value }) => {
                    let secrets = ctx
                        .grants
                        .check(&id, "secrets")
                        .ok()
                        .and(ctx.secrets.as_ref())
                        .ok_or_else(|| invalid("secrets are not available"))?;
                    secrets.set(&id, &secret, &value).map_err(invalid)?;
                    ActionValue::None
                }
                (ActionKind::File, PluginActionValue::File { path }) => {
                    ctx.grants
                        .check(&id, "net.http")
                        .map_err(|_| invalid("not permitted"))?;
                    let handle = ctx
                        .files
                        .register(&id, std::path::Path::new(&path))
                        .map_err(invalid)?;
                    ActionValue::File(handle)
                }
                (ActionKind::Password { .. } | ActionKind::File, _) => {
                    return Err(invalid("wrong value for this action"));
                }
                (_, value) => action_value(value),
            };
            Ok(service.ui_input(
                &id,
                UiInput::Action {
                    source,
                    action,
                    value,
                },
            ))
        })
        .await?;
    delivered_or_error(delivered)
}

/// Other user inputs for a plugin's UI.
#[tauri::command]
#[specta::specta]
pub async fn plugin_ui_event(app: AppHandle, id: String, input: PluginUiInput) -> ApiResult<()> {
    let input = match input {
        PluginUiInput::ProfileCardOpened { session } => UiInput::ProfileCardOpened(session),
        PluginUiInput::PanelVisibility { visibility } => {
            UiInput::PanelVisibility(visibility.into())
        }
        PluginUiInput::UserMenuRequested { session } => UiInput::UserMenuRequested(session),
        PluginUiInput::DialogClosed => UiInput::DialogClosed,
        PluginUiInput::WebviewMessage { text } => UiInput::WebviewMessage(text),
    };
    let delivered = plugins_of(&app)?
        .call(move |service| Ok(service.ui_input(&id, input)))
        .await?;
    delivered_or_error(delivered)
}

/// The UI found a mention of the user in a message from `session` (plugins get `mentioned`,
/// never the text).
#[tauri::command]
#[specta::specta]
pub fn plugins_mentioned(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
) -> ApiResult<()> {
    Ok(engine.plugin_mentioned(to_engine_server_id(server_id), session)?)
}

/// Whether the server passes plugin data (FR-6.38); the answer so far, kept current by the
/// `plugins://shared-state` event.
#[tauri::command]
#[specta::specta]
pub fn plugins_shared_supported(engine: State<'_, Engine>, server_id: ServerId) -> bool {
    engine.plugin_shared_supported(to_engine_server_id(server_id))
}

/// A slash command of an active plugin (FR-6.43).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommand {
    pub plugin_id: String,
    pub plugin_name: String,
    /// Without the leading `/`.
    pub name: String,
    pub description: String,
    pub usage: Option<String>,
    /// Who owns this name instead: show "Command taken by <takenBy>" in the plugin's settings.
    /// `null` when this plugin owns the command (only those are offered in the chat box).
    pub taken_by: Option<String>,
}

/// What happened to a line typed into the chat box.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginCommandOutcome {
    /// A plugin received it; the line is never posted as chat.
    Plugin,
    /// Not a plugin command: send the line as normal text.
    Unknown,
}

/// The slash commands of all running plugins that hold `commands`, in install order.
#[tauri::command]
#[specta::specta]
pub async fn plugin_commands(app: AppHandle) -> ApiResult<Vec<PluginCommand>> {
    let commands = plugins_of(&app)?.call(|service| service.commands()).await?;
    Ok(commands
        .into_iter()
        .map(|c| PluginCommand {
            plugin_id: c.plugin,
            plugin_name: c.plugin_name,
            name: c.name,
            description: c.description,
            usage: c.usage,
            taken_by: c.taken_by,
        })
        .collect())
}

/// Hands a typed line starting with `/` to the plugin that owns the command, as a `ui-action`
/// with source `command` (action = command name, value = the rest of the line).
#[tauri::command]
#[specta::specta]
pub async fn plugin_run_command(app: AppHandle, line: String) -> ApiResult<PluginCommandOutcome> {
    let outcome = plugins_of(&app)?
        .call(move |service| service.run_command(&line))
        .await?;
    Ok(match outcome {
        CommandOutcome::Plugin => PluginCommandOutcome::Plugin,
        CommandOutcome::Unknown => PluginCommandOutcome::Unknown,
    })
}

fn action_value(value: PluginActionValue) -> ActionValue {
    match value {
        PluginActionValue::None | PluginActionValue::File { .. } => ActionValue::None,
        PluginActionValue::Boolean { value } => ActionValue::Boolean(value),
        PluginActionValue::Number { value } => ActionValue::Number(value),
        PluginActionValue::Text { value } => ActionValue::Text(value),
        PluginActionValue::Reorder { from, to } => ActionValue::Reorder((from, to)),
    }
}

fn delivered_or_error(delivered: bool) -> ApiResult<()> {
    if delivered {
        Ok(())
    } else {
        Err(ApiError::new(
            ErrorCode::InvalidInput,
            "the plugin is not running or may not get this",
        ))
    }
}

/// A user-facing refusal; never contains the typed text.
fn invalid(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(ErrorCode::InvalidInput, error.to_string())
}

pub(super) fn registry_client(developer_mode: bool) -> ApiResult<Client> {
    Client::new(UrlPolicy::new(developer_mode)).map_err(registry_error)
}

pub(super) fn registry_error(error: mumcord_plugins::registry::RegistryError) -> ApiError {
    plugins::service_error(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_plugins::registry::Severity;
    use mumcord_plugins::service::RevocationNotice;

    fn info(source: Source) -> PluginInfo {
        PluginInfo {
            id: "foo".into(),
            name: "Foo".into(),
            version: "1.0.0".into(),
            source,
            developer_folder: None,
            status: Status::Off,
            enabled: false,
            disabled_reason: Some("revoked".into()),
            required: Vec::new(),
            optional: Vec::new(),
            granted: Vec::new(),
            visibility: PluginVisibility::Everyone,
            pinned: false,
            notifications: true,
            installed_at: 0,
            badge: false,
            revocation: Some(RevocationNotice {
                severity: Severity::Disable,
                reason: "Leaks data".into(),
            }),
            icon: None,
        }
    }

    #[test]
    fn installed_plugins_carry_their_repository_name_and_revocation_reason() {
        let repositories = vec![(3, "Foo Repo".to_string())];
        let row = InstalledPlugin::new(info(Source::Repo { repository_id: 3 }), &repositories);
        assert_eq!(
            row.source,
            PluginSourceInfo::Repo {
                repository_id: 3,
                name: "Foo Repo".into()
            }
        );
        assert_eq!(row.revocation.as_deref(), Some("Leaks data"));
        let gone = InstalledPlugin::new(info(Source::Repo { repository_id: 9 }), &repositories);
        assert_eq!(
            gone.source,
            PluginSourceInfo::Repo {
                repository_id: 9,
                name: String::new()
            }
        );
    }

    #[test]
    fn assets_are_only_images() {
        assert_eq!(asset_mime("assets/egg.SVG"), Some("image/svg+xml"));
        assert_eq!(asset_mime("assets/pet.png"), Some("image/png"));
        assert_eq!(asset_mime("assets/pet.webp"), Some("image/webp"));
        assert_eq!(asset_mime("assets/notes.txt"), None);
        assert_eq!(asset_mime("assets/noext"), None);
    }

    #[test]
    fn action_values_map_to_wit() {
        assert!(matches!(
            action_value(PluginActionValue::Reorder { from: 1, to: 3 }),
            ActionValue::Reorder((1, 3))
        ));
        assert!(matches!(
            action_value(PluginActionValue::File { path: "/x".into() }),
            ActionValue::None
        ));
        let json = serde_json::json!({"type": "text", "value": "2d6"});
        let value: PluginActionValue = serde_json::from_value(json).expect("parses");
        assert!(matches!(action_value(value), ActionValue::Text(t) if t == "2d6"));
    }

    #[test]
    fn plugin_slots_round_trip_through_json() {
        let slot = PluginSlot::MemberBadge { session: 7 };
        let json = serde_json::to_value(&slot).expect("serialises");
        assert_eq!(
            json,
            serde_json::json!({"kind": "memberBadge", "session": 7})
        );
        assert_eq!(
            serde_json::from_value::<PluginSlot>(json).expect("parses"),
            slot
        );
    }

    fn indexed(icon: Option<&str>) -> IndexPlugin {
        serde_json::from_value(serde_json::json!({
            "id": "dice",
            "name": {"en": "Dice"},
            "summary": {"en": "Roll dice"},
            "icon": icon,
            "screenshots": [{"url": "https://example.org/a.png", "alt": {"en": "A roll"}}],
            "author_keys": [],
            "reviewed": true,
            "updated": "2026-10-01",
            "versions": [{"version": "1.0.0", "api": 1, "url": "https://example.org/d.mcplugin",
                          "size": 10, "sha256": "ab".repeat(32), "permissions": []}]
        }))
        .expect("test step")
    }

    #[test]
    fn store_images_come_only_from_the_index_entry() {
        let plugin = indexed(Some("https://example.org/icon.png"));
        assert_eq!(
            image_url(&plugin, PluginIndexImage::Icon),
            Some(("https://example.org/icon.png".to_owned(), MAX_ICON_BYTES))
        );
        assert_eq!(
            image_url(&plugin, PluginIndexImage::Screenshot { index: 0 }),
            Some(("https://example.org/a.png".to_owned(), MAX_SCREENSHOT_BYTES))
        );
        assert_eq!(
            image_url(&plugin, PluginIndexImage::Screenshot { index: 1 }),
            None
        );
        assert_eq!(image_url(&indexed(None), PluginIndexImage::Icon), None);
    }

    #[test]
    fn install_offer_splits_required_and_optional() {
        let manifest = Manifest::parse(
            serde_json::json!({
                "manifest": 1, "id": "poll", "name": "Poll", "description": "Polls",
                "version": "1.1.0", "api": "^1.0", "authors": ["Mumcord"],
                "license": "MIT", "categories": ["chat"],
                "permissions": {
                    "required": ["ui.chat-card", "peers.messages"],
                    "optional": ["notify", {"id": "ui.panel", "suggested": true}]
                },
                "messaging": {"max_payload": 256, "default_visibility": "friends"}
            })
            .to_string()
            .as_bytes(),
        )
        .expect("test step");
        let offer = install_offer("poll", &manifest);
        assert_eq!(offer.version, "1.1.0");
        assert_eq!(offer.required, ["ui.chat-card", "peers.messages"]);
        assert_eq!(
            offer.optional,
            [
                PluginOptionalPermission {
                    id: "notify".into(),
                    suggested: false
                },
                PluginOptionalPermission {
                    id: "ui.panel".into(),
                    suggested: true
                },
            ]
        );
        assert_eq!(offer.default_visibility, PluginVisibilityChoice::Friends);
    }

    fn store_index(ids: &[&str]) -> Index {
        let plugins: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| {
                serde_json::json!({
                    "id": id,
                    "name": {"en": id},
                    "summary": {"en": "A plugin"},
                    "author_keys": ["RWStYW7iMZi0HAbHe+TL+GQ4NH70MYE6hJMKZDGGvpbqucV6UpbYwVXz"],
                    "updated": "2026-10-01",
                    "versions": [{"version": "1.0.0", "api": 1,
                        "url": format!("https://example.org/{id}.mcplugin"),
                        "size": 10, "sha256": "ab".repeat(32), "permissions": []}]
                })
            })
            .collect();
        let bytes = serde_json::to_vec(
            &serde_json::json!({"format": 1, "sequence": 1, "plugins": plugins}),
        )
        .expect("index json");
        Index::parse(&bytes, &mumcord_plugins::registry::IndexKind::Official).expect("index")
    }

    fn repo(repository_id: i64, ids: &[&str]) -> RepoIndex {
        RepoIndex {
            repository_id,
            name: format!("Repo {repository_id}"),
            index: store_index(ids),
        }
    }

    #[test]
    fn repositories_never_shadow_official_plugins_and_the_oldest_repository_wins() {
        let official = store_index(&["dice"]);
        let repositories = [repo(1, &["dice", "radio"]), repo(2, &["radio", "quiz"])];
        let listed: Vec<(i64, &str)> = repo_plugins(Some(&official), &repositories)
            .into_iter()
            .map(|(r, p)| (r.repository_id, p.id.as_str()))
            .collect();
        assert_eq!(listed, vec![(1, "radio"), (2, "quiz")]);
    }
}
