//! Start-up, enable/disable, permissions, visibility, remove, reset, export and "Turn off all
//! plugins" (FR-6.6, FR-6.30, FR-6.62, spec §7.2).

use std::path::PathBuf;

use mumcord_store::{PluginRecord, PluginVisibility};
use semver::Version;

use super::{Origin, PluginService, ServiceError, allowed_grants, now_millis, settings_event};
use crate::grants::revokes_required;
use crate::host::HostError;
use crate::manifest::is_valid_id;

impl PluginService {
    /// Loads the installed plugins and starts the ones that are on. Also purges data kept for
    /// more than 30 days, re-checks the installed files once per app version (tampered ones
    /// stay off) and deletes version folders no longer needed. With `--no-plugins` nothing is
    /// loaded.
    pub fn start(&mut self) -> Result<(), ServiceError> {
        match self.ctx.purge_removed_plugins(now_millis()) {
            Ok(purged) => purged.iter().for_each(|id| self.remove_files(id)),
            Err(error) => tracing::warn!(%error, "could not purge removed plugins"),
        }
        if self.no_plugins {
            self.changed();
            return Ok(());
        }
        let settings = self.store().settings()?;
        self.host.set_all_off(settings.plugins_all_off);

        let mut records = Vec::new();
        let stored = self.store().plugins()?;
        for record in stored {
            let dev = matches!(Origin::parse(&record.source), Some(Origin::Dev(_)));
            if dev && !settings.developer_mode {
                // Developer mode was turned off while Mumcord was closed.
                self.drop_dev(&record.id)?;
            } else {
                records.push(record);
            }
        }
        self.check_integrity(&mut records)?;
        for record in &records {
            self.load_record(record);
        }
        self.clean_folders(&records);
        self.changed();
        Ok(())
    }

    /// Turns a plugin on or off (spec §7.2: "user enables" / "user disables"). Turning on also
    /// clears an auto-off, a revocation needs B17b's rules and is not cleared here.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), ServiceError> {
        if self.no_plugins {
            return Err(ServiceError::NoPlugins);
        }
        let record = self.record(id)?;
        if enabled {
            if self.host.all_off() {
                return Err(ServiceError::AllOff);
            }
            let manifest = self.loaded_manifest(id)?;
            if revokes_required(&manifest.permissions.required, &record.granted) {
                return Err(ServiceError::RequiredMissing);
            }
            self.update_record(id, |r| {
                r.enabled = true;
                r.disabled_reason = None;
            })?;
            self.changed();
            self.host.enable(id)?;
        } else {
            self.update_record(id, |r| {
                r.enabled = false;
                r.disabled_reason = Some("user".into());
            })?;
            self.stop(id);
            self.changed();
        }
        Ok(())
    }

    /// Replaces the granted permissions (FR-6.30): a revoked optional permission applies to the
    /// next call and event; a missing required one turns the plugin off with reason
    /// `permission`. Permissions the manifest does not ask for, and official-only ones for
    /// plugins that are not official, are ignored.
    pub fn set_permissions(&mut self, id: &str, granted: &[String]) -> Result<(), ServiceError> {
        let record = self.record(id)?;
        let source = Origin::parse(&record.source)
            .ok_or_else(|| ServiceError::NotInstalled(id.to_owned()))?
            .source();
        let manifest = self.loaded_manifest(id)?;
        let granted = allowed_grants(manifest, source, granted.iter().map(String::as_str));
        let missing = revokes_required(&manifest.permissions.required, &granted);
        let record = self.update_record(id, |r| {
            r.granted.clone_from(&granted);
            if missing && r.enabled {
                r.enabled = false;
                r.disabled_reason = Some("permission".into());
            }
        })?;
        self.ctx.grants.set(id, &record.granted);
        if missing {
            self.stop(id);
        } else {
            self.host.send(id, settings_event(&record));
        }
        self.changed();
        Ok(())
    }

    /// Who may see that the user runs this plugin (FR-6.33).
    pub fn set_visibility(
        &mut self,
        id: &str,
        visibility: PluginVisibility,
    ) -> Result<(), ServiceError> {
        let record = self.update_record(id, |r| r.visibility = visibility)?;
        self.host.send(id, settings_event(&record));
        self.changed();
        Ok(())
    }

    /// The plugin's "notifications" switch (FR-6.62).
    pub fn set_notifications(&mut self, id: &str, on: bool) -> Result<(), ServiceError> {
        self.update_record(id, |r| r.notifications = on)?;
        self.ctx.notify.set_enabled(id, on);
        self.changed();
        Ok(())
    }

    /// Removes a plugin (FR-6.62). Without `delete_data` its storage stays for 30 days and a
    /// reinstall restores it; with it, storage and secrets go now. A developer plugin is
    /// unloaded together with its data.
    pub fn remove(&mut self, id: &str, delete_data: bool) -> Result<(), ServiceError> {
        let record = self.record(id)?;
        if matches!(Origin::parse(&record.source), Some(Origin::Dev(_))) {
            return self.unload_dev(id);
        }
        self.unload(id);
        self.remove_files(id);
        if delete_data {
            self.ctx.delete_plugin_data(id)?;
        } else {
            self.store().mark_plugin_removed(id, now_millis())?;
        }
        self.forget_badge(id)?;
        self.changed();
        Ok(())
    }

    /// "Reset data": deletes the plugin's storage and secrets. A running plugin is restarted so
    /// it starts from the empty state.
    pub fn reset_data(&mut self, id: &str) -> Result<(), ServiceError> {
        self.record(id)?;
        self.ctx.reset_plugin_data(id)?;
        if self.host.state(id) == Some(crate::RunState::Running) {
            self.stop(id);
            self.host.enable(id)?;
        }
        self.changed();
        Ok(())
    }

    /// "Export data": the JSON the app writes to the file the user chose. Never secrets.
    pub fn export_data(&self, id: &str) -> Result<String, ServiceError> {
        self.record(id)?;
        Ok(self.ctx.export_plugin_data(id)?)
    }

    /// "Turn off all plugins" (FR-6.6): stops every plugin now and keeps them off across
    /// restarts. Turning it off again starts the plugins the user had on.
    pub fn set_all_off(&mut self, all_off: bool) -> Result<(), ServiceError> {
        {
            let store = self.store();
            let mut settings = store.settings()?;
            settings.plugins_all_off = all_off;
            store.save_settings(&settings)?;
        }
        if !self.no_plugins {
            self.host.set_all_off(all_off);
            if !all_off {
                let stored = self.store().plugins()?;
                for record in stored {
                    self.start_if_on(&record);
                }
            }
        }
        self.changed();
        Ok(())
    }

    /// Whether "Turn off all plugins" is on (persisted setting).
    pub fn all_off(&self) -> Result<bool, ServiceError> {
        Ok(self.store().settings()?.plugins_all_off)
    }

    /// Loads one stored plugin and starts it if it is on. Failures are kept for the plugin
    /// list; they never stop the other plugins.
    pub(super) fn load_record(&mut self, record: &PluginRecord) {
        self.load_errors.remove(&record.id);
        if let Err(error) = self.try_load(record) {
            tracing::warn!(plugin = record.id.as_str(), %error, "could not load a plugin");
            self.load_errors
                .insert(record.id.clone(), error.to_string());
            return;
        }
        self.start_if_on(record);
    }

    fn try_load(&mut self, record: &PluginRecord) -> Result<(), ServiceError> {
        let origin = Origin::parse(&record.source)
            .ok_or_else(|| ServiceError::NotInstalled(record.id.clone()))?;
        let dir = match &origin {
            Origin::Dev(folder) => folder.clone(),
            _ => self.version_dir(&record.id, &record.version),
        };
        let previous = record
            .previous_version
            .as_deref()
            .and_then(|v| Version::parse(v).ok());
        let manifest = self.host.load(&dir, previous)?;
        // The folder must still hold the plugin the row is about, with the source's rules.
        let checked = if manifest.id == record.id {
            manifest
                .validate(origin.source())
                .map_err(ServiceError::from)
        } else {
            Err(ServiceError::IdTaken(manifest.id.clone()))
        };
        let loaded_id = manifest.id.clone();
        if let Err(error) = checked {
            let _ = self.host.unload(&loaded_id);
            return Err(error);
        }
        let granted = allowed_grants(
            manifest,
            origin.source(),
            record.granted.iter().map(String::as_str),
        );
        self.ctx.grants.set(&record.id, granted);
        self.ctx
            .notify
            .set_enabled(&record.id, record.notifications);
        Ok(())
    }

    /// Starts a loaded plugin the user has on, unless Turn off all is on. A failing start backs
    /// off by itself and shows in the plugin list.
    fn start_if_on(&mut self, record: &PluginRecord) {
        if !record.enabled || self.host.all_off() || self.host.manifest(&record.id).is_none() {
            return;
        }
        if let Err(error) = self.host.enable(&record.id) {
            tracing::warn!(plugin = record.id.as_str(), %error, "plugin did not start");
        }
    }

    /// Stops a plugin; it stays loaded.
    pub(super) fn stop(&mut self, id: &str) {
        // Failures while stopping are logged by the host; the plugin is off either way.
        let _ = self.host.disable(id);
    }

    /// Stops and forgets a plugin in the runtime and the permission gate.
    pub(super) fn unload(&mut self, id: &str) {
        if let Err(error) = self.host.unload(id)
            && !matches!(error, HostError::NotLoaded(_))
        {
            tracing::warn!(plugin = id, %error, "plugin failed while unloading");
        }
        self.load_errors.remove(id);
        self.forget_runtime(id);
    }

    /// Drops what the host API keeps per plugin.
    pub(super) fn forget_runtime(&self, id: &str) {
        self.ctx.grants.forget(id);
        self.ctx.notify.forget(id);
    }

    fn loaded_manifest(&self, id: &str) -> Result<&crate::Manifest, ServiceError> {
        self.host
            .manifest(id)
            .ok_or_else(|| ServiceError::NotInstalled(id.to_owned()))
    }

    pub(super) fn version_dir(&self, id: &str, version: &str) -> PathBuf {
        self.plugins_dir.join(id).join(version)
    }

    /// Re-hashes the installed files once per app version; changed ones are turned off with
    /// reason `tampered`.
    fn check_integrity(&mut self, records: &mut [PluginRecord]) -> Result<(), ServiceError> {
        let dirs: Vec<(String, String, PathBuf)> = records
            .iter()
            .filter(|r| !matches!(Origin::parse(&r.source), Some(Origin::Dev(_))))
            .map(|r| {
                (
                    r.id.clone(),
                    r.version.clone(),
                    self.version_dir(&r.id, &r.version),
                )
            })
            .collect();
        let installed: Vec<(&str, &str, &std::path::Path)> = dirs
            .iter()
            .map(|(id, version, dir)| (id.as_str(), version.as_str(), dir.as_path()))
            .collect();
        let tampered = match self.integrity.recheck(&self.integrity_version, &installed) {
            Ok(tampered) => tampered,
            Err(error) => {
                tracing::warn!(%error, "could not re-check the installed plugins");
                return Ok(());
            }
        };
        for record in records.iter_mut().filter(|r| tampered.contains(&r.id)) {
            record.enabled = false;
            record.disabled_reason = Some("tampered".into());
            self.store().save_plugin(record)?;
        }
        Ok(())
    }

    /// Deletes everything under `<plugins>/<id>/` and its integrity entries.
    pub(super) fn remove_files(&mut self, id: &str) {
        if !is_valid_id(id) {
            return;
        }
        let dir = self.plugins_dir.join(id);
        for version in version_folders(&dir) {
            if let Err(error) = self.integrity.forget(id, &version) {
                tracing::warn!(plugin = id, %error, "could not update the integrity record");
            }
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(plugin = id, %error, "could not delete plugin files"),
        }
    }

    /// Deletes version folders nobody needs: everything but the active and the previous
    /// version (kept for rollback, FR-6.68), leftovers of interrupted installs, and folders of
    /// plugins that are no longer installed.
    fn clean_folders(&mut self, records: &[PluginRecord]) {
        let Ok(entries) = std::fs::read_dir(&self.plugins_dir) else {
            return;
        };
        let folders: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| is_valid_id(name))
            .collect();
        for id in folders {
            let Some(record) = records
                .iter()
                .find(|r| r.id == id && !matches!(Origin::parse(&r.source), Some(Origin::Dev(_))))
            else {
                self.remove_files(&id);
                continue;
            };
            let dir = self.plugins_dir.join(&id);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == record.version || record.previous_version.as_deref() == Some(&name) {
                    continue;
                }
                if !name.starts_with('.')
                    && let Err(error) = self.integrity.forget(&id, &name)
                {
                    tracing::warn!(plugin = id.as_str(), %error, "could not update the integrity record");
                }
                let path = entry.path();
                let removed = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                if let Err(error) = removed {
                    tracing::warn!(plugin = id.as_str(), %error, "could not delete an old plugin version");
                }
            }
        }
    }
}

/// The version folders under `<plugins>/<id>/` (not the hidden install leftovers).
fn version_folders(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| !name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default()
}
