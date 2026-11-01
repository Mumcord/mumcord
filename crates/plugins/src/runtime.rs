//! The wasmtime sandbox (#268, FR-6.1–6.3). A plugin is a WebAssembly component that can only
//! call the imports in `wit/`: there is no WASI, so no files, sockets, clocks or randomness. Each
//! running plugin has its own `Store` with a memory limit, and every call into it has a CPU
//! deadline enforced by epoch interruption. Events reach a plugin through its [`EventQueue`].

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use semver::Version;
use tokio::sync::mpsc::UnboundedSender;
use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Component, InstancePre, Linker};
use wasmtime::{
    Config, Engine, ResourceLimiter, Store, StoreLimits, StoreLimitsBuilder, Trap, UpdateDeadline,
};

use crate::api;
use crate::api::files::Files;
use crate::api::http::{Http, HttpSet};
use crate::api::io::PluginIo;
use crate::api::notify::Notify;
use crate::api::secrets::{PluginSecrets, SecretsApi};
use crate::api::storage::{Storage, StorageApi};
use crate::api::theme::Theme;
use crate::api::timers::{TimerSet, Timers};
use crate::cache;
use crate::chat::ChatRef;
use crate::diag::Diagnostics;
use crate::grants::Grants;
use crate::host::HostEvent;
use crate::peer_key::PeerKeys;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({ path: "wit", world: "plugin" });
}

/// The WIT types (events, peers, UI tree …) as Rust types, for the service that feeds plugins.
pub use bindings::mumcord::plugin as wit;
use bindings::mumcord::plugin::events::Event;
pub use bindings::mumcord::plugin::log::Level;
use bindings::mumcord::plugin::types::{Peer, StartInfo};
use bindings::{Plugin, PluginPre};

/// How often the CPU clock ticks.
pub const EPOCH_TICK: Duration = Duration::from_millis(10);
/// Ticks a normal call (event, timer, stop) may use: 50 ms.
pub const CALL_TICKS: u64 = 5;
/// Ticks `start` may use: 500 ms.
pub const START_TICKS: u64 = 50;
/// Wall-clock limit of any call, including the time spent waiting in host storage, which
/// does not count against the plugin's own budget (FR-6.3 addendum).
pub const CALL_WALL_CAP: Duration = Duration::from_millis(500);
pub const MAX_TABLE_ELEMENTS: usize = 10_000;
/// Core instances per plugin. A component from wit-bindgen is a few core modules (the plugin
/// plus small adapter shims), so this is more than one but still tight.
const MAX_INSTANCES: usize = 16;
const MAX_TABLES: usize = 8;
const MAX_MEMORIES: usize = 1;

/// Plugin log buffer size: lines, and characters per line.
pub const LOG_LINES: usize = 200;
pub const LOG_LINE_CHARS: usize = 200;

/// Events waiting per plugin (pending talking states included).
pub const QUEUE_CAPACITY: usize = 256;
/// Talking events delivered per plugin and second; more are coalesced per user.
pub const TALKING_PER_SECOND: usize = 10;

/// The only major API version this Mumcord serves.
const API_MAJOR: u64 = 1;

/// Interfaces a plugin may import, without the `@version` suffix. The version must be `1.x`;
/// the linker then also refuses a minor version newer than the host. A component imports every
/// interface whose types it uses (e.g. `events` for `on-event`), so all of them are listed; an
/// import of a function the host does not provide yet fails in the linker.
const ALLOWED_IMPORTS: &[&str] = &[
    "mumcord:plugin/types",
    "mumcord:plugin/log",
    "mumcord:plugin/storage",
    "mumcord:plugin/timers",
    "mumcord:plugin/clock",
    "mumcord:plugin/notify",
    "mumcord:plugin/theme",
    "mumcord:plugin/random",
    "mumcord:plugin/peers",
    "mumcord:plugin/ui",
    "mumcord:plugin/chat",
    "mumcord:plugin/secrets",
    "mumcord:plugin/http",
    "mumcord:plugin/webview",
    "mumcord:plugin/events",
];

/// One WIT interface the host serves. Each `api/*` module implements the generated `Host` trait
/// of its interface for [`PluginState`] and this trait to put it into the linker; [`HOST_APIS`]
/// lists them all.
pub(crate) trait HostApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()>;
}

type AddToLinker = fn(&mut Linker<PluginState>) -> wasmtime::Result<()>;

/// Every interface the host serves. One line per `api/*` module.
const HOST_APIS: &[AddToLinker] = &[
    <api::log::LogApi as HostApi>::add_to_linker,
    <api::timers::TimersApi as HostApi>::add_to_linker,
    <api::clock::ClockApi as HostApi>::add_to_linker,
    <api::notify::NotifyApi as HostApi>::add_to_linker,
    <api::theme::ThemeApi as HostApi>::add_to_linker,
    <api::random::RandomApi as HostApi>::add_to_linker,
    <StorageApi as HostApi>::add_to_linker,
    <SecretsApi as HostApi>::add_to_linker,
    <api::http::HttpApi as HostApi>::add_to_linker,
    <api::ui::UiApi as HostApi>::add_to_linker,
    <api::peers::PeersApi as HostApi>::add_to_linker,
    <api::chat::ChatApi as HostApi>::add_to_linker,
    <api::webview::WebviewApi as HostApi>::add_to_linker,
];

/// Services shared by all plugins that host calls need (store, grants, settings …). Later host
/// API tasks add their fields here; the service builds it once.
#[derive(Debug, Default)]
pub struct HostContext {
    /// `storage` (FR-6.20) and the data helpers for export, reset and remove.
    pub storage: Storage,
    /// `secrets` (FR-6.25); `None` turns the interface off (tests, no keychain wanted).
    pub secrets: Option<Arc<PluginSecrets>>,
    /// The permission gate every host call and event goes through.
    pub grants: Grants,
    pub timers: Timers,
    pub notify: Notify,
    pub theme: Theme,
    /// `None` until the service has loaded the per-install secret; peers then get no key.
    pub peer_keys: Option<PeerKeys>,
    /// `net.http` (FR-6.37): allowed addresses and the request worker.
    pub http: Http,
    /// Files the user picked in plugin file pickers, for `net.http` uploads.
    pub files: Files,
    /// Carries out `ui`, `peers` and `chat` calls; `None` refuses them with `not-permitted`.
    pub io: Option<Arc<dyn PluginIo>>,
}

/// One wasmtime engine for all plugins plus the thread that ticks the CPU clock.
pub struct Runtime {
    engine: Engine,
    linker: Linker<PluginState>,
    app_version: Version,
    cache_dir: Option<PathBuf>,
    ticker: Option<JoinHandle<()>>,
    stop_ticker: Arc<AtomicBool>,
}

/// Why a component cannot be loaded.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("the plugin is not a valid WebAssembly component: {0}")]
    Invalid(String),
    /// The import that was refused is kept for the log; users see the FR-6.1 text.
    #[error("This plugin uses features Mumcord does not provide")]
    UnknownImport(String),
    #[error("the plugin does not match the Mumcord plugin API: {0}")]
    Mismatch(String),
}

/// Why a call into a plugin failed. Any failure stops the plugin: a component instance cannot
/// be entered again after a trap.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    #[error("took too long and was stopped")]
    Timeout,
    #[error("used more memory than allowed")]
    OutOfMemory,
    #[error("crashed: {0}")]
    Trap(String),
    /// `start` returned an error of its own.
    #[error("could not start: {0}")]
    Refused(String),
}

/// A compiled plugin, checked against the API and ready to start on any thread.
#[derive(Clone)]
pub struct Compiled {
    engine: Engine,
    pre: PluginPre<PluginState>,
    /// The plugin imports `chat.is-replay`, so it can tell replayed chat events from live ones.
    knows_replay: bool,
}

/// Everything a plugin's store is built from.
pub(crate) struct StartParams {
    pub id: Arc<str>,
    pub memory_bytes: usize,
    pub storage_quota: u64,
    pub previous_version: Option<String>,
    pub ctx: Arc<HostContext>,
    pub outbox: UnboundedSender<HostEvent>,
    pub log: Arc<Mutex<LogBuffer>>,
    pub diag: Arc<Diagnostics>,
    /// The plugin's event queue, for host calls that answer later (timers).
    pub queue: Weak<EventQueue>,
}

/// One running plugin: its store and instance. Lives on the plugin's own thread.
pub struct Instance {
    store: Store<PluginState>,
    plugin: Plugin,
}

/// What the host keeps per running plugin, reachable from host functions.
pub struct PluginState {
    pub(crate) id: Arc<str>,
    pub(crate) ctx: Arc<HostContext>,
    /// Where host calls report what the plugin wants (UI trees, notifications, messages …).
    pub(crate) outbox: UnboundedSender<HostEvent>,
    pub(crate) log: Arc<Mutex<LogBuffer>>,
    pub(crate) diag: Arc<Diagnostics>,
    /// The manifest's storage quota in bytes (FR-6.20).
    pub(crate) storage_quota: u64,
    pub(crate) timers: TimerSet,
    pub(crate) http: HttpSet,
    /// The FR-6.42 update rate per UI slot.
    pub(crate) ui: api::ui::UiLimiter,
    pub(crate) clock: CallClock,
    /// The chat message behind the event being handled (`chat.current-message`).
    pub(crate) chat: ChatRef,
    limiter: Limiter,
}

/// The current call's time. The epoch clock counts wall time, so time spent waiting for the
/// database (lock, disk) is given back to the plugin at its deadline, up to [`CALL_WALL_CAP`].
#[derive(Debug)]
pub(crate) struct CallClock {
    started: Instant,
    waited: Duration,
    refunded: Duration,
}

impl Default for CallClock {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            waited: Duration::ZERO,
            refunded: Duration::ZERO,
        }
    }
}

impl CallClock {
    /// Runs a host call whose time is waiting, not the plugin's own work.
    pub(crate) fn waiting<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let result = f();
        self.waited += started.elapsed();
        result
    }

    /// At the deadline: stop the plugin unless waiting time is still to be given back.
    fn deadline(&mut self) -> UpdateDeadline {
        if self.started.elapsed() >= CALL_WALL_CAP {
            return UpdateDeadline::Interrupt;
        }
        let owed = self.waited.saturating_sub(self.refunded);
        let ticks = u64::try_from(owed.as_nanos() / EPOCH_TICK.as_nanos()).unwrap_or(u64::MAX);
        if ticks == 0 {
            return UpdateDeadline::Interrupt;
        }
        self.refunded += EPOCH_TICK * u32::try_from(ticks).unwrap_or(u32::MAX);
        UpdateDeadline::Continue(ticks)
    }
}

/// The FR-6.3 store limits, plus the current memory size for Diagnostics.
struct Limiter {
    limits: StoreLimits,
    diag: Arc<Diagnostics>,
}

impl Runtime {
    /// `cache_dir` is the app's cache folder; compiled plugins go to `<cache_dir>/plugins/`.
    /// `None` compiles every time.
    pub fn new(app_version: Version, cache_dir: Option<PathBuf>) -> Result<Self, CompileError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|e| CompileError::Invalid(e.to_string()))?;

        let mut linker = Linker::new(&engine);
        for add in HOST_APIS {
            add(&mut linker).map_err(|e| CompileError::Invalid(e.to_string()))?;
        }

        let stop_ticker = Arc::new(AtomicBool::new(false));
        let ticker = {
            let engine = engine.clone();
            let stop = Arc::clone(&stop_ticker);
            std::thread::Builder::new()
                .name("mumcord-plugin-clock".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(EPOCH_TICK);
                        engine.increment_epoch();
                    }
                })
                .map_err(|e| CompileError::Invalid(e.to_string()))?
        };
        Ok(Self {
            engine,
            linker,
            app_version,
            cache_dir,
            ticker: Some(ticker),
            stop_ticker,
        })
    }

    /// Compiles a component (or takes it from the cache) and checks its imports before any of
    /// its code runs.
    pub fn compile(&self, wasm: &[u8]) -> Result<Compiled, CompileError> {
        let cache_path = self
            .cache_dir
            .as_deref()
            .map(|dir| cache::path(dir, &cache::key(wasm, &self.app_version)));
        let cached = cache_path
            .as_deref()
            .and_then(|path| cache::load(&self.engine, path));
        let fresh = cached.is_none();
        let component = match cached {
            Some(component) => component,
            None => Component::new(&self.engine, wasm)
                .map_err(|e| CompileError::Invalid(format!("{e:#}")))?,
        };
        let mut knows_replay = false;
        for (name, item) in component.component_type().imports(&self.engine) {
            if !import_allowed(name) {
                return Err(CompileError::UnknownImport(name.to_owned()));
            }
            if name.starts_with("mumcord:plugin/chat@")
                && let ComponentItem::ComponentInstance(instance) = item.ty
            {
                knows_replay = instance
                    .exports(&self.engine)
                    .any(|(export, _)| export == "is-replay");
            }
        }
        let pre: InstancePre<PluginState> = self
            .linker
            .instantiate_pre(&component)
            .map_err(|e| CompileError::Mismatch(format!("{e:#}")))?;
        let pre = PluginPre::new(pre).map_err(|e| CompileError::Mismatch(format!("{e:#}")))?;
        // Only components that passed the checks are cached.
        if let (true, Some(path)) = (fresh, &cache_path)
            && let Err(error) = cache::store(path, &component)
        {
            tracing::warn!(%error, "could not cache a compiled plugin");
        }
        Ok(Compiled {
            engine: self.engine.clone(),
            pre,
            knows_replay,
        })
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop_ticker.store(true, Ordering::Relaxed);
        if let Some(ticker) = self.ticker.take() {
            let _ = ticker.join();
        }
    }
}

/// `mumcord:plugin/<interface>@1.x` only: no WASI, no unversioned or other-major imports.
fn import_allowed(name: &str) -> bool {
    let Some((interface, version)) = name.split_once('@') else {
        return false;
    };
    ALLOWED_IMPORTS.contains(&interface)
        && Version::parse(version).is_ok_and(|v| v.major == API_MAJOR)
}

impl Compiled {
    /// Whether replayed chat events may go to this plugin (see `chat.is-replay`): a plugin built
    /// before replays existed could not tell them from live ones and would act on them twice.
    pub(crate) fn knows_replay(&self) -> bool {
        self.knows_replay
    }

    /// Instantiates the plugin in a fresh store and calls its `start`.
    pub(crate) fn start(&self, params: &StartParams) -> Result<Instance, CallError> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(params.memory_bytes)
            .table_elements(MAX_TABLE_ELEMENTS)
            .instances(MAX_INSTANCES)
            .tables(MAX_TABLES)
            .memories(MAX_MEMORIES)
            // A failed memory.grow would otherwise just return -1 and let the plugin carry on.
            .trap_on_grow_failure(true)
            .build();
        let state = PluginState {
            id: Arc::clone(&params.id),
            ctx: Arc::clone(&params.ctx),
            outbox: params.outbox.clone(),
            log: Arc::clone(&params.log),
            diag: Arc::clone(&params.diag),
            storage_quota: params.storage_quota,
            timers: params.ctx.timers.set(Weak::clone(&params.queue)),
            http: params
                .ctx
                .http
                .set(Arc::clone(&params.id), Weak::clone(&params.queue)),
            ui: api::ui::UiLimiter::default(),
            clock: CallClock::default(),
            chat: ChatRef::default(),
            limiter: Limiter {
                limits,
                diag: Arc::clone(&params.diag),
            },
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limiter);
        store.epoch_deadline_callback(|mut state| Ok(state.data_mut().clock.deadline()));

        store.set_epoch_deadline(START_TICKS);
        store.data_mut().clock = CallClock::default();
        let started = Instant::now();
        let result = self.pre.instantiate(&mut store).and_then(|plugin| {
            let info = StartInfo {
                api_version: crate::API_VERSION.to_string(),
                previous_version: params.previous_version.clone(),
            };
            plugin
                .call_start(&mut store, &info)
                .map(|outcome| (plugin, outcome))
        });
        params.diag.add_cpu(started.elapsed());
        let (plugin, outcome) = result.map_err(classify)?;
        outcome
            .map_err(|reason| CallError::Refused(reason.chars().take(LOG_LINE_CHARS).collect()))?;
        Ok(Instance { store, plugin })
    }
}

impl Instance {
    pub(crate) fn on_event(&mut self, event: &Event, chat: ChatRef) -> Result<(), CallError> {
        self.store.set_epoch_deadline(CALL_TICKS);
        self.store.data_mut().clock = CallClock::default();
        self.store.data_mut().chat = chat;
        let started = Instant::now();
        let result = self.plugin.call_on_event(&mut self.store, event);
        self.store.data_mut().chat = ChatRef::default();
        self.store.data().diag.add_cpu(started.elapsed());
        result.map_err(classify)
    }

    /// Calls `stop`. The instance is dropped afterwards either way.
    pub(crate) fn stop(mut self) -> Result<(), CallError> {
        self.store.set_epoch_deadline(CALL_TICKS);
        self.store.data_mut().clock = CallClock::default();
        let started = Instant::now();
        let result = self.plugin.call_stop(&mut self.store);
        self.store.data().diag.add_cpu(started.elapsed());
        result.map_err(classify)
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.store.data().diag.set_memory(0);
    }
}

fn classify(error: wasmtime::Error) -> CallError {
    match error.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => CallError::Timeout,
        // `trap_on_grow_failure` reports a refused grow as a plain error, not a `Trap`.
        _ if format!("{error:#}").contains("forcing trap when growing memory") => {
            CallError::OutOfMemory
        }
        _ => CallError::Trap(format!("{error:#}").chars().take(LOG_LINE_CHARS).collect()),
    }
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = self.limits.memory_growing(current, desired, maximum)?;
        if allowed {
            // A plugin has exactly one memory, so its size is the plugin's memory use.
            self.diag.set_memory(desired);
        }
        Ok(allowed)
    }

    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.limits.memory_grow_failed(error)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        self.limits.table_growing(current, desired, maximum)
    }

    fn table_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.limits.table_grow_failed(error)
    }

    fn instances(&self) -> usize {
        self.limits.instances()
    }

    fn tables(&self) -> usize {
        self.limits.tables()
    }

    fn memories(&self) -> usize {
        self.limits.memories()
    }
}

impl bindings::mumcord::plugin::types::Host for PluginState {}

impl PluginState {
    /// The [`PluginIo`] for `ui`, `peers` and `chat`; without one these calls are not permitted.
    pub(crate) fn io(&self) -> Result<&dyn PluginIo, wit::types::Error> {
        self.ctx
            .io
            .as_deref()
            .ok_or(wit::types::Error::NotPermitted)
    }
}

/// The last [`LOG_LINES`] lines a plugin logged, each cut to [`LOG_LINE_CHARS`] characters.
/// Kept apart from the app log so plugins cannot write into it.
#[derive(Debug, Default)]
pub struct LogBuffer {
    lines: VecDeque<(Level, String)>,
}

impl LogBuffer {
    pub(crate) fn push(&mut self, level: Level, message: &str) {
        if self.lines.len() == LOG_LINES {
            self.lines.pop_front();
        }
        let line = message
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(LOG_LINE_CHARS)
            .collect();
        self.lines.push_back((level, line));
    }

    pub fn lines(&self) -> impl Iterator<Item = (Level, &str)> {
        self.lines
            .iter()
            .map(|(level, line)| (*level, line.as_str()))
    }
}

/// What a plugin thread does next.
pub(crate) enum Next {
    Event(Box<Event>, ChatRef),
    Stop,
}

/// The events waiting for one plugin (FR-6.2): at most [`QUEUE_CAPACITY`]; when full, the
/// oldest non-UI event is dropped (UI events only when nothing else is left) and counted.
/// Talking events wait as one state per user (the newest wins) and are delivered at most
/// [`TALKING_PER_SECOND`] times per second.
pub(crate) struct EventQueue {
    state: Mutex<QueueState>,
    changed: Condvar,
    diag: Arc<Diagnostics>,
}

struct QueueState {
    events: VecDeque<(u64, Event, ChatRef)>,
    /// Pending talking state per user, in order of the first change since the last delivery.
    talking: VecDeque<(u64, Peer, bool)>,
    /// When the talking events of the last second were delivered.
    talking_sent: VecDeque<Instant>,
    next_seq: u64,
    accepting: bool,
    stop: bool,
}

impl EventQueue {
    pub(crate) fn new(diag: Arc<Diagnostics>) -> Self {
        Self {
            state: Mutex::new(QueueState {
                events: VecDeque::new(),
                talking: VecDeque::new(),
                talking_sent: VecDeque::new(),
                next_seq: 0,
                accepting: true,
                stop: false,
            }),
            changed: Condvar::new(),
            diag,
        }
    }

    fn lock(&self) -> MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues an event. Returns false when the plugin is not taking events (backing off); the
    /// event is then dropped and counted.
    pub(crate) fn push(&self, event: Event) -> bool {
        self.push_chat(event, ChatRef::default())
    }

    /// Queues a chat event together with the message it is about (`chat.current-message`).
    pub(crate) fn push_chat(&self, event: Event, chat: ChatRef) -> bool {
        let mut state = self.lock();
        if !state.accepting {
            self.diag.events_dropped(1);
            return false;
        }
        let dropped = state.push(event, chat);
        drop(state);
        self.diag.events_dropped(dropped);
        self.changed.notify_one();
        true
    }

    /// Blocks until there is an event to deliver or the plugin has to stop.
    pub(crate) fn pop(&self) -> Next {
        let mut state = self.lock();
        loop {
            if state.stop {
                return Next::Stop;
            }
            let now = Instant::now();
            if let Some((event, chat)) = state.pop(now) {
                return Next::Event(Box::new(event), chat);
            }
            state = match state.talking_wake_at() {
                Some(at) => {
                    self.changed
                        .wait_timeout(state, at.saturating_duration_since(now))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    /// Waits until `until`; true when the plugin has to stop instead.
    pub(crate) fn wait_for_stop(&self, until: Instant) -> bool {
        let mut state = self.lock();
        loop {
            if state.stop {
                return true;
            }
            let now = Instant::now();
            if now >= until {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, until - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    pub(crate) fn request_stop(&self) {
        self.lock().stop = true;
        self.changed.notify_all();
    }

    /// While a plugin backs off, events are not kept: drops what waits and refuses new ones.
    pub(crate) fn set_accepting(&self, accepting: bool) {
        let mut state = self.lock();
        state.accepting = accepting;
        if !accepting {
            let waiting = state.events.len() + state.talking.len();
            state.events.clear();
            state.talking.clear();
            drop(state);
            self.diag.events_dropped(waiting);
        }
    }
}

impl QueueState {
    /// Adds an event; returns how many had to be dropped for it.
    fn push(&mut self, event: Event, chat: ChatRef) -> usize {
        let seq = self.next_seq;
        self.next_seq += 1;
        if let Event::Talking((peer, talking)) = &event
            && let Some(pending) = self
                .talking
                .iter_mut()
                .find(|(_, p, _)| p.session == peer.session)
        {
            pending.1 = peer.clone();
            pending.2 = *talking;
            return 0;
        }
        let mut dropped = 0;
        while self.events.len() + self.talking.len() >= QUEUE_CAPACITY {
            self.drop_one();
            dropped += 1;
        }
        match event {
            Event::Talking((peer, talking)) => self.talking.push_back((seq, peer, talking)),
            event => self.events.push_back((seq, event, chat)),
        }
        dropped
    }

    /// Drops the oldest non-UI entry (talking states count as non-UI), else the oldest one.
    fn drop_one(&mut self) {
        let non_ui = self.events.iter().position(|(_, e, _)| !is_ui(e));
        let event_seq = non_ui.map(|i| self.events[i].0);
        let talking_seq = self.talking.front().map(|(seq, _, _)| *seq);
        match (non_ui, event_seq, talking_seq) {
            (Some(i), Some(e), Some(t)) if e < t => {
                self.events.remove(i);
            }
            (_, _, Some(_)) => {
                self.talking.pop_front();
            }
            (Some(i), _, None) => {
                self.events.remove(i);
            }
            (None, _, None) => {
                self.events.pop_front();
            }
        }
    }

    /// The next event in arrival order, unless it is a talking event over the rate limit; then
    /// other events go first.
    fn pop(&mut self, now: Instant) -> Option<(Event, ChatRef)> {
        while self
            .talking_sent
            .front()
            .is_some_and(|sent| now.duration_since(*sent) >= Duration::from_secs(1))
        {
            self.talking_sent.pop_front();
        }
        let talking_allowed = self.talking_sent.len() < TALKING_PER_SECOND;
        let talking_seq = self
            .talking
            .front()
            .filter(|_| talking_allowed)
            .map(|(seq, _, _)| *seq);
        let event_seq = self.events.front().map(|(seq, _, _)| *seq);
        match (event_seq, talking_seq) {
            (Some(e), Some(t)) if t < e => self.pop_talking(now),
            (Some(_), _) => self
                .events
                .pop_front()
                .map(|(_, event, chat)| (event, chat)),
            (None, Some(_)) => self.pop_talking(now),
            (None, None) => None,
        }
    }

    fn pop_talking(&mut self, now: Instant) -> Option<(Event, ChatRef)> {
        let (_, peer, talking) = self.talking.pop_front()?;
        self.talking_sent.push_back(now);
        Some((Event::Talking((peer, talking)), ChatRef::default()))
    }

    /// When a held-back talking event may go out.
    fn talking_wake_at(&self) -> Option<Instant> {
        if self.talking.is_empty() {
            return None;
        }
        self.talking_sent
            .front()
            .map(|sent| *sent + Duration::from_secs(1))
    }
}

/// Events from the user's own interaction with the plugin's UI; they are dropped last.
fn is_ui(event: &Event) -> bool {
    matches!(
        event,
        Event::UiAction(_)
            | Event::ProfileCardOpened(_)
            | Event::PanelVisibilityChanged(_)
            | Event::UserMenuRequested(_)
            | Event::DialogClosed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_buffer_caps_lines_and_length() {
        let mut log = LogBuffer::default();
        for i in 0..LOG_LINES + 5 {
            log.push(Level::Info, &format!("{i}\n{}", "x".repeat(500)));
        }
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), LOG_LINES);
        assert!(lines[0].1.starts_with("5 x"));
        assert_eq!(lines[0].1.chars().count(), LOG_LINE_CHARS);
    }

    #[test]
    fn only_versioned_mumcord_1_imports_are_allowed() {
        assert!(import_allowed("mumcord:plugin/log@1.0.0"));
        assert!(import_allowed("mumcord:plugin/events@1.3.0"));
        assert!(!import_allowed("mumcord:plugin/log@2.0.0"));
        assert!(!import_allowed("mumcord:plugin/log"));
        assert!(!import_allowed("mumcord:plugin/nope@1.0.0"));
        assert!(!import_allowed("wasi:filesystem/types@0.2.0"));
    }

    fn peer(session: u32) -> Peer {
        Peer {
            session,
            name: format!("user{session}"),
            key: None,
        }
    }

    fn queue() -> (EventQueue, Arc<Diagnostics>) {
        let diag = Arc::new(Diagnostics::default());
        (EventQueue::new(Arc::clone(&diag)), diag)
    }

    fn drain(state: &mut QueueState, now: Instant) -> Vec<Event> {
        std::iter::from_fn(|| state.pop(now).map(|(event, _)| event)).collect()
    }

    #[test]
    fn full_queue_drops_the_oldest_non_ui_event_and_counts_it() {
        let (queue, diag) = queue();
        queue.push(Event::DialogClosed);
        for i in 0..QUEUE_CAPACITY - 1 {
            queue.push(Event::Timer(i as u32));
        }
        queue.push(Event::Timer(999));
        assert_eq!(diag.snapshot().dropped_events, 1);
        let events = drain(&mut queue.lock(), Instant::now());
        assert_eq!(events.len(), QUEUE_CAPACITY);
        assert!(matches!(events[0], Event::DialogClosed));
        assert!(matches!(events[1], Event::Timer(1)), "timer 0 was dropped");
        assert!(matches!(events.last(), Some(Event::Timer(999))));
    }

    #[test]
    fn a_queue_full_of_ui_events_drops_the_oldest_ui_event() {
        let (queue, diag) = queue();
        for _ in 0..QUEUE_CAPACITY {
            queue.push(Event::DialogClosed);
        }
        queue.push(Event::Timer(7));
        assert_eq!(diag.snapshot().dropped_events, 1);
        let events = drain(&mut queue.lock(), Instant::now());
        assert_eq!(events.len(), QUEUE_CAPACITY);
        assert!(matches!(events.last(), Some(Event::Timer(7))));
    }

    #[test]
    fn talking_is_coalesced_per_user_and_limited_per_second() {
        let (queue, diag) = queue();
        queue.push(Event::Talking((peer(1), true)));
        queue.push(Event::Timer(1));
        queue.push(Event::Talking((peer(1), false)));
        for session in 2..=12 {
            queue.push(Event::Talking((peer(session), true)));
        }
        assert_eq!(diag.snapshot().dropped_events, 0);

        let mut state = queue.lock();
        let now = Instant::now();
        let first = drain(&mut state, now);
        // User 1 keeps its place but shows only the newest state; then the timer, then talking
        // up to the limit.
        assert!(matches!(&first[0], Event::Talking((p, false)) if p.session == 1));
        assert!(matches!(first[1], Event::Timer(1)));
        assert_eq!(first.len(), 1 + TALKING_PER_SECOND);
        assert_eq!(state.talking_wake_at(), Some(now + Duration::from_secs(1)));

        let later = drain(&mut state, now + Duration::from_secs(1));
        assert_eq!(later.len(), 2);
        assert!(matches!(&later[1], Event::Talking((p, true)) if p.session == 12));
    }

    #[test]
    fn backing_off_drops_waiting_and_new_events() {
        let (queue, diag) = queue();
        queue.push(Event::Timer(1));
        queue.push(Event::Talking((peer(1), true)));
        queue.set_accepting(false);
        assert!(!queue.push(Event::Timer(2)));
        assert_eq!(diag.snapshot().dropped_events, 3);
        queue.set_accepting(true);
        assert!(queue.push(Event::Timer(3)));
        assert!(matches!(queue.pop(), Next::Event(e, _) if matches!(*e, Event::Timer(3))));
    }

    #[test]
    fn stop_wins_over_waiting_events() {
        let (queue, _) = queue();
        queue.push(Event::Timer(1));
        queue.request_stop();
        assert!(matches!(queue.pop(), Next::Stop));
        assert!(queue.wait_for_stop(Instant::now() + Duration::from_secs(60)));
    }
}
