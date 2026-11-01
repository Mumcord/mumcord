//! Plugin lifecycle (#268, FR-6.2–6.6): load from an installed package directory, enable,
//! disable, unload. Every running plugin has its own OS thread `mumcord-plugin-<id>` that owns
//! its store and works through its event queue. A plugin that traps, runs out of time or memory
//! is stopped on that thread and restarted after a back-off ([`Health`]); the rest of Mumcord
//! keeps running. The service hears about starts and failures through [`HostEvent`]s.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use semver::Version;
use tokio::sync::mpsc::UnboundedSender;

use crate::diag::Diagnostics;
use crate::health::{Health, Recovery, backoff_steps};
use crate::manifest::{Manifest, ManifestError};
use crate::package::MANIFEST_FILE;
use crate::runtime::wit::events::Event;
use crate::runtime::{
    CallError, CompileError, Compiled, EventQueue, HostContext, Instance, Level, LogBuffer, Next,
    Runtime, StartParams,
};
use crate::version::{Incompatibility, check_compatibility};

/// Largest component file that is read from disk; matches the unpacked package limit.
const MAX_ENTRY_BYTES: u64 = crate::package::MAX_UNPACKED_BYTES;

/// What the host tells the plugin service. Host API tasks add variants for what plugins ask
/// for (UI trees, notifications, messages …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// The plugin started (also after a scheduled restart): send it the current settings,
    /// language and theme.
    Started { id: String },
    /// The plugin failed and was stopped. `Recovery::StayOff` means it was turned off after
    /// repeated crashes ("Turned off after repeated crashes" + notification-centre entry).
    Failed {
        id: String,
        error: CallError,
        recovery: Recovery,
    },
}

/// The runtime part of the plugin lifecycle (spec §7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// Loaded and checked, not running.
    Disabled,
    Running,
    /// Stopped after a failure; starts again by itself at `retry_at`.
    BackingOff {
        error: CallError,
        retry_at: Instant,
    },
    /// Turned off after repeated crashes; stays off until the user turns it on again.
    AutoOff {
        error: CallError,
    },
    /// "Turn off all plugins" is on.
    AllOff,
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
    #[error("all plugins are turned off")]
    AllOff,
    #[error("the plugin thread failed: {0}")]
    Thread(String),
}

struct Slot {
    dir: PathBuf,
    manifest: Manifest,
    compiled: Compiled,
    previous_version: Option<Version>,
    state: Arc<Mutex<RunState>>,
    log: Arc<Mutex<LogBuffer>>,
    diag: Arc<Diagnostics>,
    worker: Option<Worker>,
}

/// The thread of a running (or backing-off) plugin. It ends on stop or auto-off.
struct Worker {
    queue: Arc<EventQueue>,
    thread: JoinHandle<Result<(), CallError>>,
}

/// All loaded plugins. Loading compiles on the caller's thread and `enable` waits for the
/// plugin's `start` (up to 500 ms), so the app calls these off the UI and audio threads.
pub struct PluginHost {
    runtime: Runtime,
    app_version: Version,
    ctx: Arc<HostContext>,
    outbox: UnboundedSender<HostEvent>,
    backoff: [Duration; 3],
    all_off: bool,
    plugins: BTreeMap<String, Slot>,
}

impl PluginHost {
    /// `cache_dir` is the app's cache folder for compiled plugins (`None`: no cache); `outbox`
    /// receives [`HostEvent`]s from every plugin thread.
    pub fn new(
        app_version: Version,
        cache_dir: Option<PathBuf>,
        ctx: Arc<HostContext>,
        outbox: UnboundedSender<HostEvent>,
    ) -> Result<Self, CompileError> {
        Ok(Self {
            runtime: Runtime::new(app_version.clone(), cache_dir)?,
            app_version,
            ctx,
            outbox,
            backoff: backoff_steps(),
            all_off: false,
            plugins: BTreeMap::new(),
        })
    }

    /// Replaces the restart back-off (5 s, 30 s, 5 min); for tests. Applies from the next
    /// `enable`.
    pub fn set_backoff(&mut self, steps: [Duration; 3]) {
        self.backoff = steps;
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

        let entry = dir.join(crate::package::WASM_FILE);
        if std::fs::metadata(&entry)?.len() > MAX_ENTRY_BYTES {
            return Err(LoadError::EntryTooLarge);
        }
        let compiled = self.runtime.compile(&std::fs::read(&entry)?)?;

        let id = manifest.id.clone();
        let state = if self.all_off {
            RunState::AllOff
        } else {
            RunState::Disabled
        };
        let slot = Slot {
            dir: dir.to_owned(),
            manifest,
            compiled,
            previous_version,
            state: Arc::new(Mutex::new(state)),
            log: Arc::default(),
            diag: Arc::default(),
            worker: None,
        };
        Ok(&self.plugins.entry(id).or_insert(slot).manifest)
    }

    /// Starts a plugin on its own thread and waits for its `start`. Also the user's "turn on
    /// again" after an auto-off: every enable starts counting failures from zero. If `start`
    /// fails, the error is returned and the plugin backs off and retries on its own.
    pub fn enable(&mut self, id: &str) -> Result<(), HostError> {
        if self.all_off {
            return Err(HostError::AllOff);
        }
        let backoff = self.backoff;
        let ctx = Arc::clone(&self.ctx);
        let outbox = self.outbox.clone();
        let slot = self.plugins.get_mut(id).ok_or_else(|| not_loaded(id))?;
        if slot
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.thread.is_finished())
        {
            return Ok(());
        }
        if let Some(worker) = slot.worker.take() {
            // An auto-off thread already ended; only its panic would be news.
            if worker.thread.join().is_err() {
                tracing::error!(plugin = id, "plugin thread panicked");
            }
        }

        let params = StartParams {
            id: Arc::from(id),
            memory_bytes: slot.manifest.memory_mib as usize * 1024 * 1024,
            previous_version: slot
                .previous_version
                .as_ref()
                .filter(|v| **v != slot.manifest.version)
                .map(Version::to_string),
            ctx,
            outbox,
            log: Arc::clone(&slot.log),
            diag: Arc::clone(&slot.diag),
        };
        let queue = Arc::new(EventQueue::new(Arc::clone(&slot.diag)));
        let (reply, started) = sync_channel(1);
        let thread = {
            let compiled = slot.compiled.clone();
            let queue = Arc::clone(&queue);
            let state = Arc::clone(&slot.state);
            std::thread::Builder::new()
                .name(format!("mumcord-plugin-{id}"))
                .spawn(move || run(&compiled, &params, &queue, &state, backoff, reply))
                .map_err(|e| HostError::Thread(e.to_string()))?
        };
        slot.worker = Some(Worker { queue, thread });
        match started.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(HostError::Call {
                id: id.to_owned(),
                error,
            }),
            Err(_) => Err(HostError::Thread("ended before the plugin started".into())),
        }
    }

    /// Stops a plugin (also one that is backing off); it stays loaded. A plugin that fails in
    /// `stop` is still stopped.
    pub fn disable(&mut self, id: &str) -> Result<(), HostError> {
        let off_state = if self.all_off {
            RunState::AllOff
        } else {
            RunState::Disabled
        };
        let slot = self.slot_mut(id)?;
        let result = slot
            .worker
            .take()
            .map_or(Ok(()), |worker| stop_worker(id, worker));
        *lock(&slot.state) = off_state;
        if let Err(error) = &result {
            tracing::warn!(plugin = id, %error, "plugin failed while stopping");
        }
        result
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

    /// "Turn off all plugins" (FR-6.6): stops every plugin now and refuses to start any until
    /// it is switched back. Turning it back on starts nothing; the service enables the plugins
    /// the user had on. Keeping the switch across restarts is the service's job.
    pub fn set_all_off(&mut self, all_off: bool) {
        self.all_off = all_off;
        let ids: Vec<String> = self.plugins.keys().cloned().collect();
        for id in ids {
            if all_off {
                // Failures while stopping are already logged; the plugin is off either way.
                let _ = self.disable(&id);
            } else if let Some(slot) = self.plugins.get(&id) {
                let mut state = lock(&slot.state);
                if *state == RunState::AllOff {
                    *state = RunState::Disabled;
                }
            }
        }
    }

    pub fn all_off(&self) -> bool {
        self.all_off
    }

    /// Queues an event for one plugin. Returns false when it is not running; a plugin that is
    /// backing off counts the event as dropped.
    pub fn send(&self, id: &str, event: Event) -> bool {
        self.plugins
            .get(id)
            .and_then(|slot| slot.worker.as_ref())
            .is_some_and(|worker| worker.queue.push(event))
    }

    /// Queues an event for every running plugin. Permission filtering happens before this.
    pub fn broadcast(&self, event: &Event) {
        for worker in self
            .plugins
            .values()
            .filter_map(|slot| slot.worker.as_ref())
        {
            worker.queue.push(event.clone());
        }
    }

    pub fn state(&self, id: &str) -> Option<RunState> {
        self.plugins.get(id).map(|slot| lock(&slot.state).clone())
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

    /// The plugin's diagnostics counters; they survive restarts while the plugin stays loaded.
    pub fn diagnostics(&self, id: &str) -> Option<Arc<Diagnostics>> {
        self.plugins.get(id).map(|slot| Arc::clone(&slot.diag))
    }

    /// The plugin's own log lines, oldest first.
    pub fn log_lines(&self, id: &str) -> Vec<(Level, String)> {
        self.plugins.get(id).map_or_else(Vec::new, |slot| {
            lock(&slot.log)
                .lines()
                .map(|(level, line)| (level, line.to_owned()))
                .collect()
        })
    }

    fn slot_mut(&mut self, id: &str) -> Result<&mut Slot, HostError> {
        self.plugins.get_mut(id).ok_or_else(|| not_loaded(id))
    }
}

impl Drop for PluginHost {
    /// Plugin threads must end before the runtime's clock stops, or a plugin stuck in a loop
    /// would never be interrupted.
    fn drop(&mut self) {
        for (id, slot) in &mut self.plugins {
            if let Some(worker) = slot.worker.take() {
                let _ = stop_worker(id, worker);
            }
        }
    }
}

fn stop_worker(id: &str, worker: Worker) -> Result<(), HostError> {
    worker.queue.request_stop();
    match worker.thread.join() {
        Ok(result) => result.map_err(|error| HostError::Call {
            id: id.to_owned(),
            error,
        }),
        Err(_) => Err(HostError::Thread("panicked".into())),
    }
}

/// The plugin thread: start, deliver events until told to stop, and on a failure back off and
/// start again until [`Health`] says to stay off. Returns the result of the plugin's `stop`.
fn run(
    compiled: &Compiled,
    params: &StartParams,
    queue: &EventQueue,
    state: &Mutex<RunState>,
    backoff: [Duration; 3],
    reply: SyncSender<Result<(), CallError>>,
) -> Result<(), CallError> {
    let mut health = Health::new(backoff);
    let mut reply = Some(reply);
    loop {
        let error = match compiled.start(params) {
            Ok(mut instance) => {
                *lock(state) = RunState::Running;
                let _ = params.outbox.send(HostEvent::Started {
                    id: params.id.to_string(),
                });
                if let Some(reply) = reply.take() {
                    let _ = reply.send(Ok(()));
                }
                match serve(&mut instance, queue) {
                    None => return instance.stop(),
                    Some(error) => error,
                }
            }
            Err(error) => error,
        };

        queue.set_accepting(false);
        let recovery = health.record_failure(Instant::now());
        tracing::warn!(
            plugin = &*params.id,
            %error,
            stays_off = recovery == Recovery::StayOff,
            "plugin stopped"
        );
        *lock(state) = match recovery {
            Recovery::RetryAt(retry_at) => RunState::BackingOff {
                error: error.clone(),
                retry_at,
            },
            Recovery::StayOff => RunState::AutoOff {
                error: error.clone(),
            },
        };
        let _ = params.outbox.send(HostEvent::Failed {
            id: params.id.to_string(),
            error: error.clone(),
            recovery,
        });
        if let Some(reply) = reply.take() {
            let _ = reply.send(Err(error));
        }
        let Recovery::RetryAt(retry_at) = recovery else {
            return Ok(());
        };
        if queue.wait_for_stop(retry_at) {
            return Ok(());
        }
        params.diag.restarted();
        queue.set_accepting(true);
    }
}

/// Delivers events until a stop (`None`) or a failed call.
fn serve(instance: &mut Instance, queue: &EventQueue) -> Option<CallError> {
    loop {
        match queue.pop() {
            Next::Stop => return None,
            Next::Event(event) => {
                if let Err(error) = instance.on_event(&event) {
                    return Some(error);
                }
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_loaded(id: &str) -> HostError {
    HostError::NotLoaded(id.to_owned())
}
