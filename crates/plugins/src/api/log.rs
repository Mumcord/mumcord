//! `log` (FR-6.26): lines go only to the plugin's ring buffer, never to Mumcord's log files.

use std::sync::PoisonError;

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::log::{Host, Level, add_to_linker};
use crate::runtime::{HostApi, PluginState};

pub(crate) struct LogApi;

impl HostApi for LogApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn write(&mut self, level: Level, message: String) {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(level, &message);
    }
}
