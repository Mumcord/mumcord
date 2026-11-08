//! The host side of `mumcord:plugin@1.1.0` (FR-9.5 … FR-9.9, FR-9.24).
//!
//! 1.1 only adds to 1.0, so the 1.1 interfaces delegate to the 1.0 host implementations in the
//! other `api/*` modules. Bindgen makes separate Rust types per package, so every value crosses
//! with [`Up`] (1.0 to 1.1, always possible) or [`Down`] (1.1 to 1.0, `None` for a case 1.1
//! added, such as the `stage` slot). The conversions are generated from the 1.0 WIT with the
//! `record!` and `variant!` macros in `minor_host.rs`; bindgen's `with` cannot share plain types between
//! packages.
//!
//! Events are queued as 1.1 events (the superset): a 1.1 plugin gets them as they are, a 1.0
//! plugin gets them converted back and never sees a 1.1-only event (FR-9.8).

use crate::api::host_calls;
use crate::runtime::PluginState;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({ path: "wit/1.1.0", world: "plugin" });
}

/// The 1.1 WIT types as Rust types.
pub use bindings::mumcord::plugin as wit;

// Compiled once per minor on purpose: each needs its own bindgen types.
#[allow(clippy::duplicate_mod)]
#[path = "minor_host.rs"]
mod host;
pub use host::{Down, Up, add_to_linker, relay_status};

impl PluginState {
    /// Waits for the user in a host dialog (FR-9.103, FR-9.105). The plugin handles no events
    /// meanwhile; its queue stays bounded and drops the oldest, so only their count is logged.
    pub(crate) fn wait_for_user<T>(&mut self, waiter: host_calls::Waiter<T>) -> Option<T> {
        let dropped = self.diag.snapshot().dropped_events;
        let answer = self
            .clock
            .waiting_on_user(|| waiter.wait(host_calls::DIALOG_WAIT_CAP));
        let dropped = self.diag.snapshot().dropped_events.saturating_sub(dropped);
        if dropped > 0 {
            tracing::info!(plugin = %self.id, dropped, "plugin events dropped while a dialog was open");
        }
        answer
    }
}
