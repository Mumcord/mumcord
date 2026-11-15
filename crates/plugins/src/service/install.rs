//! Installing from a signed index (FR-6.15, FR-6.29, FR-6.60, spec §6.3): download → verify →
//! unpack atomically → compile → start, with a progress event per step. A failed step leaves
//! nothing behind: no half-written folder, no row, no grants.
//!
//! Before the prompt, [`PluginService::install_check`] refuses hard stops and computes the score
//! and the confirmation gate (FR-9.78, FR-9.80, FR-9.81); `install` checks them again, so a
//! package can never get past the gate by skipping the prompt.

use mumcord_store::{PluginRecord, PluginVisibility};
use tokio::sync::mpsc::UnboundedSender;

use super::gates::{self, GateInput, InstallCheck, InstallGate};
use super::{Origin, PluginService, ServiceError, ServiceEvent, allowed_grants, now_millis};
use crate::manifest::{Manifest, Source};
use crate::registry::{Client, Download};
use crate::score::Scored;
use crate::verify::{IndexEntry, Verified, verify_package};

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
    /// The user ticked the prompt's confirmation box (FR-9.80); needed when the gate asks.
    pub confirmed: bool,
}

/// One step of an install, for the progress shown in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallStage {
    /// `received` of `total` package bytes (the size from the signed index).
    Downloading {
        received: u64,
        total: u64,
    },
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
/// a while; reports `Downloading` as the bytes arrive (at most once per whole percent, so the UI
/// is not flooded), and `Failed` if the download fails.
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
    progress(InstallStage::Downloading {
        received: 0,
        total: size,
    });
    let mut throttle = DownloadThrottle::new(size);
    let mut on_bytes = |received| {
        if throttle.should_report(received) {
            progress(InstallStage::Downloading {
                received,
                total: size,
            });
        }
    };
    client
        .download_with_progress(url, size, &mut on_bytes)
        .await
        .map_err(|error| {
            progress(InstallStage::Failed {
                reason: error.to_string(),
            });
            error.into()
        })
}

/// Lets a download report only when its whole percent changes.
struct DownloadThrottle {
    total: u64,
    last_percent: u64,
}

impl DownloadThrottle {
    fn new(total: u64) -> Self {
        Self {
            total,
            last_percent: 0,
        }
    }

    fn should_report(&mut self, received: u64) -> bool {
        let percent = received.saturating_mul(100) / self.total.max(1);
        if percent <= self.last_percent {
            return false;
        }
        self.last_percent = percent;
        true
    }
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
        if let Err(error) = &result {
            self.note_refused(&id, error);
        }
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
        let verified = self.verified(request.source, &request.entry, download)?;
        let manifest = &verified.package.manifest;
        self.refuse_unrunnable(manifest)?;
        let score = self.install_score(
            &request.source,
            &verified.package,
            download_size(download),
            &verified.author_key,
        )?;
        let gate = self.install_gate(request.source, manifest, &score.score)?;
        if gate.blocked_by_safety {
            return Err(ServiceError::BlockedBySafety);
        }
        if gate.confirm.is_some() && !request.confirmed {
            return Err(ServiceError::NeedsConfirmation);
        }
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
        self.set_origins(id, request.source);
        self.set_webview_root(id, request.source, &dir, &record.granted);
        self.ctx.notify.set_enabled(id, true);
        // Before the start, so a plugin may already message from its `start`.
        self.sync_peers();
        if !self.host.all_off()
            && let Err(error) = self.enable_plugin(id)
        {
            self.undo_install(id, previous.as_ref());
            return Err(error.into());
        }
        if gate.confirm == Some(gates::ConfirmKind::HighRisk) {
            // FR-9.83 compares later updates with what the user accepted here.
            self.store()
                .confirm_plugin_high_risk(id, &record.version, now_millis())?;
        }
        self.default_badge(id)?;
        Ok(())
    }

    /// What the install prompt shows for a downloaded package (FR-9.78, FR-9.80, FR-9.81): a
    /// hard stop refuses it without a score; otherwise its score and the confirmation gate.
    /// Nothing is unpacked or run. Other failures (a broken package, a newer Mumcord needed)
    /// are errors as before.
    pub fn install_check(
        &self,
        source: Source,
        entry: &IndexEntry,
        download: &Download,
    ) -> Result<InstallCheck, ServiceError> {
        let verified = match self.verified(source, entry, download) {
            Ok(verified) => verified,
            Err(error) => {
                self.note_refused(&entry.id, &error);
                return match gates::refusal_of(&error) {
                    Some(refusal) => Ok(InstallCheck::Refused { refusal }),
                    None => Err(error),
                };
            }
        };
        let manifest = &verified.package.manifest;
        self.refuse_unrunnable(manifest)?;
        let score = self.install_score(
            &source,
            &verified.package,
            download_size(download),
            &verified.author_key,
        )?;
        let gate = self.install_gate(source, manifest, &score.score)?;
        Ok(InstallCheck::Ready { score, gate })
    }

    /// Verifies a downloaded package (FR-6.50) and refuses a revoked one (FR-9.78).
    pub(super) fn verified(
        &self,
        source: Source,
        entry: &IndexEntry,
        download: &Download,
    ) -> Result<Verified, ServiceError> {
        let verified = verify_package(&download.bytes, entry, &download.signature, source)?;
        let manifest = &verified.package.manifest;
        if let Some(revocation) = self.revoked.find(
            &manifest.id,
            &manifest.version.to_string(),
            &verified.package.digest.sha256_hex(),
            Some(&verified.author_key),
        ) {
            return Err(ServiceError::Revoked(
                revocation.reason(&self.language).to_owned(),
            ));
        }
        Ok(verified)
    }

    /// The confirmation box and safety setting for installing `manifest` (FR-9.80, FR-9.81).
    fn install_gate(
        &self,
        source: Source,
        manifest: &Manifest,
        scored: &Scored,
    ) -> Result<InstallGate, ServiceError> {
        let store = self.store();
        let block_high_risk = store.settings()?.plugins.block_high_risk;
        let first_from_repository = match source {
            Source::Repo { repository_id } => !store.plugins()?.iter().any(|record| {
                record.removed_at.is_none()
                    && Origin::parse(&record.source) == Some(Origin::Repo(repository_id))
            }),
            Source::Official | Source::Dev => false,
        };
        Ok(gates::install_gate(GateInput {
            source,
            scored,
            own_code_and_websites: manifest
                .all_permissions()
                .any(|p| gates::OWN_CODE_AND_WEBSITES.contains(&p)),
            first_from_repository,
            block_high_risk,
        }))
    }

    /// FR-9.17: refuses a package this Mumcord cannot run. Runs on the opened package, before
    /// anything is written to disk.
    pub(super) fn refuse_unrunnable(&self, manifest: &Manifest) -> Result<(), ServiceError> {
        match crate::version::refusal(manifest, self.host.app_version()) {
            Some(text) => Err(ServiceError::Incompatible(text)),
            None => Ok(()),
        }
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

/// The `.mcplugin` size the score counts.
fn download_size(download: &Download) -> u64 {
    u64::try_from(download.bytes.len()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::DownloadThrottle;

    #[test]
    fn download_progress_is_reported_once_per_percent() {
        let mut throttle = DownloadThrottle::new(23_000_000);
        let reported = (1..=23_000_u64)
            .map(|chunk| chunk * 1_000)
            .filter(|&received| throttle.should_report(received))
            .count();
        assert_eq!(reported, 100);
    }

    #[test]
    fn a_tiny_download_reports_its_end() {
        let mut throttle = DownloadThrottle::new(10);
        assert!(throttle.should_report(10));
        assert!(!throttle.should_report(10));
    }

    #[test]
    fn an_empty_size_does_not_divide_by_zero() {
        let mut throttle = DownloadThrottle::new(0);
        assert!(!throttle.should_report(0));
    }
}
