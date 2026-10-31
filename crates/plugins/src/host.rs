//! Plugin lifecycle (#268): load from an installed package directory, enable, disable, unload.
//! A plugin that traps or runs out of time is stopped and its instance dropped; the rest of
//! Mumcord keeps running and [`Health`] decides when it may start again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use semver::Version;

use crate::health::{Health, Recovery};
use crate::manifest::{Manifest, ManifestError};
use crate::package::MANIFEST_FILE;
use crate::runtime::{CallError, CompileError, Compiled, Instance, Level, LogBuffer, Runtime};
use crate::version::{Incompatibility, check_compatibility};

/// Largest component file that is read from disk; matches the unpacked package limit.
const MAX_ENTRY_BYTES: u64 = crate::package::MAX_UNPACKED_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    /// Loaded and checked, not running.
    Disabled,
    Enabled,
    /// Stopped after a crash, a timeout or a refused start. `retry_at: None` means it stays off
    /// until the user turns it on again.
    Failed {
        error: CallError,
        retry_at: Option<Instant>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not read the plugin: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("{0}")]
    Incompatible(#[from] Incompatibility),
    #[error(transparent)]
    Compile(#[from] CompileError),
    #[error("a plugin with the id {0:?} is already loaded")]
    AlreadyLoaded(String),
    #[error("the plugin file is larger than allowed")]
    EntryTooLarge,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HostError {
    #[error("no plugin with the id {0:?} is loaded")]
    NotLoaded(String),
    #[error("the plugin {id:?} {error}")]
    Call { id: String, error: CallError },
}

struct Slot {
    dir: PathBuf,
    manifest: Manifest,
    compiled: Compiled,
    previous_version: Option<Version>,
    instance: Option<Instance>,
    state: PluginState,
    health: Health,
    log: Arc<Mutex<LogBuffer>>,
}

/// All loaded plugins. Calls run on the caller's thread; the app runs the host off the UI and
/// audio threads.
pub struct PluginHost {
    runtime: Runtime,
    app_version: Version,
    plugins: BTreeMap<String, Slot>,
}

impl PluginHost {
    pub fn new(app_version: Version) -> Result<Self, CompileError> {
        Ok(Self {
            runtime: Runtime::new()?,
            app_version,
            plugins: BTreeMap::new(),
        })
    }

    /// Loads an installed plugin directory: reads and checks the manifest, the version rules and
    /// the component's imports. Nothing of the plugin runs yet. `previous_version` is the version
    /// that was installed before an update; the plugin sees it at start.
    pub fn load(
        &mut self,
        dir: &Path,
        previous_version: Option<Version>,
    ) -> Result<&Manifest, LoadError> {
        let manifest_path = dir.join(MANIFEST_FILE);
        if std::fs::metadata(&manifest_path)?.len() > crate::manifest::MAX_MANIFEST_BYTES as u64 {
            return Err(ManifestError::TooLarge.into());
        }
        let manifest = Manifest::parse(&std::fs::read(&manifest_path)?)?;
        if self.plugins.contains_key(&manifest.id) {
            return Err(LoadError::AlreadyLoaded(manifest.id));
        }
        check_compatibility(&manifest, &self.app_version)?;

        let entry = dir.join(&manifest.entry);
        if std::fs::metadata(&entry)?.len() > MAX_ENTRY_BYTES {
            return Err(LoadError::EntryTooLarge);
        }
        let compiled = self.runtime.compile(&std::fs::read(&entry)?)?;

        let id = manifest.id.clone();
        let slot = Slot {
            dir: dir.to_owned(),
            manifest,
            compiled,
            previous_version,
            instance: None,
            state: PluginState::Disabled,
            health: Health::default(),
            log: Arc::default(),
        };
        Ok(&self.plugins.entry(id).or_insert(slot).manifest)
    }

    /// Starts a plugin. Also used for scheduled restarts after a failure; [`Self::turn_on_again`]
    /// is the user's explicit retry.
    pub fn enable(&mut self, id: &str) -> Result<(), HostError> {
        let runtime = &self.runtime;
        let slot = self.plugins.get_mut(id).ok_or_else(|| not_loaded(id))?;
        if slot.instance.is_some() {
            return Ok(());
        }
        let memory_bytes = slot.manifest.memory_mib as usize * 1024 * 1024;
        let previous = slot
            .previous_version
            .as_ref()
            .filter(|v| **v != slot.manifest.version)
            .map(Version::to_string);
        match runtime.start(
            &slot.compiled,
            memory_bytes,
            Arc::clone(&slot.log),
            previous,
        ) {
            Ok(instance) => {
                slot.instance = Some(instance);
                slot.state = PluginState::Enabled;
                Ok(())
            }
            Err(error) => Err(fail(id, slot, error)),
        }
    }

    /// The user turned a failed plugin on again: forget earlier failures, then start it.
    pub fn turn_on_again(&mut self, id: &str) -> Result<(), HostError> {
        self.slot_mut(id)?.health.reset();
        self.enable(id)
    }

    /// Stops a plugin; it stays loaded. A plugin that fails in `stop` is still stopped.
    pub fn disable(&mut self, id: &str) -> Result<(), HostError> {
        let slot = self.slot_mut(id)?;
        let result = match slot.instance.take() {
            Some(instance) => instance.stop(),
            None => Ok(()),
        };
        slot.state = PluginState::Disabled;
        result.map_err(|error| {
            tracing::warn!(plugin = id, %error, "plugin failed while stopping");
            HostError::Call {
                id: id.to_owned(),
                error,
            }
        })
    }

    /// Stops (if running) and forgets a plugin.
    pub fn unload(&mut self, id: &str) -> Result<(), HostError> {
        let stopped = self.disable(id);
        self.plugins.remove(id);
        match stopped {
            Err(HostError::NotLoaded(_)) => stopped,
            // A plugin that crashed while stopping is gone all the same.
            _ => Ok(()),
        }
    }

    pub fn state(&self, id: &str) -> Option<&PluginState> {
        self.plugins.get(id).map(|slot| &slot.state)
    }

    pub fn manifest(&self, id: &str) -> Option<&Manifest> {
        self.plugins.get(id).map(|slot| &slot.manifest)
    }

    /// The directory the plugin was loaded from (assets, translations).
    pub fn dir(&self, id: &str) -> Option<&Path> {
        self.plugins.get(id).map(|slot| slot.dir.as_path())
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.keys().map(String::as_str)
    }

    /// The plugin's own log lines, oldest first.
    pub fn log_lines(&self, id: &str) -> Vec<(Level, String)> {
        self.plugins.get(id).map_or_else(Vec::new, |slot| {
            let log = slot.log.lock().unwrap_or_else(PoisonError::into_inner);
            log.lines()
                .map(|(level, line)| (level, line.to_owned()))
                .collect()
        })
    }

    fn slot_mut(&mut self, id: &str) -> Result<&mut Slot, HostError> {
        self.plugins.get_mut(id).ok_or_else(|| not_loaded(id))
    }
}

/// Records a failure: the instance is already gone; decide when it may start again.
fn fail(id: &str, slot: &mut Slot, error: CallError) -> HostError {
    let retry_at = match slot.health.record_failure(Instant::now()) {
        Recovery::RetryAt(at) => Some(at),
        Recovery::StayOff => None,
    };
    tracing::warn!(plugin = id, %error, stays_off = retry_at.is_none(), "plugin stopped");
    slot.instance = None;
    slot.state = PluginState::Failed {
        error: error.clone(),
        retry_at,
    };
    HostError::Call {
        id: id.to_owned(),
        error,
    }
}

fn not_loaded(id: &str) -> HostError {
    HostError::NotLoaded(id.to_owned())
}
