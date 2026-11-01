//! The plugin service (spec §7.2): the one place that changes which plugins are installed and
//! running. It ties the runtime ([`PluginHost`]), the permission gate, the store rows, the
//! integrity record and the installed folders together, so every step of a plugin's life
//! (install, enable, disable, permission changes, remove, reset, Turn off all, developer
//! folders) keeps them in step.
//!
//! Every method may block: loading compiles a component and enabling waits for the plugin's
//! `start` (up to 500 ms). The app keeps the service behind a mutex and calls it on a blocking
//! thread, never on the UI or audio threads. What the UI must hear about arrives as
//! [`ServiceEvent`]s; what plugin threads report arrives as [`HostEvent`]s, which the app hands
//! back to [`PluginService::handle_host_event`].

mod commands;
mod dev;
mod install;
mod lifecycle;
mod messaging;
mod plugin_io;
mod revocation;
mod ui;
mod updates;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mumcord_store::{IdentityId, PluginRecord, PluginVisibility, Store, UnixMillis};
use semver::Version;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::api::secrets::PluginSecrets;
use crate::api::storage::{DataError, Storage};
use crate::host::{HostError, HostEvent, LoadError, PluginHost, RunState};
use crate::manifest::{ManifestError, OptionalPermission, Source};
use crate::package::PackageError;
use crate::peer_key::PeerKeys;
use crate::registry::RegistryError;
use crate::runtime::{CompileError, HostContext};
use crate::verify::{Integrity, VerifyFailure};
use crate::wit::events::{Event, PluginSettings};
use crate::wit::theme::Mode;

pub use commands::{CommandOutcome, PluginCommand};
pub use install::{InstallRequest, InstallStage, fetch};
pub use messaging::Announce;
pub use plugin_io::{PeerInfo, SelfInfo, UiUpdate};
pub use revocation::{REVOKED_PREFIX_TEXT, RevocationNotice, cached_revocations};
pub use ui::UiInput;
pub use updates::{Approval, Due, Schedule, UpdateInfo, load_repository, run_check};

/// The permission that lets a plugin show a badge next to user names (FR-6.63).
const BADGE_PERMISSION: &str = "ui.member-badge";

/// How the service is set up at app start.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// `<data>/plugins`: installed versions, the integrity record and the peer-key secret.
    pub plugins_dir: PathBuf,
    /// Compiled-plugin cache (`None`: compile on every start).
    pub cache_dir: Option<PathBuf>,
    pub app_version: Version,
    /// `--no-plugins` (FR-6.6): no plugin is loaded in this run; nothing is persisted.
    pub no_plugins: bool,
}

/// What the UI hears from the service.
#[derive(Debug, Clone)]
pub enum ServiceEvent {
    /// The plugin list or a plugin's state changed (`plugins://changed`).
    Changed,
    /// One step of an install (`plugins://install-progress`).
    InstallProgress { id: String, stage: InstallStage },
    /// A plugin was turned off after repeated crashes: "Turned off after repeated crashes" plus
    /// a notification-centre entry.
    AutoOff { id: String },
    /// A plugin's `notify.send` passed the limits. Title and body are untrusted text.
    Notify {
        id: String,
        title: String,
        body: String,
    },
    /// A plugin message for the engine's low-priority lane (`Engine::plugin_send`). When its
    /// ticket ends `Expired`, call [`PluginService::message_expired`].
    PeerSend {
        server: mumcord_store::ServerId,
        plugin: String,
        outgoing: crate::messaging::Outgoing,
    },
    /// A plugin changed its UI (`plugin://ui`). Trees are untrusted.
    Ui { plugin: String, update: UiUpdate },
    /// `chat.send` passed the gesture gate: post `line` ("[Name] text") as a channel message
    /// through the normal chat path.
    ChatPost { plugin: String, line: String },
    /// Hide text message `message` and show the plugin's card `card` instead (FR-6.45).
    Supersede {
        plugin: String,
        card: String,
        message: u64,
    },
    /// A new plugin version is waiting in the Updates tab: a notification-centre entry, no OS
    /// notification (FR-6.69). Sent once per version.
    UpdateAvailable {
        id: String,
        name: String,
        version: String,
    },
}

/// Where a plugin is in its life, for Settings → Plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    /// Off by the user, a revoked required permission, revocation or tampering (see
    /// [`PluginInfo::disabled_reason`]).
    Off,
    /// Crashed; starts again by itself.
    BackingOff {
        error: String,
    },
    /// Turned off after repeated crashes; stays off until the user turns it on.
    AutoOff {
        error: String,
    },
    /// "Turn off all plugins" is on.
    AllOff,
    /// Started with `--no-plugins`: nothing is loaded in this run.
    NoPlugins,
    /// The installed files could not be loaded (incompatible, missing, broken).
    NotLoaded {
        error: String,
    },
}

/// One plugin as Settings → Plugins shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
    pub id: String,
    /// Empty when the plugin could not be loaded.
    pub name: String,
    pub version: String,
    pub source: Source,
    /// The folder of a developer plugin ("Developer plugin" label, FR-6.73).
    pub developer_folder: Option<PathBuf>,
    pub status: Status,
    /// Whether the user wants it on (it may still be off because of Turn off all).
    pub enabled: bool,
    /// `crashes`, `revoked`, `tampered`, `user` or `permission`.
    pub disabled_reason: Option<String>,
    pub required: Vec<String>,
    pub optional: Vec<OptionalPermission>,
    pub granted: Vec<String>,
    pub visibility: PluginVisibility,
    pub pinned: bool,
    pub notifications: bool,
    pub installed_at: UnixMillis,
    /// This plugin's badge is the one shown next to names (FR-6.63).
    pub badge: bool,
    /// A `revoked.json` entry matching the installed version (FR-6.70).
    pub revocation: Option<RevocationNotice>,
    /// The manifest icon, a path under `assets/`.
    pub icon: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Verify(#[from] VerifyFailure),
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error(transparent)]
    Host(#[from] HostError),
    #[error(transparent)]
    Compile(#[from] CompileError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Store(#[from] mumcord_store::Error),
    #[error(transparent)]
    Data(#[from] DataError),
    #[error("could not write the plugin files: {0}")]
    Io(#[from] io::Error),
    #[error("no plugin with the id {0:?} is installed")]
    NotInstalled(String),
    #[error("This plugin is already installed")]
    AlreadyInstalled,
    #[error("Another plugin with the id {0:?} is already installed")]
    IdTaken(String),
    #[error("Turn on the plugin's required permissions first")]
    RequiredMissing,
    #[error("All plugins are turned off")]
    AllOff,
    #[error("Plugins are off for this session (--no-plugins)")]
    NoPlugins,
    #[error("Developer mode is off")]
    DeveloperModeOff,
    #[error("{0:?} is not a developer plugin")]
    NotDeveloper(String),
    #[error("No update is available for {0:?}")]
    NoUpdate(String),
    /// The update needs new permissions or comes from a new author key; it waits in the
    /// Updates tab (FR-6.67).
    #[error("This update needs your approval")]
    NeedsApproval,
    #[error("Mumcord turned this plugin off: {0}")]
    Revoked(String),
}

/// Where an installed plugin came from, as stored in `plugin.source`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Origin {
    Official,
    Repo(i64),
    Dev(PathBuf),
}

impl Origin {
    fn parse(text: &str) -> Option<Self> {
        if text == "official" {
            return Some(Self::Official);
        }
        if let Some(id) = text.strip_prefix("repo:") {
            return id.parse().ok().map(Self::Repo);
        }
        text.strip_prefix("dev:")
            .filter(|path| !path.is_empty())
            .map(|path| Self::Dev(PathBuf::from(path)))
    }

    fn from_source(source: Source) -> Self {
        match source {
            Source::Official => Self::Official,
            Source::Repo { repository_id } => Self::Repo(repository_id),
            // Developer folders are stored by `dev.rs` with their path.
            Source::Dev => Self::Dev(PathBuf::new()),
        }
    }

    fn source(&self) -> Source {
        match self {
            Self::Official => Source::Official,
            Self::Repo(repository_id) => Source::Repo {
                repository_id: *repository_id,
            },
            Self::Dev(_) => Source::Dev,
        }
    }

    fn to_db(&self) -> String {
        match self {
            Self::Official => "official".into(),
            Self::Repo(id) => format!("repo:{id}"),
            Self::Dev(path) => format!("dev:{}", path.display()),
        }
    }
}

/// See the module docs.
pub struct PluginService {
    host: PluginHost,
    ctx: Arc<HostContext>,
    io: Arc<plugin_io::ServiceIo>,
    store: Arc<Mutex<Store>>,
    plugins_dir: PathBuf,
    integrity: Integrity,
    /// The version the per-app-version re-hash runs for (B14; debug builds can fake it).
    integrity_version: String,
    no_plugins: bool,
    language: String,
    /// Do Not Disturb or quiet time.
    quiet: bool,
    /// Why an installed plugin could not be loaded in this run.
    load_errors: BTreeMap<String, String>,
    events: UnboundedSender<ServiceEvent>,
    /// Available updates and rollback reasons (FR-6.66–6.69).
    updates: updates::Updates,
    /// The latest revocation list applied (bundled, cached or fetched).
    revoked: crate::registry::RevocationList,
}

impl PluginService {
    /// Builds the service and its shared host context. Nothing is loaded until
    /// [`Self::start`]. `secrets` is the keychain for the `secrets` interface (`None`: off).
    /// Returns the receivers for host events (to be passed back to
    /// [`Self::handle_host_event`]) and service events (for the UI).
    pub fn new(
        config: ServiceConfig,
        store: Arc<Mutex<Store>>,
        secrets: Option<Arc<PluginSecrets>>,
    ) -> Result<
        (
            Self,
            UnboundedReceiver<HostEvent>,
            UnboundedReceiver<ServiceEvent>,
        ),
        ServiceError,
    > {
        std::fs::create_dir_all(&config.plugins_dir)?;
        let peer_keys = PeerKeys::load_or_create(&config.plugins_dir)?;
        let (events, events_rx) = unbounded_channel();
        let io = Arc::new(plugin_io::ServiceIo::new(
            Some(peer_keys.clone()),
            events.clone(),
        ));
        let ctx = Arc::new(HostContext {
            storage: Storage::new(Arc::clone(&store)),
            secrets,
            peer_keys: Some(peer_keys),
            http: crate::api::http::Http::new(&config.plugins_dir),
            io: Some(Arc::clone(&io) as Arc<dyn crate::api::io::PluginIo>),
            ..HostContext::default()
        });
        let (host_tx, host_rx) = unbounded_channel();
        let host = PluginHost::new(
            config.app_version.clone(),
            config.cache_dir,
            Arc::clone(&ctx),
            host_tx,
        )?;
        let service = Self {
            host,
            ctx,
            io,
            store,
            integrity: Integrity::load(&config.plugins_dir)?,
            plugins_dir: config.plugins_dir,
            integrity_version: crate::verify::app_version(&config.app_version.to_string()),
            no_plugins: config.no_plugins,
            language: "en".into(),
            quiet: false,
            load_errors: BTreeMap::new(),
            events,
            updates: updates::Updates::default(),
            revoked: crate::registry::RevocationList::bundled(),
        };
        Ok((service, host_rx, events_rx))
    }

    /// The shared host context (for the host API parts the app wires up).
    pub fn context(&self) -> &Arc<HostContext> {
        &self.ctx
    }

    /// Started with `--no-plugins` (banner in Settings → Plugins).
    pub fn no_plugins(&self) -> bool {
        self.no_plugins
    }

    /// Replaces the restart back-off (5 s, 30 s, 5 min); for tests and the debug test hook.
    pub fn set_backoff(&mut self, steps: [std::time::Duration; 3]) {
        self.host.set_backoff(steps);
    }

    /// Every installed plugin (developer plugins included), by id.
    pub fn list(&self) -> Result<Vec<PluginInfo>, ServiceError> {
        let records = self.store().plugins()?;
        let badge = self.badge_plugin()?;
        Ok(records
            .into_iter()
            .map(|record| self.info(record, badge.as_deref()))
            .collect())
    }

    /// The plugin's own log lines, oldest first (Developer tab, FR-6.73).
    pub fn log_lines(&self, id: &str) -> Vec<(crate::Level, String)> {
        self.host.log_lines(id)
    }

    pub fn diagnostics(&self, id: &str) -> Option<crate::DiagSnapshot> {
        self.host.diagnostics(id).map(|diag| diag.snapshot())
    }

    /// The installed folder of a running or loaded plugin (assets, translations).
    pub fn dir(&self, id: &str) -> Option<&Path> {
        self.host.dir(id)
    }

    /// Every third-party repository's id and name, for the "Unofficial · <name>" label (FR-6.61).
    pub fn repository_names(&self) -> Result<Vec<(i64, String)>, ServiceError> {
        Ok(self
            .store()
            .plugin_repositories()?
            .into_iter()
            .map(|r| (r.id, r.name))
            .collect())
    }

    /// The badge shown next to names (FR-6.63): the chosen plugin if it is installed and holds
    /// `ui.member-badge`, else none.
    pub fn badge_plugin(&self) -> Result<Option<String>, ServiceError> {
        let store = self.store();
        let Some(chosen) = store.settings()?.plugins_badge_plugin else {
            return Ok(None);
        };
        Ok(store
            .plugin(&chosen)?
            .filter(|r| r.removed_at.is_none() && r.granted.iter().any(|p| p == BADGE_PERMISSION))
            .map(|r| r.id))
    }

    /// The user picked the badge plugin (`None`: no badge).
    pub fn set_badge_plugin(&mut self, id: Option<String>) -> Result<(), ServiceError> {
        let store = self.store();
        let mut settings = store.settings()?;
        settings.plugins_badge_plugin = id;
        store.save_settings(&settings)?;
        drop(store);
        self.changed();
        Ok(())
    }

    /// The identity whose `identity` storage scope plugins see: the focused server's, else the
    /// default identity.
    pub fn set_identity(&self, identity: Option<IdentityId>) {
        self.ctx.storage.set_identity(identity);
    }

    /// The app language changed: running plugins get `language-changed`.
    pub fn set_language(&mut self, language: &str) {
        language.clone_into(&mut self.language);
        self.host
            .broadcast(&Event::LanguageChanged(self.language.clone()));
    }

    /// The theme changed: running plugins get `theme-changed`.
    pub fn set_theme(&self, mode: Mode, reduced_motion: bool) {
        self.ctx.theme.set(mode, reduced_motion);
        self.host
            .broadcast(&Event::ThemeChanged(self.ctx.theme.info()));
    }

    /// What a plugin thread reported. Started plugins get their settings, language and theme;
    /// a plugin that stays off after repeated crashes is stored as off with reason `crashes`.
    pub fn handle_host_event(&mut self, event: HostEvent) -> Result<(), ServiceError> {
        match event {
            HostEvent::Started { id } => {
                if let Some(record) = self.store().plugin(&id)? {
                    self.host.send(&id, settings_event(&record));
                }
                self.host
                    .send(&id, Event::LanguageChanged(self.language.clone()));
                self.host
                    .send(&id, Event::ThemeChanged(self.ctx.theme.info()));
                self.host.send(&id, self.self_event(&id));
                self.changed();
            }
            HostEvent::Failed { id, recovery, .. } => {
                if recovery == crate::Recovery::StayOff {
                    self.update_record(&id, |record| {
                        record.enabled = false;
                        record.disabled_reason = Some("crashes".into());
                    })?;
                    self.emit(ServiceEvent::AutoOff { id });
                }
                self.changed();
            }
            HostEvent::Notify { id, title, body } => {
                self.emit(ServiceEvent::Notify { id, title, body });
            }
        }
        Ok(())
    }

    fn info(&self, record: PluginRecord, badge: Option<&str>) -> PluginInfo {
        let origin = Origin::parse(&record.source);
        let manifest = self.host.manifest(&record.id);
        let status = if self.no_plugins {
            Status::NoPlugins
        } else if let Some(error) = self.load_errors.get(&record.id) {
            Status::NotLoaded {
                error: error.clone(),
            }
        } else {
            match self.host.state(&record.id) {
                Some(RunState::Running) => Status::Running,
                Some(RunState::BackingOff { error, .. }) => Status::BackingOff {
                    error: error.to_string(),
                },
                Some(RunState::AutoOff { error }) => Status::AutoOff {
                    error: error.to_string(),
                },
                Some(RunState::AllOff) => Status::AllOff,
                Some(RunState::Disabled) | None => Status::Off,
            }
        };
        let revocation = self.revocation_notice(&record);
        PluginInfo {
            name: manifest.map(|m| m.name.clone()).unwrap_or_default(),
            icon: manifest.and_then(|m| m.icon.clone()),
            required: manifest
                .map(|m| m.permissions.required.clone())
                .unwrap_or_default(),
            optional: manifest
                .map(|m| m.permissions.optional.clone())
                .unwrap_or_default(),
            source: origin.as_ref().map_or(Source::Dev, Origin::source),
            developer_folder: match origin {
                Some(Origin::Dev(path)) => Some(path),
                _ => None,
            },
            status,
            badge: badge == Some(record.id.as_str()),
            id: record.id,
            version: record.version,
            enabled: record.enabled,
            disabled_reason: record.disabled_reason,
            granted: record.granted,
            visibility: record.visibility,
            pinned: record.pinned,
            notifications: record.notifications,
            revocation,
            installed_at: record.installed_at,
        }
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// An installed (not removed) plugin's row.
    fn record(&self, id: &str) -> Result<PluginRecord, ServiceError> {
        self.store()
            .plugin(id)?
            .filter(|r| r.removed_at.is_none())
            .ok_or_else(|| ServiceError::NotInstalled(id.to_owned()))
    }

    /// Changes and saves an installed plugin's row; returns the saved row.
    fn update_record(
        &self,
        id: &str,
        change: impl FnOnce(&mut PluginRecord),
    ) -> Result<PluginRecord, ServiceError> {
        let mut record = self.record(id)?;
        change(&mut record);
        self.store().save_plugin(&record)?;
        Ok(record)
    }

    /// Makes the newly installed `id` the badge plugin if it is the only installed plugin with
    /// the badge permission and none is chosen yet (FR-6.63 default: the first one installed).
    fn default_badge(&self, id: &str) -> Result<(), ServiceError> {
        let store = self.store();
        let mut settings = store.settings()?;
        if settings.plugins_badge_plugin.is_some() {
            return Ok(());
        }
        let holders: Vec<String> = store
            .plugins()?
            .into_iter()
            .filter(|r| r.granted.iter().any(|p| p == BADGE_PERMISSION))
            .map(|r| r.id)
            .collect();
        if holders == [id] {
            settings.plugins_badge_plugin = Some(id.to_owned());
            store.save_settings(&settings)?;
        }
        Ok(())
    }

    /// Clears the badge choice when its plugin goes away.
    fn forget_badge(&self, id: &str) -> Result<(), ServiceError> {
        let store = self.store();
        let mut settings = store.settings()?;
        if settings.plugins_badge_plugin.as_deref() == Some(id) {
            settings.plugins_badge_plugin = None;
            store.save_settings(&settings)?;
        }
        Ok(())
    }

    fn emit(&self, event: ServiceEvent) {
        // The app holds the receiver for as long as the service lives; a closed channel only
        // happens at shutdown, when nobody needs the event any more.
        let _ = self.events.send(event);
    }

    fn changed(&self) {
        self.sync_peers();
        self.emit(ServiceEvent::Changed);
    }
}

/// `settings-changed` for a plugin's row.
fn settings_event(record: &PluginRecord) -> Event {
    use crate::wit::events::Visibility;
    Event::SettingsChanged(PluginSettings {
        granted: record.granted.clone(),
        visibility: match record.visibility {
            PluginVisibility::Everyone => Visibility::Everyone,
            PluginVisibility::Friends => Visibility::Friends,
            PluginVisibility::Nobody => Visibility::Nobody,
        },
    })
}

/// The part of `wanted` a plugin may hold: only permissions its manifest asks for, each once,
/// and never an official-only permission for a plugin that is not official (FR-6.12, FR-6.73).
fn allowed_grants<'a>(
    manifest: &crate::Manifest,
    source: Source,
    wanted: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let official = source == Source::Official;
    let mut granted: Vec<String> = Vec::new();
    for id in wanted {
        let allowed = official || !crate::permissions::get(id).is_some_and(|p| p.official_only);
        if allowed && manifest.has_permission(id) && !granted.iter().any(|g| g == id) {
            granted.push(id.to_owned());
        }
    }
    granted
}

/// The required permissions plus the optional ones in `optional`.
fn with_required<'a>(
    manifest: &'a crate::Manifest,
    optional: impl IntoIterator<Item = &'a str>,
) -> impl Iterator<Item = &'a str> {
    manifest
        .permissions
        .required
        .iter()
        .map(String::as_str)
        .chain(optional)
}

fn now_millis() -> UnixMillis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
