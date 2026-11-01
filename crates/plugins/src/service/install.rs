//! Installing from a signed index (FR-6.15, FR-6.29, FR-6.60, spec §6.3): download → verify →
//! unpack atomically → compile → start, with a progress event per step. A failed step leaves
//! nothing behind: no half-written folder, no row, no grants.

use mumcord_store::{PluginRecord, PluginVisibility};
use tokio::sync::mpsc::UnboundedSender;

use super::{Origin, PluginService, ServiceError, ServiceEvent, allowed_grants, now_millis};
use crate::manifest::Source;
use crate::registry::{Client, Download};
use crate::verify::{IndexEntry, verify_package};

/// What the user accepted in the install prompt.
#[derive(Debug, Clone)]
pub struct InstallRequest {
    /// Official or a third-party repository; developer folders use `load_dev`.
    pub source: Source,
    /// The signed index entry of the version to install.
    pub entry: IndexEntry,
    /// The optional permissions the user switched on; required ones are always granted.
    pub optional: Vec<String>,
    /// The visibility choice (only asked when the plugin uses `peers.messages`).
    pub visibility: PluginVisibility,
}

/// One step of an install, for the progress shown in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallStage {
    Downloading,
    Verifying,
    Unpacking,
    Starting,
    /// Installed; the UI shows the toast "<name> installed".
    Done,
    /// The install stopped; `reason` is a user-facing text (§5.7).
    Failed {
        reason: String,
    },
}

/// Downloads a package and its author signature. Runs outside the service lock, as it can take
/// a while; reports `Downloading`, and `Failed` if the download fails.
pub async fn fetch(
    client: &Client,
    events: &UnboundedSender<ServiceEvent>,
    id: &str,
    url: &str,
    size: u64,
) -> Result<Download, ServiceError> {
    let progress = |stage| {
        let _ = events.send(ServiceEvent::InstallProgress {
            id: id.to_owned(),
            stage,
        });
    };
    progress(InstallStage::Downloading);
    client.download(url, size).await.map_err(|error| {
        progress(InstallStage::Failed {
            reason: error.to_string(),
        });
        error.into()
    })
}

impl PluginService {
    /// A sender for [`fetch`]'s progress events (same channel as the service's own events).
    pub fn events(&self) -> UnboundedSender<ServiceEvent> {
        self.events.clone()
    }

    /// Verifies, unpacks, loads and starts a downloaded package. A plugin removed with its data
    /// kept gets that data back (FR-6.62). With "Turn off all plugins" on, the plugin is
    /// installed and starts once plugins are turned on again.
    pub fn install(
        &mut self,
        request: &InstallRequest,
        download: &Download,
    ) -> Result<(), ServiceError> {
        let id = request.entry.id.clone();
        let result = self.install_inner(request, download);
        let stage = match &result {
            Ok(()) => InstallStage::Done,
            Err(error) => InstallStage::Failed {
                reason: error.to_string(),
            },
        };
        self.emit(ServiceEvent::InstallProgress { id, stage });
        self.changed();
        result
    }

    fn install_inner(
        &mut self,
        request: &InstallRequest,
        download: &Download,
    ) -> Result<(), ServiceError> {
        if self.no_plugins {
            return Err(ServiceError::NoPlugins);
        }
        let id = &request.entry.id;
        if request.source == Source::Dev {
            return Err(ServiceError::NotDeveloper(id.clone()));
        }
        let previous = self.store().plugin(id)?;
        match &previous {
            Some(record) if record.removed_at.is_none() => {
                return Err(
                    if Origin::parse(&record.source).is_some_and(|o| o.source() == Source::Dev) {
                        ServiceError::IdTaken(id.clone())
                    } else {
                        ServiceError::AlreadyInstalled
                    },
                );
            }
            _ => {}
        }

        self.progress(id, InstallStage::Verifying);
        let verified = verify_package(
            &download.bytes,
            &request.entry,
            &download.signature,
            request.source,
        )?;
        let manifest = &verified.package.manifest;
        let version = manifest.version.to_string();
        let granted = allowed_grants(
            manifest,
            request.source,
            super::with_required(manifest, request.optional.iter().map(String::as_str)),
        );

        self.progress(id, InstallStage::Unpacking);
        let dir = verified.package.install(&self.plugins_dir)?;
        if let Err(error) = self.integrity.record(id, &version, &dir) {
            self.remove_files(id);
            return Err(error.into());
        }

        self.progress(id, InstallStage::Starting);
        if let Err(error) = self.host.load(&dir, None) {
            self.remove_files(id);
            return Err(error.into());
        }
        let record = PluginRecord {
            id: id.clone(),
            version,
            previous_version: None,
            source: Origin::from_source(request.source).to_db(),
            sha256: Some(verified.package.digest.sha256_hex()),
            author_key: Some(verified.author_key.clone()),
            enabled: true,
            disabled_reason: None,
            visibility: request.visibility,
            granted,
            pinned: false,
            notifications: true,
            removed_at: None,
            installed_at: now_millis(),
        };
        let saved = self.store().save_plugin(&record);
        if let Err(error) = saved {
            self.undo_install(id, previous.as_ref());
            return Err(error.into());
        }
        self.ctx.grants.set(id, &record.granted);
        self.ctx.notify.set_enabled(id, true);
        if !self.host.all_off()
            && let Err(error) = self.host.enable(id)
        {
            self.undo_install(id, previous.as_ref());
            return Err(error.into());
        }
        self.default_badge(id)?;
        Ok(())
    }

    /// Takes back a failed install: stops and forgets the plugin, deletes its files and puts
    /// the row back the way it was (a removed plugin keeps its kept data).
    fn undo_install(&mut self, id: &str, previous: Option<&PluginRecord>) {
        if let Err(error) = self.host.unload(id) {
            tracing::warn!(plugin = id, %error, "could not unload a failed install");
        }
        self.forget_runtime(id);
        self.remove_files(id);
        let restored = match previous {
            Some(record) => self.store().save_plugin(record).map_err(ServiceError::from),
            None => self.ctx.delete_plugin_data(id).map_err(ServiceError::from),
        };
        if let Err(error) = restored {
            tracing::warn!(plugin = id, %error, "could not undo a failed install");
        }
    }

    fn progress(&self, id: &str, stage: InstallStage) {
        self.emit(ServiceEvent::InstallProgress {
            id: id.to_owned(),
            stage,
        });
    }
}
