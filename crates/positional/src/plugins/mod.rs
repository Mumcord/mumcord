//! Host for the official Mumble game plugins (FR-1.42b, FR-1.43).
//!
//! Plugins are native libraries implementing Mumble's C plugin interface ([`abi`]). They are
//! found in Mumcord's plugin folder and in the folders of an existing Mumble installation
//! ([`plugin_dirs`]), loaded only once the plugin list is asked for or a plugin is enabled,
//! and initialised only while enabled. All plugin calls run on one dedicated thread
//! (`mumcord-plugins`), never on the audio or poll threads:
//! - while the engine asks a plugin's [`PositionSource`] for positions, the thread hands the
//!   running programs to the plugin every [`SEARCH_INTERVAL`] until it finds its game, then
//!   fetches positions every [`POLL`] and publishes them for the source to pick up;
//! - when the engine stops asking (positional audio off), the plugin stops reading the game
//!   within [`DEMAND_TIMEOUT`]; memory-reading plugins are what some anti-cheat systems flag.
//!
//! A plugin that fails (error codes, an incompatible API, a panic in the host code around it)
//! is marked and left alone; the others keep working. A plugin that crashes the process or
//! hangs cannot be contained by an in-process host; plugins are therefore off until the user
//! enables them one by one.

pub mod abi;
mod library;
mod processes;

use std::collections::HashMap;
use std::ffi::CString;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use library::{LoadError, PluginLibrary, RawPositions, Search};

use crate::{Fetch, PositionSource, Positional};

/// How often an active plugin is asked for positions.
pub const POLL: Duration = Duration::from_millis(20);
/// How often a plugin waiting for its game gets the list of running programs.
pub const SEARCH_INTERVAL: Duration = Duration::from_secs(2);
/// A plugin whose source the engine has not asked for this long stops reading the game.
pub const DEMAND_TIMEOUT: Duration = Duration::from_secs(1);
/// Published positions older than this are not handed out (the host thread is stuck).
const STALE: Duration = Duration::from_millis(500);
/// How long [`PluginHost::list`] waits for the host thread (loading many plugins takes a
/// moment).
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a plugin does not deliver positions although it is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginProblem {
    /// Built for a plugin interface Mumcord does not implement.
    Incompatible,
    /// `mumble_init` reported an error.
    InitFailed,
    /// The plugin said it can never deliver positions on this system.
    Unsupported,
    /// Calling the plugin failed unexpectedly; it is not called again until re-enabled.
    Failed,
}

/// A game plugin as shown in the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Stable identifier: the file name without `lib` prefix and extension, e.g. "gtav".
    pub id: String,
    /// Name the plugin reports (untrusted text); the identifier if it reports none.
    pub name: String,
    pub version: Option<String>,
    pub enabled: bool,
    /// The plugin found its game and delivers positions.
    pub active: bool,
    pub problem: Option<PluginProblem>,
}

/// The host thread stopped or did not answer in time.
#[derive(Debug, thiserror::Error)]
pub enum PluginHostError {
    #[error("the plugin host does not respond")]
    Unresponsive,
}

/// The folders searched for plugins, in order of precedence: Mumcord's own folder first, then
/// the user's and the system's Mumble plugin folders where they exist. A plugin identifier
/// found in several folders is taken from the first.
pub fn plugin_dirs(mumcord_dir: PathBuf) -> Vec<PathBuf> {
    let mut dirs = vec![mumcord_dir];
    dirs.extend(mumble_dirs());
    dirs
}

#[cfg(target_os = "linux")]
fn mumble_dirs() -> Vec<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let mut dirs: Vec<PathBuf> = data
        .map(|d| d.join("Mumble/Mumble/Plugins"))
        .into_iter()
        .collect();
    dirs.extend(
        [
            "/usr/lib/mumble/plugins",
            "/usr/lib64/mumble/plugins",
            "/usr/lib/x86_64-linux-gnu/mumble/plugins",
            "/usr/lib/aarch64-linux-gnu/mumble/plugins",
            "/usr/local/lib/mumble/plugins",
        ]
        .map(PathBuf::from),
    );
    dirs
}

#[cfg(target_os = "macos")]
fn mumble_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Application Support/Mumble/Mumble/Plugins"))
        .into_iter()
        .collect();
    dirs.push(PathBuf::from("/Applications/Mumble.app/Contents/Plugins"));
    dirs
}

#[cfg(windows)]
fn mumble_dirs() -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut dirs = Vec::new();
    for base in [env("APPDATA"), env("LOCALAPPDATA")].into_iter().flatten() {
        dirs.push(base.join(r"Mumble\Mumble\Plugins"));
    }
    if let Some(programs) = env("ProgramFiles") {
        dirs.push(programs.join(r"Mumble\client\plugins"));
        dirs.push(programs.join(r"Mumble\plugins"));
    }
    dirs
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn mumble_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// The plugin identifier of a library file, or `None` if it is not a plugin library here.
fn plugin_id(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    if !ext.eq_ignore_ascii_case(std::env::consts::DLL_EXTENSION) {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let id = stem
        .strip_prefix(std::env::consts::DLL_PREFIX)
        .filter(|s| !s.is_empty())
        .unwrap_or(stem);
    (!id.is_empty()).then(|| id.to_owned())
}

/// Latest positions of one plugin, shared between the host thread and its source.
#[derive(Default)]
struct Slot {
    /// When the engine last asked the source.
    demanded_at: Option<Instant>,
    latest: Option<(Instant, Fetch)>,
}

type SharedSlot = Arc<Mutex<Slot>>;
type Slots = Arc<Mutex<HashMap<String, SharedSlot>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned slot only means a panic elsewhere; its data stays usable.
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn slot(slots: &Slots, id: &str) -> SharedSlot {
    lock(slots).entry(id.to_owned()).or_default().clone()
}

enum Command {
    List(mpsc::Sender<Vec<PluginInfo>>),
    SetEnabled(Vec<String>),
}

/// Handle to the plugin host thread. Cheap to clone; the thread shuts every plugin down and
/// ends when the last handle is dropped.
#[derive(Clone)]
pub struct PluginHost {
    commands: mpsc::Sender<Command>,
    slots: Slots,
}

impl PluginHost {
    /// Starts the host thread for plugins in `dirs` (see [`plugin_dirs`]). Nothing is loaded
    /// yet.
    pub fn start(dirs: Vec<PathBuf>) -> std::io::Result<Self> {
        let (commands, rx) = mpsc::channel();
        let slots = Slots::default();
        let host = Host {
            dirs,
            plugins: Vec::new(),
            slots: slots.clone(),
        };
        std::thread::Builder::new()
            .name("mumcord-plugins".into())
            .spawn(move || host.run(&rx))?;
        Ok(Self { commands, slots })
    }

    /// The plugins found (looking for new files each time), loading them if needed.
    pub fn list(&self) -> Result<Vec<PluginInfo>, PluginHostError> {
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(Command::List(reply))
            .map_err(|_| PluginHostError::Unresponsive)?;
        answer
            .recv_timeout(REPLY_TIMEOUT)
            .map_err(|_| PluginHostError::Unresponsive)
    }

    /// Enables exactly the plugins in `ids` (unknown identifiers are ignored) and shuts the
    /// others down.
    pub fn set_enabled(&self, ids: &[String]) -> Result<(), PluginHostError> {
        self.commands
            .send(Command::SetEnabled(ids.to_vec()))
            .map_err(|_| PluginHostError::Unresponsive)
    }

    /// One position source per plugin, in the given order, for `Engine::set_position_sources`.
    pub fn sources(&self, ids: &[String]) -> Vec<Box<dyn PositionSource>> {
        ids.iter()
            .map(|id| Box::new(PluginSource(slot(&self.slots, id))) as Box<dyn PositionSource>)
            .collect()
    }
}

/// Positions of one game plugin as a [`PositionSource`]. Never calls the plugin itself: it
/// takes what the host thread published.
pub struct PluginSource(SharedSlot);

impl PositionSource for PluginSource {
    fn fetch(&mut self, now: Instant) -> Option<Fetch> {
        let mut slot = lock(&self.0);
        slot.demanded_at = Some(now);
        slot.latest
            .as_ref()
            .filter(|(at, _)| now.saturating_duration_since(*at) < STALE)
            .map(|(_, fetch)| fetch.clone())
    }
}

/// Where an enabled plugin stands.
enum Phase {
    Disabled,
    /// Initialised; waiting for its game.
    Searching {
        next: Instant,
    },
    /// Found its game; positions are fetched.
    Active,
    Failed(PluginProblem),
}

struct Loaded {
    library: PluginLibrary,
    initialised: bool,
    phase: Phase,
}

enum Entry {
    Loaded(Box<Loaded>),
    Incompatible,
}

struct Plugin {
    id: String,
    path: PathBuf,
    entry: Entry,
}

/// State of the host thread.
struct Host {
    dirs: Vec<PathBuf>,
    plugins: Vec<Plugin>,
    slots: Slots,
}

impl Host {
    fn run(mut self, commands: &mpsc::Receiver<Command>) {
        loop {
            match commands.recv_timeout(POLL) {
                Ok(Command::List(reply)) => {
                    self.scan();
                    // The asker may have given up waiting.
                    let _ = reply.send(self.infos());
                }
                Ok(Command::SetEnabled(ids)) => self.set_enabled(&ids),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.tick(Instant::now());
        }
        for plugin in &mut self.plugins {
            if let Entry::Loaded(loaded) = &mut plugin.entry {
                disable(&plugin.id, loaded);
            }
        }
    }

    /// Loads plugin files not seen yet.
    fn scan(&mut self) {
        for dir in self.dirs.clone() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
            paths.sort();
            for path in paths {
                let Some(id) = plugin_id(&path) else {
                    continue;
                };
                if self.plugins.iter().any(|p| p.id == id || p.path == path) {
                    continue;
                }
                if let Some(entry) = load(&id, &path) {
                    self.plugins.push(Plugin { id, path, entry });
                }
            }
        }
    }

    fn infos(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|p| match &p.entry {
                Entry::Incompatible => PluginInfo {
                    id: p.id.clone(),
                    name: p.id.clone(),
                    version: None,
                    enabled: false,
                    active: false,
                    problem: Some(PluginProblem::Incompatible),
                },
                Entry::Loaded(l) => PluginInfo {
                    id: p.id.clone(),
                    name: if l.library.name.is_empty() {
                        p.id.clone()
                    } else {
                        l.library.name.clone()
                    },
                    version: l.library.version.clone(),
                    enabled: !matches!(l.phase, Phase::Disabled),
                    active: matches!(l.phase, Phase::Active),
                    problem: match l.phase {
                        Phase::Failed(problem) => Some(problem),
                        _ => None,
                    },
                },
            })
            .collect()
    }

    fn set_enabled(&mut self, ids: &[String]) {
        if !ids.is_empty() {
            self.scan();
        }
        for plugin in &mut self.plugins {
            let Entry::Loaded(loaded) = &mut plugin.entry else {
                continue;
            };
            let wanted = ids.contains(&plugin.id);
            let enabled = !matches!(loaded.phase, Phase::Disabled);
            if wanted && !enabled {
                guarded(&plugin.id, loaded, |l| enable(&plugin.id, l));
            } else if !wanted && enabled {
                guarded(&plugin.id, loaded, |l| disable(&plugin.id, l));
                clear(&self.slots, &plugin.id);
            }
        }
    }

    /// Searches or fetches for every enabled plugin the engine currently asks for.
    fn tick(&mut self, now: Instant) {
        let mut programs: Option<Vec<(CString, u64)>> = None;
        for plugin in &mut self.plugins {
            let Entry::Loaded(loaded) = &mut plugin.entry else {
                continue;
            };
            if !matches!(loaded.phase, Phase::Searching { .. } | Phase::Active) {
                continue;
            }
            let slot = slot(&self.slots, &plugin.id);
            let demanded = lock(&slot)
                .demanded_at
                .is_some_and(|at| now.saturating_duration_since(at) < DEMAND_TIMEOUT);
            let id = plugin.id.as_str();
            guarded(id, loaded, |l| match l.phase {
                Phase::Active if !demanded => {
                    l.library.shutdown_positional();
                    l.phase = Phase::Searching { next: now };
                }
                Phase::Active => match l.library.fetch_positional() {
                    Some(raw) => {
                        let fetch = to_fetch(&l.library.name, id, raw);
                        lock(&slot).latest = Some((now, fetch));
                    }
                    None => {
                        tracing::info!(plugin = id, "game plugin: the game is gone");
                        l.library.shutdown_positional();
                        l.phase = Phase::Searching {
                            next: now + SEARCH_INTERVAL,
                        };
                    }
                },
                Phase::Searching { next } if demanded && now >= next => {
                    let programs = programs.get_or_insert_with(processes::running);
                    match l.library.init_positional(programs) {
                        Search::Found => {
                            tracing::info!(plugin = id, "game plugin: found its game");
                            l.phase = Phase::Active;
                        }
                        Search::NotNow => {
                            l.phase = Phase::Searching {
                                next: now + SEARCH_INTERVAL,
                            };
                        }
                        Search::Never => {
                            tracing::info!(plugin = id, "game plugin: unsupported on this system");
                            l.phase = Phase::Failed(PluginProblem::Unsupported);
                        }
                    }
                }
                _ => {}
            });
            if !matches!(loaded.phase, Phase::Active) {
                lock(&slot).latest = None;
            }
        }
    }
}

/// Loads one library; `None` if it is no positional game plugin.
fn load(id: &str, path: &Path) -> Option<Entry> {
    let result = catch_unwind(|| PluginLibrary::load(path));
    match result {
        Ok(Ok(library)) => {
            tracing::info!(plugin = id, name = %library.name, "game plugin loaded");
            Some(Entry::Loaded(Box::new(Loaded {
                library,
                initialised: false,
                phase: Phase::Disabled,
            })))
        }
        Ok(Err(LoadError::Incompatible(version))) => {
            tracing::warn!(plugin = id, %version, "game plugin skipped: incompatible plugin API");
            Some(Entry::Incompatible)
        }
        Ok(Err(e @ (LoadError::NotAPlugin(_) | LoadError::NotPositional))) => {
            tracing::debug!(plugin = id, reason = %e, "library skipped");
            None
        }
        Ok(Err(e @ LoadError::Open(_))) => {
            tracing::warn!(plugin = id, reason = %e, "game plugin skipped");
            None
        }
        Err(_) => {
            tracing::warn!(plugin = id, "game plugin skipped: loading it panicked");
            None
        }
    }
}

/// Runs `f` on a plugin; a panic marks the plugin failed instead of ending the host thread.
fn guarded(id: &str, loaded: &mut Loaded, f: impl FnOnce(&mut Loaded)) {
    if catch_unwind(AssertUnwindSafe(|| f(loaded))).is_err() {
        tracing::error!(plugin = id, "game plugin failed; it is not called again");
        loaded.phase = Phase::Failed(PluginProblem::Failed);
    }
}

fn enable(id: &str, loaded: &mut Loaded) {
    if !loaded.initialised {
        // Mumble gives every plugin its own number; any unique value works.
        if !loaded.library.init(next_plugin_id()) {
            tracing::warn!(plugin = id, "game plugin failed to initialise");
            loaded.phase = Phase::Failed(PluginProblem::InitFailed);
            return;
        }
        loaded.initialised = true;
    }
    tracing::info!(plugin = id, "game plugin enabled");
    loaded.phase = Phase::Searching {
        next: Instant::now(),
    };
}

fn disable(id: &str, loaded: &mut Loaded) {
    if matches!(loaded.phase, Phase::Active) {
        loaded.library.shutdown_positional();
    }
    if loaded.initialised {
        loaded.library.shutdown();
        loaded.initialised = false;
    }
    if !matches!(loaded.phase, Phase::Disabled) {
        tracing::info!(plugin = id, "game plugin disabled");
    }
    loaded.phase = Phase::Disabled;
}

fn next_plugin_id() -> abi::PluginId {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn clear(slots: &Slots, id: &str) {
    lock(&slot(slots, id)).latest = None;
}

/// What the engine gets from a plugin: the context is prefixed with the plugin name and a NUL
/// byte like the official client does, so only players using the same plugin and game
/// context hear each other in 3D.
fn to_fetch(name: &str, id: &str, raw: RawPositions) -> Fetch {
    let name = if name.is_empty() { id } else { name };
    let mut context = Vec::with_capacity(name.len() + 1 + raw.context.len());
    context.extend_from_slice(name.as_bytes());
    context.push(0);
    context.extend_from_slice(raw.context.as_bytes());
    Fetch {
        source: name.to_owned(),
        game: None,
        data: Positional {
            avatar: raw.avatar,
            camera: raw.camera,
            context,
            identity: raw.identity,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Pose;

    #[test]
    fn plugin_ids_follow_file_names() {
        let file = |stem: &str, ext: &str| PathBuf::from(format!("/p/{stem}.{ext}"));
        let ext = std::env::consts::DLL_EXTENSION;
        let prefix = std::env::consts::DLL_PREFIX;
        assert_eq!(
            plugin_id(&file(&format!("{prefix}gtav"), ext)).as_deref(),
            Some("gtav")
        );
        assert_eq!(plugin_id(&file("readme", "txt")), None);
        assert_eq!(plugin_id(&PathBuf::from("/p/noext")), None);
    }

    #[test]
    fn context_carries_the_plugin_name() {
        let raw = RawPositions {
            avatar: Pose {
                position: [1.0, 2.0, 3.0],
                ..Pose::default()
            },
            context: "server:1".into(),
            identity: "me".into(),
            ..RawPositions::default()
        };
        let fetch = to_fetch("Grand Theft Auto V", "gtav", raw);
        assert_eq!(fetch.source, "Grand Theft Auto V");
        assert_eq!(fetch.data.context, b"Grand Theft Auto V\0server:1");
        assert_eq!(fetch.data.identity, "me");
        assert_eq!(fetch.data.avatar.position, [1.0, 2.0, 3.0]);
        assert_eq!(to_fetch("", "gtav", RawPositions::default()).source, "gtav");
    }

    #[test]
    fn sources_hand_out_fresh_positions_only() {
        let slots = Slots::default();
        let mut source = PluginSource(slot(&slots, "x"));
        let now = Instant::now();
        assert_eq!(source.fetch(now), None);
        assert!(lock(&slot(&slots, "x")).demanded_at.is_some());
        let fetch = to_fetch("X", "x", RawPositions::default());
        lock(&slot(&slots, "x")).latest = Some((now, fetch.clone()));
        assert_eq!(source.fetch(now), Some(fetch));
        assert_eq!(source.fetch(now + STALE), None);
    }

    #[test]
    fn mumble_dirs_come_after_mumcords_own() {
        let dirs = plugin_dirs(PathBuf::from("/data/plugins"));
        assert_eq!(dirs[0], PathBuf::from("/data/plugins"));
    }
}
