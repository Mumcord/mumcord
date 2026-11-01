//! Glue between the engine, the plugin service (`mumcord_plugins`) and the UI (spec 06, §8).
//!
//! The service lives behind one mutex and is only called on blocking threads: loading or
//! starting a plugin can take a while and must never stall the async runtime. Engine and
//! service events are pumped by background tasks; commands are thin wrappers.

mod feed;
pub mod suggestions;
pub mod ui;
pub mod webview;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mumcord_engine::Engine;
use mumcord_plugins::api::secrets::PluginSecrets;
use mumcord_plugins::api::webview::Webviews;
use mumcord_plugins::registry::{Client, Download, Index, UrlPolicy};
use mumcord_plugins::service::{
    InstallStage, PluginService, Schedule, ServiceConfig, ServiceError, ServiceEvent, Status,
    run_check,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::commands::types::{ApiError, ApiResult, ErrorCode};
use ui::{PluginUi, Trees};

/// The command-line flag that starts Mumcord without any plugin (FR-6.6).
const NO_PLUGINS_FLAG: &str = "--no-plugins";
/// How often updates and revocations are checked for (B17b decides what is due).
const REGISTRY_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// The app's plugin state (managed as `Arc<Plugins>`).
pub struct Plugins {
    service: Arc<Mutex<PluginService>>,
    trees: Mutex<Trees>,
    /// The last official index, for installs from the store.
    index: Mutex<Option<Index>>,
    /// The last indexes of the switched-on third-party repositories, oldest repository first.
    repo_indexes: Mutex<Vec<RepoIndex>>,
    /// Registry caches, one folder per source (`official`, `repo-<id>`).
    registry_cache: PathBuf,
    /// The package verified for the open install prompt, installed on "Install".
    prepared: Mutex<Option<PreparedInstall>>,
    /// Store icons and screenshots as data URLs, by source URL (the UI never fetches).
    images: Mutex<HashMap<String, String>>,
    /// Open web views, shared with the service: the `mumcord-web` protocol reads it without
    /// waiting for the service lock.
    webviews: Arc<Webviews>,
}

/// One third-party repository's signed index, as Browse last loaded it.
pub struct RepoIndex {
    pub repository_id: i64,
    pub name: String,
    pub index: Index,
}

/// A downloaded and verified package waiting for the user's answer in the install prompt.
pub struct PreparedInstall {
    pub id: String,
    pub version: String,
    pub download: Download,
}

/// Images kept for the session; the store shows a few dozen at most.
const MAX_CACHED_IMAGES: usize = 200;

/// `plugins://changed`: the plugin list or a plugin's state changed; reload `plugins_list`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "plugins://changed")]
pub struct PluginsChanged {}

/// `plugins://shared-state`: whether `server` passes plugin data (FR-6.38). Sent whenever the
/// engine decides, so the UI can show the "not supported" hint; `plugins_shared_supported`
/// answers for a server that decided before the UI listened.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "plugins://shared-state")]
pub struct PluginsSharedState {
    pub server_id: crate::commands::types::ServerId,
    pub supported: bool,
}

/// One step of an install.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "stage", rename_all = "camelCase")]
pub enum PluginInstallStage {
    /// `received` of `total` package bytes (packages are at most 64 MiB, so u32 fits).
    #[serde(rename_all = "camelCase")]
    Downloading {
        received: u32,
        total: u32,
    },
    Verifying,
    Unpacking,
    Starting,
    /// Installed: the UI shows "<name> installed".
    Done,
    /// Stopped; `reason` is a user-facing text.
    Failed {
        reason: String,
    },
}

impl From<InstallStage> for PluginInstallStage {
    fn from(stage: InstallStage) -> Self {
        match stage {
            InstallStage::Downloading { received, total } => Self::Downloading {
                received: u32::try_from(received).unwrap_or(u32::MAX),
                total: u32::try_from(total).unwrap_or(u32::MAX),
            },
            InstallStage::Verifying => Self::Verifying,
            InstallStage::Unpacking => Self::Unpacking,
            InstallStage::Starting => Self::Starting,
            InstallStage::Done => Self::Done,
            InstallStage::Failed { reason } => Self::Failed { reason },
        }
    }
}

/// `plugins://install-progress`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "plugins://install-progress")]
pub struct PluginInstallProgress {
    pub id: String,
    pub progress: PluginInstallStage,
}

impl Plugins {
    /// Runs `f` on the service on a blocking thread.
    pub async fn call<T, F>(self: &Arc<Self>, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut PluginService) -> Result<T, ServiceError> + Send + 'static,
    {
        let service = Arc::clone(&self.service);
        tauri::async_runtime::spawn_blocking(move || f(&mut lock(&service)))
            .await
            .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
            .map_err(service_error)
    }

    /// Like [`Self::call`] for steps that fail with app errors.
    pub async fn call_api<T, F>(self: &Arc<Self>, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut PluginService) -> ApiResult<T> + Send + 'static,
    {
        let service = Arc::clone(&self.service);
        tauri::async_runtime::spawn_blocking(move || f(&mut lock(&service)))
            .await
            .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
    }

    /// "Check now": looks for updates right away, ignoring the 6-hour wait. The check's future is
    /// not `Send`, so it runs on a blocking thread.
    pub async fn check_now(self: &Arc<Self>, developer_mode: bool) -> ApiResult<()> {
        let this = Arc::clone(self);
        let runtime = tauri::async_runtime::handle().inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let client = Client::new(UrlPolicy::new(developer_mode))
                .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
            let mut schedule = Schedule::new();
            runtime
                .block_on(run_check(
                    &client,
                    &this.service,
                    &mut schedule,
                    &this.registry_cache,
                ))
                .map_err(service_error)
        })
        .await
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
    }

    pub fn webviews(&self) -> &Webviews {
        &self.webviews
    }

    pub fn trees(&self) -> MutexGuard<'_, Trees> {
        lock(&self.trees)
    }

    pub fn index(&self) -> MutexGuard<'_, Option<Index>> {
        lock(&self.index)
    }

    pub fn repo_indexes(&self) -> MutexGuard<'_, Vec<RepoIndex>> {
        lock(&self.repo_indexes)
    }

    /// The service itself, for registry steps that lock it between network calls.
    pub fn service(&self) -> &Arc<Mutex<PluginService>> {
        &self.service
    }

    pub fn registry_cache(&self) -> &std::path::Path {
        &self.registry_cache
    }

    pub fn prepared(&self) -> MutexGuard<'_, Option<PreparedInstall>> {
        lock(&self.prepared)
    }

    pub fn cached_image(&self, url: &str) -> Option<String> {
        lock(&self.images).get(url).cloned()
    }

    pub fn cache_image(&self, url: &str, data_url: &str) {
        let mut images = lock(&self.images);
        if images.len() >= MAX_CACHED_IMAGES {
            images.clear();
        }
        images.insert(url.to_owned(), data_url.to_owned());
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The plugin state, or an error when the service could not start.
pub fn state(app: &AppHandle) -> ApiResult<Arc<Plugins>> {
    app.try_state::<Arc<Plugins>>()
        .map(|s| Arc::clone(&s))
        .ok_or_else(|| ApiError::new(ErrorCode::Internal, "plugins are not available"))
}

/// Maps service errors; their texts are the user-facing ones of spec §5.7.
pub fn service_error(error: ServiceError) -> ApiError {
    let code = match &error {
        ServiceError::NotInstalled(_) => ErrorCode::NotFound,
        ServiceError::Io(_) => ErrorCode::Io,
        ServiceError::Store(_)
        | ServiceError::Host(_)
        | ServiceError::Compile(_)
        | ServiceError::Load(_)
        | ServiceError::Data(_) => ErrorCode::Internal,
        ServiceError::Registry(_) => ErrorCode::ServerUnreachable,
        _ => ErrorCode::InvalidInput,
    };
    ApiError::new(code, error.to_string())
}

/// Builds the plugin service and starts its plugins in the background. Without it the app runs
/// normally; plugin commands then fail.
pub fn start(app: &AppHandle, engine: &Engine) {
    if let Err(e) = try_start(app, engine) {
        tracing::error!(error = %e, "starting the plugin service failed");
    }
}

fn try_start(app: &AppHandle, engine: &Engine) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = crate::portable::data_dir(app)?;
    let cache_dir = crate::portable::cache_dir(app)?;
    let no_plugins = std::env::args().any(|a| a == NO_PLUGINS_FLAG);
    let config = ServiceConfig {
        // FR-6.15. The game plugin host scans the same folder for libraries only.
        plugins_dir: data_dir.join("plugins"),
        cache_dir: Some(cache_dir.join("plugin-modules")),
        app_version: semver::Version::parse(env!("CARGO_PKG_VERSION"))?,
        no_plugins,
    };
    let secrets = Some(Arc::new(PluginSecrets::os()));
    let (mut service, host_events, events) =
        PluginService::new(config, engine.store_handle(), secrets)?;
    test_hooks(&mut service);
    tracing::info!(no_plugins, "plugin service ready");
    let webviews = service.webviews();
    let plugins = Arc::new(Plugins {
        service: Arc::new(Mutex::new(service)),
        trees: Mutex::default(),
        index: Mutex::default(),
        repo_indexes: Mutex::default(),
        registry_cache: cache_dir.join("plugin-registry"),
        prepared: Mutex::default(),
        images: Mutex::default(),
        webviews,
    });
    app.manage(Arc::clone(&plugins));

    let starting = Arc::clone(&plugins);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = starting.call(PluginService::start).await {
            tracing::error!(error = %e, "starting the plugins failed");
        }
    });
    pump_host_events(&plugins, host_events);
    pump_service_events(app, &plugins, engine, events);
    feed::pump_engine_events(app, &plugins, engine);
    feed::pump_announces(&plugins, engine);
    pump_registry_checks(&plugins, engine);
    focus(app, &plugins, engine.voice_server());
    Ok(())
}

/// Debug builds only: shorter restart back-off for the crash tests (AC-6.8).
#[cfg(debug_assertions)]
fn test_hooks(service: &mut PluginService) {
    if std::env::var_os("MUMCORD_PLUGIN_TEST_FAST_BACKOFF").is_some() {
        let step = std::time::Duration::from_millis(500);
        service.set_backoff([step, step * 2, step * 4]);
    }
}

#[cfg(not(debug_assertions))]
fn test_hooks(_service: &mut PluginService) {}

/// Updates and revocations (B17b): at start and every 15 minutes; the service decides what
/// is due. Runs on its own thread because the check's future is not `Send`.
fn pump_registry_checks(plugins: &Arc<Plugins>, engine: &Engine) {
    let plugins = Arc::clone(plugins);
    let engine = engine.clone();
    let runtime = tauri::async_runtime::handle().inner().clone();
    let spawned = std::thread::Builder::new()
        .name("plugin-updates".into())
        .spawn(move || {
            let mut schedule = Schedule::new();
            loop {
                let developer_mode = engine.settings().map(|s| s.developer_mode).unwrap_or(false);
                match Client::new(UrlPolicy::new(developer_mode)) {
                    Ok(client) => {
                        let check = run_check(
                            &client,
                            &plugins.service,
                            &mut schedule,
                            &plugins.registry_cache,
                        );
                        if let Err(e) = runtime.block_on(check) {
                            tracing::warn!(error = %e, "the plugin update check failed");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "no client for the plugin update check"),
                }
                std::thread::sleep(REGISTRY_CHECK_INTERVAL);
            }
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "starting the plugin update checks failed");
    }
}

/// Plugin starts and crashes go back into the service.
fn pump_host_events(
    plugins: &Arc<Plugins>,
    mut host_events: UnboundedReceiver<mumcord_plugins::HostEvent>,
) {
    let plugins = Arc::clone(plugins);
    tauri::async_runtime::spawn(async move {
        while let Some(event) = host_events.recv().await {
            if let Err(e) = plugins
                .call(move |service| service.handle_host_event(event))
                .await
            {
                tracing::warn!(error = %e, "handling a plugin host event failed");
            }
        }
    });
}

/// Service events to the UI.
fn pump_service_events(
    app: &AppHandle,
    plugins: &Arc<Plugins>,
    engine: &Engine,
    mut events: UnboundedReceiver<ServiceEvent>,
) {
    let app = app.clone();
    let plugins = Arc::clone(plugins);
    let engine = engine.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            if let Err(e) = on_service_event(&app, &plugins, &engine, event).await {
                tracing::warn!(error = %e, "forwarding a plugin event failed");
            }
        }
    });
}

async fn on_service_event(
    app: &AppHandle,
    plugins: &Arc<Plugins>,
    engine: &Engine,
    event: ServiceEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    match event {
        ServiceEvent::Changed => {
            let (running, chat_patterns) = plugins
                .call(|service| {
                    let running = service
                        .list()?
                        .into_iter()
                        .filter(|p| p.status == Status::Running)
                        .map(|p| p.id)
                        .collect::<Vec<_>>();
                    Ok((running, service.chat_pattern_set()))
                })
                .await?;
            // The engine forwards only chat messages that match an active plugin's pattern.
            engine.plugin_set_chat_patterns(chat_patterns);
            plugins.trees().retain(|id| running.iter().any(|r| r == id));
            plugins
                .webviews()
                .close_unless(|id| running.iter().any(|r| r == id));
            PluginsChanged {}.emit(app)?;
        }
        ServiceEvent::InstallProgress { id, stage } => PluginInstallProgress {
            id,
            progress: stage.into(),
        }
        .emit(app)?,
        ServiceEvent::Ui { plugin, update } => {
            let change = ui::PluginUiChange::new(&plugin, update);
            plugins.trees().apply(&plugin, &change);
            PluginUi { plugin, change }.emit(app)?;
        }
        ServiceEvent::AutoOff { id } => PluginNotification {
            plugin: id,
            notice: PluginNotice::AutoOff,
        }
        .emit(app)?,
        ServiceEvent::UpdateAvailable { id, name, version } => PluginNotification {
            plugin: id,
            notice: PluginNotice::UpdateAvailable { name, version },
        }
        .emit(app)?,
        ServiceEvent::Notify { id, title, body } => PluginNotification {
            plugin: id,
            notice: PluginNotice::Message { title, body },
        }
        .emit(app)?,
        ServiceEvent::PeerSend {
            server,
            plugin,
            outgoing,
        } => feed::send(plugins, engine, server, plugin, outgoing),
        ServiceEvent::ChatPost {
            plugin,
            server,
            line,
        } => match feed::post_line(engine, server, &line).await {
            Ok(message) => crate::commands::chat::MessageReceived {
                server_id: crate::commands::types::to_api_server_id(server),
                message: message.into(),
            }
            .emit(app)?,
            Err(e) => tracing::info!(plugin, error = %e, "plugin chat line not posted"),
        },
        ServiceEvent::Supersede {
            plugin,
            card,
            message,
        } => PluginSupersede {
            plugin,
            card,
            message_id: message.to_string(),
        }
        .emit(app)?,
    }
    Ok(())
}

/// What a plugin notification says.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginNotice {
    /// `notify.send`; title and body are untrusted plain text.
    Message { title: String, body: String },
    /// "Turned off after repeated crashes".
    AutoOff,
    /// A new version waits in the Updates tab (no OS notification).
    UpdateAvailable { name: String, version: String },
}

/// `plugins://notification`: an entry for the notification centre (category Plugins).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "plugins://notification")]
pub struct PluginNotification {
    pub plugin: String,
    pub notice: PluginNotice,
}

/// `plugin://supersede`: hide chat message `messageId` and show the plugin's chat card `card`
/// in its place, or only a "Hidden by <plugin>" line when `card` is `null` (FR-6.45). The UI
/// keeps the original text one click away and shows the plugin's icon.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "plugin://supersede")]
pub struct PluginSupersede {
    pub plugin: String,
    pub card: Option<String>,
    pub message_id: String,
}

/// Engine events the plugins care about besides their own stream: the focused server and
/// incoming chat lines (supersede check).
pub fn on_engine_event(
    app: &AppHandle,
    server_id: crate::commands::types::ServerId,
    event: &mumcord_engine::EngineEvent,
) {
    let server_id = crate::commands::types::to_engine_server_id(server_id);
    let Ok(plugins) = state(app) else {
        return;
    };
    match event {
        mumcord_engine::EngineEvent::VoiceServerChanged(server) => {
            focus(app, &plugins, *server);
        }
        mumcord_engine::EngineEvent::MessageReceived(message) => {
            let Some((id, sender, line)) = feed::incoming_line(message) else {
                return;
            };
            let plugins = Arc::clone(&plugins);
            tauri::async_runtime::spawn(async move {
                let result = plugins
                    .call(move |service| Ok(service.chat_text(server_id, id, sender, &line)))
                    .await;
                // A match arrives as a `Supersede` service event; nothing to do here.
                if let Err(e) = result {
                    tracing::warn!(error = %e, "the plugin supersede check failed");
                }
            });
        }
        _ => {}
    }
}

/// Plugins see the server the user is on; identity-scoped storage follows its identity.
fn focus(app: &AppHandle, plugins: &Arc<Plugins>, server: Option<mumcord_engine::ServerId>) {
    let app = app.clone();
    let plugins = Arc::clone(plugins);
    tauri::async_runtime::spawn(async move {
        let identity = identity_of(&app, server).await;
        let result = plugins
            .call(move |service| {
                service.set_focused_server(server);
                service.set_identity(identity);
                Ok(())
            })
            .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "telling plugins about the server failed");
        }
    });
}

/// The identity used on `server`, else the default one.
pub(crate) async fn identity_of(
    app: &AppHandle,
    server: Option<mumcord_engine::ServerId>,
) -> Option<mumcord_engine::IdentityId> {
    let engine = app.try_state::<Engine>()?;
    let identities = match engine.identity_list().await {
        Ok(identities) => identities,
        Err(e) => {
            tracing::warn!(error = %e, "reading the identities for plugins failed");
            return None;
        }
    };
    server
        .and_then(|s| identities.iter().find(|i| i.server_ids.contains(&s)))
        .or_else(|| identities.iter().find(|i| i.is_default))
        .map(|i| i.id)
}

/// Do Not Disturb holds back plugin notifications.
pub fn set_quiet(app: &AppHandle, quiet: bool) {
    let Ok(plugins) = state(app) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let result = plugins
            .call(move |service| {
                service.set_quiet(quiet);
                Ok(())
            })
            .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "passing Do Not Disturb to plugins failed");
        }
    });
}

/// Developer mode turned on or off: developer plugins are unloaded when it goes off.
pub fn developer_mode_changed(app: &AppHandle, on: bool) {
    let Ok(plugins) = state(app) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(e) = plugins
            .call(move |service| service.developer_mode_changed(on))
            .await
        {
            tracing::warn!(error = %e, "applying Developer mode to plugins failed");
        }
    });
}
