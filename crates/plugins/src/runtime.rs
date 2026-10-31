//! The wasmtime sandbox (#268). A plugin is a WebAssembly component that can only call the
//! imports in `wit/`: there is no WASI, so no files, sockets, clocks or randomness. Each running
//! plugin has its own `Store` with a memory limit, and every call into it has a CPU deadline
//! enforced by epoch interruption.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use wasmtime::component::{Component, HasSelf, InstancePre, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};

mod bindings {
    wasmtime::component::bindgen!({ path: "wit", world: "plugin" });
}

pub use bindings::mumcord::plugin::log::Level;
use bindings::mumcord::plugin::types::StartInfo;
use bindings::{Plugin, PluginPre};

/// How often the CPU clock ticks.
pub const EPOCH_TICK: Duration = Duration::from_millis(10);
/// Ticks a normal call (event, timer, stop) may use: 50 ms.
pub const CALL_TICKS: u64 = 5;
/// Ticks `start` may use: 500 ms.
pub const START_TICKS: u64 = 50;
pub const MAX_TABLE_ELEMENTS: usize = 10_000;
/// Core instances per plugin. A component from wit-bindgen is a few core modules (the plugin
/// plus small adapter shims), so this is more than one but still tight.
const MAX_INSTANCES: usize = 16;
const MAX_TABLES: usize = 8;
const MAX_MEMORIES: usize = 1;

/// Plugin log buffer size: lines, and characters per line.
pub const LOG_LINES: usize = 200;
pub const LOG_LINE_CHARS: usize = 200;

/// Interfaces a plugin may import, without the `@version` suffix. The linker then checks that
/// the requested version is compatible (same major, not newer than the host).
const ALLOWED_IMPORTS: &[&str] = &["mumcord:plugin/types", "mumcord:plugin/log"];

/// One wasmtime engine for all plugins plus the thread that ticks the CPU clock.
pub struct Runtime {
    engine: Engine,
    linker: Linker<PluginState>,
    ticker: Option<JoinHandle<()>>,
    stop_ticker: Arc<AtomicBool>,
}

/// Why a component cannot be loaded.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("the plugin is not a valid WebAssembly component: {0}")]
    Invalid(String),
    #[error(
        "the plugin needs {0:?}, which Mumcord does not provide (plugins get no WASI, files or network)"
    )]
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

/// A compiled plugin, checked against the API and ready to start.
#[derive(Clone)]
pub struct Compiled {
    pre: PluginPre<PluginState>,
}

/// One running plugin: its store and instance.
pub struct Instance {
    store: Store<PluginState>,
    plugin: Plugin,
}

/// What the host keeps per running plugin, reachable from host functions.
pub struct PluginState {
    limits: StoreLimits,
    log: Arc<Mutex<LogBuffer>>,
}

impl Runtime {
    pub fn new() -> Result<Self, CompileError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|e| CompileError::Invalid(e.to_string()))?;

        let mut linker = Linker::new(&engine);
        bindings::mumcord::plugin::log::add_to_linker::<PluginState, HasSelf<PluginState>>(
            &mut linker,
            |s| s,
        )
        .map_err(|e| CompileError::Invalid(e.to_string()))?;

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
            ticker: Some(ticker),
            stop_ticker,
        })
    }

    /// Compiles a component and checks its imports before any of its code runs.
    pub fn compile(&self, wasm: &[u8]) -> Result<Compiled, CompileError> {
        let component = Component::new(&self.engine, wasm)
            .map_err(|e| CompileError::Invalid(format!("{e:#}")))?;
        for (name, _) in component.component_type().imports(&self.engine) {
            let interface = name
                .split_once('@')
                .map_or(name, |(interface, _)| interface);
            if !ALLOWED_IMPORTS.contains(&interface) {
                return Err(CompileError::UnknownImport(name.to_owned()));
            }
        }
        let pre: InstancePre<PluginState> = self
            .linker
            .instantiate_pre(&component)
            .map_err(|e| CompileError::Mismatch(format!("{e:#}")))?;
        let pre = PluginPre::new(pre).map_err(|e| CompileError::Mismatch(format!("{e:#}")))?;
        Ok(Compiled { pre })
    }

    /// Instantiates a compiled plugin and calls its `start`.
    pub fn start(
        &self,
        compiled: &Compiled,
        memory_bytes: usize,
        log: Arc<Mutex<LogBuffer>>,
        previous_version: Option<String>,
    ) -> Result<Instance, CallError> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(memory_bytes)
            .table_elements(MAX_TABLE_ELEMENTS)
            .instances(MAX_INSTANCES)
            .tables(MAX_TABLES)
            .memories(MAX_MEMORIES)
            // A failed memory.grow would otherwise just return -1 and let the plugin carry on.
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(&self.engine, PluginState { limits, log });
        store.limiter(|state| &mut state.limits);
        store.epoch_deadline_trap();

        store.set_epoch_deadline(START_TICKS);
        let plugin = compiled.pre.instantiate(&mut store).map_err(classify)?;
        let info = StartInfo {
            api_version: crate::API_VERSION.to_string(),
            previous_version,
        };
        plugin
            .call_start(&mut store, &info)
            .map_err(classify)?
            .map_err(|reason| CallError::Refused(reason.chars().take(LOG_LINE_CHARS).collect()))?;
        Ok(Instance { store, plugin })
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

impl Instance {
    /// Calls `stop`. The instance is dropped afterwards either way.
    pub fn stop(mut self) -> Result<(), CallError> {
        self.store.set_epoch_deadline(CALL_TICKS);
        self.plugin.call_stop(&mut self.store).map_err(classify)
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

impl bindings::mumcord::plugin::types::Host for PluginState {}

impl bindings::mumcord::plugin::log::Host for PluginState {
    fn write(&mut self, level: Level, message: String) {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(level, &message);
    }
}

/// The last [`LOG_LINES`] lines a plugin logged, each cut to [`LOG_LINE_CHARS`] characters.
/// Kept apart from the app log so plugins cannot write into it.
#[derive(Debug, Default)]
pub struct LogBuffer {
    lines: VecDeque<(Level, String)>,
}

impl LogBuffer {
    fn push(&mut self, level: Level, message: &str) {
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
}
