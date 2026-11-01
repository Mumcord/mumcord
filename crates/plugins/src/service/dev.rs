//! Developer folders (FR-6.73): an unpacked plugin folder, unsigned, loaded only in Developer
//! mode. Such plugins are never official, never get official-only permissions (a manifest that
//! asks for one is refused), carry "Developer plugin" everywhere and are unloaded, together
//! with their data, when Developer mode is turned off.

use std::path::Path;

use mumcord_store::{PluginRecord, PluginVisibility};

use super::{Origin, PluginService, ServiceError, allowed_grants, now_millis, with_required};
use crate::manifest::{Manifest, ManifestError, Source, Visibility};
use crate::package::MANIFEST_FILE;

impl PluginService {
    /// Loads an unpacked plugin folder and starts it. Returns the plugin id. The id must not
    /// belong to an installed plugin (nor one removed with its data kept, whose data must not
    /// reach another plugin). Granted: the required and the suggested optional permissions.
    pub fn load_dev(&mut self, folder: &Path) -> Result<String, ServiceError> {
        self.check_developer_mode()?;
        let folder = folder.canonicalize()?;
        let manifest = read_dev_manifest(&folder)?;
        let id = manifest.id.clone();
        let existing = self.store().plugin(&id)?;
        if let Some(existing) = existing {
            return match Origin::parse(&existing.source) {
                Some(Origin::Dev(loaded)) if loaded == folder => {
                    self.reload_dev(&id)?;
                    Ok(id)
                }
                _ => Err(ServiceError::IdTaken(id)),
            };
        }

        let manifest = self.host.load(&folder, None)?;
        let suggested = manifest
            .permissions
            .optional
            .iter()
            .filter(|p| p.suggested)
            .map(|p| p.id.as_str());
        let granted = allowed_grants(manifest, Source::Dev, with_required(manifest, suggested));
        let visibility = match manifest.messaging.default_visibility {
            Some(Visibility::Friends) => PluginVisibility::Friends,
            Some(Visibility::Nobody) => PluginVisibility::Nobody,
            Some(Visibility::Everyone) | None => PluginVisibility::Everyone,
        };
        let record = PluginRecord {
            id: id.clone(),
            version: manifest.version.to_string(),
            previous_version: None,
            source: Origin::Dev(folder).to_db(),
            sha256: None,
            author_key: None,
            enabled: true,
            disabled_reason: None,
            visibility,
            granted,
            pinned: false,
            notifications: true,
            removed_at: None,
            installed_at: now_millis(),
        };
        let saved = self.store().save_plugin(&record);
        if let Err(error) = saved {
            self.unload(&id);
            return Err(error.into());
        }
        self.ctx.grants.set(&id, &record.granted);
        self.ctx.notify.set_enabled(&id, true);
        // A developer wants to see a failing start in the log, so the plugin stays loaded.
        if !self.host.all_off()
            && let Err(error) = self.host.enable(&id)
        {
            tracing::info!(plugin = id.as_str(), %error, "developer plugin did not start");
        }
        self.default_badge(&id)?;
        self.changed();
        Ok(id)
    }

    /// Loads a developer plugin's folder again (after the developer rebuilt it). Permissions it
    /// newly requires are granted; the on/off choice is kept.
    pub fn reload_dev(&mut self, id: &str) -> Result<(), ServiceError> {
        self.check_developer_mode()?;
        let record = self.record(id)?;
        let Some(Origin::Dev(folder)) = Origin::parse(&record.source) else {
            return Err(ServiceError::NotDeveloper(id.to_owned()));
        };
        self.unload(id);
        let result = read_dev_manifest(&folder).and_then(|manifest| {
            if manifest.id == id {
                Ok(manifest)
            } else {
                Err(ServiceError::IdTaken(manifest.id))
            }
        });
        let manifest = match result {
            Ok(manifest) => manifest,
            Err(error) => {
                self.load_errors.insert(id.to_owned(), error.to_string());
                self.changed();
                return Err(error);
            }
        };
        let granted = allowed_grants(
            &manifest,
            Source::Dev,
            with_required(&manifest, record.granted.iter().map(String::as_str)),
        );
        let record = self.update_record(id, |r| {
            r.version = manifest.version.to_string();
            r.granted = granted;
        })?;
        self.load_record(&record);
        self.changed();
        Ok(())
    }

    /// Unloads a developer plugin and deletes its data.
    pub fn unload_dev(&mut self, id: &str) -> Result<(), ServiceError> {
        let record = self.record(id)?;
        if !matches!(Origin::parse(&record.source), Some(Origin::Dev(_))) {
            return Err(ServiceError::NotDeveloper(id.to_owned()));
        }
        self.drop_dev(id)?;
        self.changed();
        Ok(())
    }

    /// Developer mode was switched (the settings command saved it): turning it off unloads
    /// every developer plugin.
    pub fn developer_mode_changed(&mut self, on: bool) -> Result<(), ServiceError> {
        if on {
            return Ok(());
        }
        let dev: Vec<String> = self
            .store()
            .plugins()?
            .into_iter()
            .filter(|r| matches!(Origin::parse(&r.source), Some(Origin::Dev(_))))
            .map(|r| r.id)
            .collect();
        for id in dev {
            self.drop_dev(&id)?;
        }
        self.changed();
        Ok(())
    }

    pub(super) fn drop_dev(&mut self, id: &str) -> Result<(), ServiceError> {
        self.unload(id);
        self.ctx.delete_plugin_data(id)?;
        self.forget_badge(id)
    }

    fn check_developer_mode(&self) -> Result<(), ServiceError> {
        if self.no_plugins {
            return Err(ServiceError::NoPlugins);
        }
        if self.store().settings()?.developer_mode {
            Ok(())
        } else {
            Err(ServiceError::DeveloperModeOff)
        }
    }
}

/// The folder's manifest with the developer-source rules applied.
fn read_dev_manifest(folder: &Path) -> Result<Manifest, ServiceError> {
    let path = folder.join(MANIFEST_FILE);
    if std::fs::metadata(&path)?.len() > crate::manifest::MAX_MANIFEST_BYTES as u64 {
        return Err(ManifestError::TooLarge.into());
    }
    let manifest = Manifest::parse(&std::fs::read(&path)?)?;
    manifest.validate(Source::Dev)?;
    Ok(manifest)
}
