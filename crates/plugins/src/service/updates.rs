//! Plugin updates (FR-6.66–6.69) and the daily revocation check (FR-6.70).
//!
//! The "Plugin updates" setting is separate from the app updater: Install automatically
//! (default), Notify only, or Off. Updates are looked for at app start and every 6 hours while
//! running, never when Off; the revocation list is fetched once a day even when Off. An update
//! installs by itself only if it asks for the same or fewer permissions and is signed by the
//! same author key; otherwise it waits in the Updates tab. Pinned plugins are left alone. If an
//! updated plugin fails to start, the previous version comes back and the reason shows in the
//! Updates tab. Updates never touch a plugin's data.
//!
//! An update never grants anything new by itself: optional permissions a new version adds stay
//! off and are offered in Plugins → Updates instead, until the user answers (F39). The signed
//! index lists permissions without saying which are optional, so in automatic mode an update
//! with new permissions is downloaded and its manifest decides: only new optional ones →
//! installs and offers them; a new required one → waits for approval as before.
//!
//! [`run_check`] does all of this; the app calls it at start and then on a timer (every
//! 15 minutes is plenty; [`Schedule`] decides what is due).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use mumcord_store::{PluginRecord, PluginRepository, PluginUpdates, Settings, UnixMillis};
use semver::Version;

use super::{
    InstallStage, Origin, PluginService, ServiceError, ServiceEvent, allowed_grants, fetch,
    now_millis, with_required,
};
use crate::manifest::{Manifest, Source};
use crate::registry::{
    Client, Download, Freshness, Index, IndexPlugin, IndexVersion, RepoOutcome, fetch_official,
    fetch_repository, fetch_revoked, normalize_key, official_base,
};
use crate::verify::{IndexEntry, verify_package};

/// How often updates are looked for while Mumcord runs (FR-6.66).
pub const UPDATE_INTERVAL_MS: UnixMillis = 6 * 60 * 60 * 1000;
/// How often the revocation list is fetched (FR-6.70).
pub const REVOCATION_INTERVAL_MS: UnixMillis = 24 * 60 * 60 * 1000;
/// Debug builds only: `1` makes the daily revocation check due at start (AC-6.20).
const FORCE_DAILY_CHECK_ENV: &str = "MUMCORD_PLUGIN_TEST_FORCE_DAILY_CHECK";
/// `plugin_repository.paused_reason` after a repository changed its key (FR-6.75).
const KEY_CHANGED: &str = "key-changed";

/// Why an update waits for the user instead of installing by itself (FR-6.67).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    /// The new version asks for these permissions on top of what the installed one has.
    NewPermissions(Vec<String>),
    /// The new version is signed by a different author key.
    NewAuthorKey,
}

/// One available update, for the Updates tab.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub id: String,
    /// From the signed index, in the UI language. Untrusted text.
    pub name: String,
    /// The installed version.
    pub from: String,
    /// The version on offer.
    pub to: String,
    pub source: Source,
    /// What the package must match; used for the install.
    pub entry: IndexEntry,
    pub url: String,
    pub size: u64,
    /// `None`: may install without asking.
    pub approval: Option<Approval>,
    /// This version was installed, failed to start and was rolled back: the reason.
    pub failed: Option<String>,
}

/// What the service remembers about updates while it runs.
#[derive(Debug, Default)]
pub(super) struct Updates {
    available: BTreeMap<String, UpdateInfo>,
    /// id → (version, reason) of the last rolled-back update; not retried automatically.
    failed: BTreeMap<String, (String, String)>,
    /// (id, version) pairs already announced in the notification centre.
    notified: BTreeSet<(String, String)>,
    /// id → (version, approval) found in a downloaded package that may not install by itself;
    /// it is not downloaded again in this run.
    held: BTreeMap<String, (String, Approval)>,
}

/// What a check should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    pub updates: bool,
    pub revocations: bool,
}

/// Decides when updates and revocations are due. One per app run.
#[derive(Debug)]
pub struct Schedule {
    updates_checked_at: Option<UnixMillis>,
    /// The cached revocation list was applied once in this run.
    revocations_applied: bool,
    force_daily: bool,
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new()
    }
}

impl Schedule {
    pub fn new() -> Self {
        Self {
            updates_checked_at: None,
            revocations_applied: false,
            force_daily: cfg!(debug_assertions)
                && std::env::var(FORCE_DAILY_CHECK_ENV).is_ok_and(|v| v == "1"),
        }
    }

    /// Updates: at the first call and every 6 hours, not when Off. Revocations: when the last
    /// successful fetch is a day old (or in the future: the clock was changed), even when Off.
    pub fn due(&self, now: UnixMillis, settings: &Settings) -> Due {
        let older_than = |at: Option<UnixMillis>, interval: UnixMillis| {
            at.is_none_or(|at| now < at || now - at >= interval)
        };
        Due {
            updates: settings.plugins_updates != PluginUpdates::Off
                && older_than(self.updates_checked_at, UPDATE_INTERVAL_MS),
            revocations: self.force_daily
                || older_than(settings.plugins_revoked_checked_at, REVOCATION_INTERVAL_MS),
        }
    }

    fn updates_checked(&mut self, now: UnixMillis) {
        self.updates_checked_at = Some(now);
    }

    fn revocations_checked(&mut self) {
        self.force_daily = false;
    }
}

/// Runs whatever is due: fetches and applies the revocation list, looks for updates and
/// installs the ones that may install by themselves. Network work happens without the
/// service lock; the service is called on blocking threads. `registry_cache` holds one
/// offline copy per source (`official`, `repo-<id>`). Errors of single sources are logged and
/// do not stop the rest.
pub async fn run_check(
    client: &Client,
    service: &Arc<Mutex<PluginService>>,
    schedule: &mut Schedule,
    registry_cache: &Path,
) -> Result<(), ServiceError> {
    let (no_plugins, settings, repositories, events) = with_service(service, |s| {
        let store = s.store();
        Ok::<_, ServiceError>((
            s.no_plugins,
            store.settings()?,
            store.plugin_repositories()?,
            s.events.clone(),
        ))
    })
    .await??;
    if no_plugins {
        return Ok(());
    }
    let now = now_millis();
    let due = schedule.due(now, &settings);
    let official_dir = registry_cache.join("official");
    let base = official_base(settings.plugins_mirror_url.as_deref())?;

    if due.revocations {
        let loaded = fetch_revoked(client, &official_dir, &base).await?;
        let fetched = matches!(loaded.freshness, Freshness::Fresh | Freshness::NotModified);
        if let Freshness::Rejected(error) = &loaded.freshness {
            tracing::warn!(%error, "the downloaded revocation list was rejected");
        }
        with_service(service, move |s| {
            s.apply_revocations(loaded.list)?;
            if fetched {
                let store = s.store();
                let mut settings = store.settings()?;
                settings.plugins_revoked_checked_at = Some(now);
                store.save_settings(&settings)?;
            }
            Ok::<_, ServiceError>(())
        })
        .await??;
        schedule.revocations_checked();
        schedule.revocations_applied = true;
    } else if !schedule.revocations_applied {
        let list = super::cached_revocations(&official_dir);
        with_service(service, move |s| s.apply_revocations(list)).await??;
        schedule.revocations_applied = true;
    }

    if !due.updates {
        return Ok(());
    }
    schedule.updates_checked(now);
    let last_seen = u64::try_from(settings.plugins_last_official_sequence).ok();
    let mut indexes: Vec<(Source, Index)> = Vec::new();
    match fetch_official(client, &official_dir, &base, last_seen).await {
        Ok(loaded) => {
            if let Freshness::Rejected(error) = &loaded.freshness {
                tracing::warn!(%error, "the downloaded official index was rejected");
            }
            if matches!(loaded.freshness, Freshness::Fresh) {
                let sequence = i64::try_from(loaded.index.sequence).unwrap_or(i64::MAX);
                with_service(service, move |s| {
                    let store = s.store();
                    let mut settings = store.settings()?;
                    settings.plugins_last_official_sequence = sequence;
                    store.save_settings(&settings)
                })
                .await??;
            }
            indexes.push((Source::Official, loaded.index));
        }
        Err(error) => tracing::warn!(%error, "could not load the official index"),
    }

    let official_ids: BTreeSet<String> = indexes
        .iter()
        .flat_map(|(_, index)| index.plugins.iter().map(|p| p.id.clone()))
        .collect();
    let is_official_id = |id: &str| official_ids.contains(id);
    // Every switched-on repository, also those nothing is installed from yet: a key change
    // must be noticed before the user installs from it (FR-6.75).
    for repository in repositories.iter().filter(|r| r.enabled) {
        if let Some(index) =
            load_repository(client, service, registry_cache, repository, &is_official_id).await?
        {
            indexes.push((
                Source::Repo {
                    repository_id: repository.id,
                },
                index,
            ));
        }
    }

    let mode = settings.plugins_updates;
    let automatic = with_service(service, move |s| s.plan_updates(&indexes, mode)).await??;
    for update in automatic {
        let Ok(download) = fetch(client, &events, &update.id, &update.url, update.size).await
        else {
            // `fetch` already reported the failure as install progress.
            continue;
        };
        let id = update.id.clone();
        let applied = with_service(service, move |s| {
            s.apply_update(&update.id, &download, None)
        })
        .await?;
        if let Err(error) = applied {
            tracing::warn!(plugin = id.as_str(), %error, "automatic plugin update not applied");
        }
    }
    Ok(())
}

/// Loads a third-party repository's signed index (offline copy in `registry_cache/repo-<id>`)
/// and records the outcome: the sequence, and a rotated key, after a successful fetch; after a
/// key change the repository is paused and its plugins are turned off (FR-6.75). Returns the
/// index to use, or `None` when the repository could not be loaded at all (logged).
pub async fn load_repository(
    client: &Client,
    service: &Arc<Mutex<PluginService>>,
    registry_cache: &Path,
    repository: &PluginRepository,
    is_official_id: &(dyn Fn(&str) -> bool + Sync),
) -> Result<Option<Index>, ServiceError> {
    let id = repository.id;
    let dir = registry_cache.join(format!("repo-{id}"));
    let (loaded, new_key) = match fetch_repository(client, &dir, repository, is_official_id).await {
        Ok(RepoOutcome::Loaded(loaded)) => (loaded, None),
        Ok(RepoOutcome::Rotated { new_key, loaded }) => (loaded, Some(new_key)),
        Ok(RepoOutcome::KeyChanged { .. }) => {
            with_service(service, move |s| s.pause_repository(id)).await??;
            tracing::warn!(repository = id, "repository key changed; paused");
            return Ok(None);
        }
        Err(error) => {
            tracing::warn!(repository = id, %error, "could not load a repository");
            return Ok(None);
        }
    };
    if let Freshness::Rejected(error) = &loaded.freshness {
        tracing::warn!(repository = id, %error, "a repository index was rejected");
    }
    if matches!(loaded.freshness, Freshness::Fresh | Freshness::NotModified) {
        let sequence = i64::try_from(loaded.index.sequence).unwrap_or(i64::MAX);
        let now = now_millis();
        with_service(service, move |s| {
            let store = s.store();
            if let Some(key) = new_key {
                store.replace_plugin_repository_key(id, &key, now)?;
            }
            store.record_plugin_repository_check(id, sequence, now)
        })
        .await??;
    }
    Ok(Some(loaded.index))
}

/// Runs `f` with the service locked, on a blocking thread (loading and starting plugins block).
async fn with_service<T: Send + 'static>(
    service: &Arc<Mutex<PluginService>>,
    f: impl FnOnce(&mut PluginService) -> T + Send + 'static,
) -> Result<T, ServiceError> {
    let service = Arc::clone(service);
    tokio::task::spawn_blocking(move || {
        let mut service = service.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut service)
    })
    .await
    .map_err(|error| ServiceError::Io(std::io::Error::other(error)))
}

impl PluginService {
    /// Pauses a repository whose key changed and turns off every plugin installed from it,
    /// keeping their data (FR-6.75). The user re-enables it via the key-change review.
    pub fn pause_repository(&mut self, repository_id: i64) -> Result<(), ServiceError> {
        self.store()
            .pause_plugin_repository(repository_id, KEY_CHANGED)?;
        let source = Origin::Repo(repository_id).to_db();
        let ids: Vec<String> = self
            .store()
            .plugins()?
            .into_iter()
            .filter(|r| r.removed_at.is_none() && r.enabled && r.source == source)
            .map(|r| r.id)
            .collect();
        for id in ids {
            if let Err(error) = self.set_enabled(&id, false) {
                tracing::warn!(plugin = id.as_str(), %error, "turning off a paused repository's plugin failed");
            }
        }
        Ok(())
    }

    /// The updates on offer, for the Updates tab.
    pub fn updates(&self) -> Vec<UpdateInfo> {
        self.updates.available.values().cloned().collect()
    }

    /// Recomputes the available updates from signed indexes (one per source) and returns the
    /// ones to install without asking. Announces every newly available version that waits for
    /// the user in the notification centre (FR-6.69).
    pub fn plan_updates(
        &mut self,
        indexes: &[(Source, Index)],
        mode: PluginUpdates,
    ) -> Result<Vec<UpdateInfo>, ServiceError> {
        let mut available = BTreeMap::new();
        if mode != PluginUpdates::Off && !self.no_plugins {
            let records = self.store().plugins()?;
            for record in records {
                if record.removed_at.is_some() || record.pinned {
                    continue;
                }
                let Some(source) = Origin::parse(&record.source)
                    .filter(|o| !matches!(o, Origin::Dev(_)))
                    .map(|o| o.source())
                else {
                    continue;
                };
                let Some(plugin) = indexes
                    .iter()
                    .find(|(s, _)| *s == source)
                    .and_then(|(_, index)| index.plugin(&record.id))
                else {
                    continue;
                };
                if let Some(update) = self.update_for(&record, source, plugin) {
                    available.insert(record.id.clone(), update);
                }
            }
        }
        let automatic: Vec<UpdateInfo> = available
            .values()
            .filter(|u| mode == PluginUpdates::Auto && u.failed.is_none())
            .filter(|u| match &u.approval {
                None => true,
                // Maybe only new optional permissions: the package's manifest decides.
                Some(Approval::NewPermissions(_)) => self
                    .updates
                    .held
                    .get(&u.id)
                    .is_none_or(|(held, _)| *held != u.to),
                Some(Approval::NewAuthorKey) => false,
            })
            .cloned()
            .collect();
        for update in available.values() {
            let key = (update.id.clone(), update.to.clone());
            if !automatic.iter().any(|a| a.id == update.id) && self.updates.notified.insert(key) {
                self.emit(ServiceEvent::UpdateAvailable {
                    id: update.id.clone(),
                    name: update.name.clone(),
                    version: update.to.clone(),
                });
            }
        }
        self.updates.available = available;
        self.changed();
        Ok(automatic)
    }

    /// The newest version above the installed one that is not revoked, if any.
    fn update_for(
        &self,
        record: &PluginRecord,
        source: Source,
        plugin: &IndexPlugin,
    ) -> Option<UpdateInfo> {
        let installed = Version::parse(&record.version).ok()?;
        let mut newer: Vec<(Version, &IndexVersion)> = plugin
            .versions
            .iter()
            .filter_map(|v| Version::parse(&v.version).ok().map(|parsed| (parsed, v)))
            .filter(|(parsed, _)| *parsed > installed)
            .collect();
        newer.sort_by(|a, b| b.0.cmp(&a.0));
        let (_, version) = newer.into_iter().find(|(_, v)| {
            let sha256 = plugin.entry(v).digest.sha256_hex();
            !self.is_revoked(
                &record.id,
                &v.version,
                &sha256,
                record.author_key.as_deref(),
            )
        })?;

        let has = |permission: &str| match self.host.manifest(&record.id) {
            Some(manifest) => manifest.has_permission(permission),
            None => record.granted.iter().any(|g| g == permission),
        };
        let new_permissions: Vec<String> = version
            .permissions
            .iter()
            .filter(|p| !has(p))
            .cloned()
            .collect();
        let same_key = record
            .author_key
            .as_deref()
            .and_then(normalize_key)
            .is_some_and(|key| plugin.author_keys.contains(&key));
        let held = self
            .updates
            .held
            .get(&record.id)
            .filter(|(held, _)| *held == version.version)
            .map(|(_, approval)| approval.clone());
        let approval = if held.is_some() {
            held
        } else if !new_permissions.is_empty() {
            Some(Approval::NewPermissions(new_permissions))
        } else if !same_key {
            Some(Approval::NewAuthorKey)
        } else {
            None
        };
        let failed = self
            .updates
            .failed
            .get(&record.id)
            .filter(|(failed, _)| *failed == version.version)
            .map(|(_, reason)| reason.clone());
        Some(UpdateInfo {
            id: record.id.clone(),
            name: plugin.name(&self.language).to_owned(),
            from: record.version.clone(),
            to: version.version.clone(),
            source,
            entry: plugin.entry(version),
            url: version.url.clone(),
            size: version.size,
            approval,
            failed,
        })
    }

    /// Pins a plugin to its version (FR-6.67): a pinned plugin is not offered updates.
    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<(), ServiceError> {
        self.update_record(id, |r| r.pinned = pinned)?;
        if pinned {
            self.updates.available.remove(id);
        }
        self.changed();
        Ok(())
    }

    /// Installs the available update of `id` from its downloaded package (see [`fetch`]).
    /// `approved` holds the optional permissions the user switched on when they approved the
    /// update in the Updates tab; `None` is an automatic update, which only goes ahead with
    /// the same author key and no new required permission (else [`ServiceError::NeedsApproval`]
    /// and the update waits). Grants carry over. If the new version fails to start, the
    /// previous version is restored and the reason shows on the update (FR-6.68).
    pub fn apply_update(
        &mut self,
        id: &str,
        download: &Download,
        approved: Option<&[String]>,
    ) -> Result<(), ServiceError> {
        let update = self
            .updates
            .available
            .get(id)
            .cloned()
            .ok_or_else(|| ServiceError::NoUpdate(id.to_owned()))?;
        let result = self.update_inner(&update, download, approved);
        let stage = match &result {
            Ok(()) => InstallStage::Done,
            Err(error) => InstallStage::Failed {
                reason: error.to_string(),
            },
        };
        self.update_progress(id, stage);
        self.changed();
        result
    }

    fn update_inner(
        &mut self,
        update: &UpdateInfo,
        download: &Download,
        approved: Option<&[String]>,
    ) -> Result<(), ServiceError> {
        if self.no_plugins {
            return Err(ServiceError::NoPlugins);
        }
        let id = update.id.as_str();
        let old = self.record(id)?;
        if old.pinned {
            return Err(ServiceError::NoUpdate(id.to_owned()));
        }
        self.update_progress(id, InstallStage::Verifying);
        let verified = verify_package(
            &download.bytes,
            &update.entry,
            &download.signature,
            update.source,
        )?;
        let manifest = &verified.package.manifest;
        let granted = match approved {
            Some(optional) => allowed_grants(
                manifest,
                update.source,
                with_required(manifest, optional.iter().map(String::as_str)),
            ),
            None => {
                let same_key = old.author_key.as_deref().and_then(normalize_key)
                    == normalize_key(&verified.author_key);
                let missing: Vec<String> = manifest
                    .permissions
                    .required
                    .iter()
                    .filter(|p| !old.granted.contains(p))
                    .cloned()
                    .collect();
                let approval = if !same_key {
                    Some(Approval::NewAuthorKey)
                } else if !missing.is_empty() {
                    Some(Approval::NewPermissions(missing))
                } else {
                    None
                };
                if let Some(approval) = approval {
                    self.hold(update, approval);
                    return Err(ServiceError::NeedsApproval);
                }
                allowed_grants(
                    manifest,
                    update.source,
                    old.granted.iter().map(String::as_str),
                )
            }
        };
        let version = manifest.version.to_string();

        let offered = offered_permissions(
            self.host.manifest(id),
            manifest,
            update.source,
            &granted,
            &self.offered_permissions()?.remove(id).unwrap_or_default(),
        );

        self.update_progress(id, InstallStage::Unpacking);
        let dir = verified.package.install(&self.plugins_dir)?;
        if let Err(error) = self.integrity.record(id, &version, &dir) {
            self.remove_version(id, &version);
            return Err(error.into());
        }

        self.update_progress(id, InstallStage::Starting);
        let revoked = old.disabled_reason.as_deref() == Some("revoked");
        let record = PluginRecord {
            version: version.clone(),
            previous_version: Some(old.version.clone()),
            sha256: Some(verified.package.digest.sha256_hex()),
            author_key: Some(verified.author_key.clone()),
            granted,
            // A version that is not revoked may run again once the user turns it on.
            disabled_reason: if revoked {
                None
            } else {
                old.disabled_reason.clone()
            },
            ..old.clone()
        };
        self.unload(id);
        if let Err(error) = self.switch_to(&record, &dir, &old.version) {
            self.roll_back(&old, &version, &error.to_string());
            return Err(error);
        }
        self.updates.available.remove(id);
        self.updates.failed.remove(id);
        self.updates.held.remove(id);
        self.save_offer(id, offered)
    }

    /// Keeps an automatic update that turned out to need the user waiting: it is announced
    /// once and not downloaded again in this run.
    fn hold(&mut self, update: &UpdateInfo, approval: Approval) {
        if let Some(available) = self.updates.available.get_mut(&update.id) {
            available.approval = Some(approval.clone());
        }
        self.updates
            .held
            .insert(update.id.clone(), (update.to.clone(), approval));
        if self
            .updates
            .notified
            .insert((update.id.clone(), update.to.clone()))
        {
            self.emit(ServiceEvent::UpdateAvailable {
                id: update.id.clone(),
                name: update.name.clone(),
                version: update.to.clone(),
            });
        }
    }

    /// Loads, saves and (if on) starts the new version of an unloaded plugin.
    fn switch_to(
        &mut self,
        record: &PluginRecord,
        dir: &Path,
        previous: &str,
    ) -> Result<(), ServiceError> {
        let source = Origin::parse(&record.source)
            .ok_or_else(|| ServiceError::NotInstalled(record.id.clone()))?
            .source();
        self.host.load(dir, Version::parse(previous).ok())?;
        self.store().save_plugin(record)?;
        self.ctx.grants.set(&record.id, &record.granted);
        // Unloading the old version forgot its web folder; the new one may bring its own.
        self.set_webview_root(&record.id, source, dir, &record.granted);
        self.ctx
            .notify
            .set_enabled(&record.id, record.notifications);
        // Before the start, so a plugin may already message from its `start`.
        self.sync_peers();
        if record.enabled && !self.host.all_off() {
            self.host.enable(&record.id)?;
        }
        Ok(())
    }

    /// Puts the previous version back after a failed update and remembers why (FR-6.68).
    fn roll_back(&mut self, old: &PluginRecord, failed_version: &str, reason: &str) {
        tracing::warn!(
            plugin = old.id.as_str(),
            "update failed to start; rolling back"
        );
        self.unload(&old.id);
        if let Err(error) = self.store().save_plugin(old) {
            tracing::warn!(plugin = old.id.as_str(), %error, "could not restore the plugin row");
        }
        self.load_record(old);
        self.remove_version(&old.id, failed_version);
        self.updates.failed.insert(
            old.id.clone(),
            (failed_version.to_owned(), reason.to_owned()),
        );
        if let Some(available) = self.updates.available.get_mut(&old.id) {
            available.failed = Some(reason.to_owned());
        }
    }

    fn update_progress(&self, id: &str, stage: InstallStage) {
        self.emit(ServiceEvent::InstallProgress {
            id: id.to_owned(),
            stage,
        });
    }

    /// Deletes one installed version folder and its integrity entry.
    fn remove_version(&mut self, id: &str, version: &str) {
        if let Err(error) = self.integrity.forget(id, version) {
            tracing::warn!(plugin = id, %error, "could not update the integrity record");
        }
        let dir = self.version_dir(id, version);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(plugin = id, %error, "could not delete a plugin version"),
        }
    }
}

/// The optional permissions to offer after an update: those the new version asks for that the
/// installed one did not, plus ones still offered from an earlier update, minus what is
/// granted. Unknown old manifest (it could not be loaded): only the earlier offers carry over.
fn offered_permissions(
    old: Option<&Manifest>,
    new: &Manifest,
    source: Source,
    granted: &[String],
    earlier: &[String],
) -> Vec<String> {
    let candidates = new
        .permissions
        .optional
        .iter()
        .map(|p| p.id.as_str())
        .filter(|id| {
            !granted.iter().any(|g| g == id)
                && (earlier.iter().any(|e| e == id) || old.is_some_and(|m| !m.has_permission(id)))
        });
    allowed_grants(new, source, candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: UnixMillis = 60 * 60 * 1000;

    fn manifest(optional: &str) -> Manifest {
        let json = format!(
            r#"{{"manifest":1,"id":"poll","name":"Poll","version":"1.0.0","api":"^1.0","authors":["Tests"],
               "description":"Polls","categories":["tools"],"license":"MIT",
               "permissions":{{"required":["ui.panel"],"optional":[{optional}]}}}}"#
        );
        Manifest::parse(json.as_bytes()).expect("manifest")
    }

    fn strings(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn only_optional_permissions_new_to_the_update_are_offered() {
        let old = manifest(r#""notify""#);
        let new = manifest(
            r#""notify", {"id":"chat.triggers","suggested":true}, "events.voice", "secrets""#,
        );
        let granted = strings(&["ui.panel", "events.voice"]);
        // `notify` was there before (left off), `events.voice` is granted, `secrets` is
        // official-only: only `chat.triggers` is offered to a third-party plugin.
        let repo = Source::Repo { repository_id: 1 };
        assert_eq!(
            offered_permissions(Some(&old), &new, repo, &granted, &[]),
            ["chat.triggers"]
        );
        assert_eq!(
            offered_permissions(Some(&old), &new, Source::Official, &granted, &[]),
            ["chat.triggers", "secrets"]
        );
        // Unanswered offers carry over; an unknown old manifest offers nothing new.
        assert_eq!(
            offered_permissions(
                Some(&new),
                &new,
                repo,
                &granted,
                &strings(&["chat.triggers"])
            ),
            ["chat.triggers"]
        );
        assert!(offered_permissions(None, &new, repo, &granted, &[]).is_empty());
        // Granted meanwhile, or dropped by the new version: no longer offered.
        let all = strings(&["ui.panel", "chat.triggers"]);
        assert!(
            offered_permissions(Some(&new), &new, repo, &all, &strings(&["chat.triggers"]))
                .is_empty()
        );
        assert!(
            offered_permissions(
                Some(&old),
                &old,
                repo,
                &granted,
                &strings(&["chat.triggers"])
            )
            .is_empty()
        );
    }

    fn schedule() -> Schedule {
        Schedule {
            updates_checked_at: None,
            revocations_applied: false,
            force_daily: false,
        }
    }

    fn settings(mode: PluginUpdates, revoked_checked_at: Option<UnixMillis>) -> Settings {
        Settings {
            plugins_updates: mode,
            plugins_revoked_checked_at: revoked_checked_at,
            ..Settings::default()
        }
    }

    #[test]
    fn updates_at_start_and_every_six_hours_but_not_when_off() {
        let now = 100 * HOUR;
        let mut schedule = schedule();
        let auto = settings(PluginUpdates::Auto, Some(now));
        assert!(schedule.due(now, &auto).updates);
        schedule.updates_checked(now);
        assert!(!schedule.due(now + 5 * HOUR, &auto).updates);
        assert!(schedule.due(now + 6 * HOUR, &auto).updates);
        let notify = settings(PluginUpdates::Notify, Some(now));
        assert!(schedule.due(now + 6 * HOUR, &notify).updates);
        let off = settings(PluginUpdates::Off, Some(now));
        assert!(!schedule.due(now + 6 * HOUR, &off).updates);
    }

    #[test]
    fn revocations_once_a_day_even_when_off() {
        let now = 100 * HOUR;
        let schedule = schedule();
        assert!(
            schedule
                .due(now, &settings(PluginUpdates::Off, None))
                .revocations
        );
        let checked = settings(PluginUpdates::Off, Some(now - 23 * HOUR));
        assert!(!schedule.due(now, &checked).revocations);
        let old = settings(PluginUpdates::Off, Some(now - 24 * HOUR));
        assert!(schedule.due(now, &old).revocations);
        let future = settings(PluginUpdates::Off, Some(now + HOUR));
        assert!(schedule.due(now, &future).revocations);
    }

    #[test]
    fn the_test_hook_forces_one_daily_check() {
        let now = 100 * HOUR;
        let mut schedule = Schedule {
            force_daily: true,
            ..schedule()
        };
        let checked = settings(PluginUpdates::Off, Some(now));
        assert!(schedule.due(now, &checked).revocations);
        schedule.revocations_checked();
        assert!(!schedule.due(now, &checked).revocations);
    }
}
