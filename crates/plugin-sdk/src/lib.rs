//! SDK for Mumcord plugins: Rust bindings to the `mumcord:plugin@1.0.0` WIT package plus small
//! helpers for plugin authors.
//!
//! A plugin is a library crate with `crate-type = ["cdylib"]`, built for
//! `wasm32-unknown-unknown`. `mumcord-plugin build` then turns the module into a WebAssembly
//! component. That target is used instead of `wasm32-wasip2` because Rust's standard library
//! on WASI imports WASI interfaces (stderr, exit, environment, …) that Mumcord never provides,
//! so such a plugin would be refused at load.
//!
//! Implement [`Plugin`] and register it with [`export!`]:
//!
//! ```no_run
//! use mumcord_plugin_sdk::{events::Event, log, types::StartInfo, ui, Plugin};
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn start(_info: StartInfo) -> Result<(), String> {
//!         log::info("hello started");
//!         let tree = ui::column()
//!             .child(ui::heading("Hello"))
//!             .child(ui::button("wave", "Wave"));
//!         ui::show(ui::Slot::Panel, tree).map_err(|e| e.to_string())
//!     }
//!
//!     fn on_event(event: Event) {
//!         if let Event::UiAction(action) = event {
//!             if action.action == "wave" {
//!                 log::info("someone waved");
//!             }
//!         }
//!     }
//!
//!     fn stop() {}
//! }
//!
//! mumcord_plugin_sdk::export!(Hello);
//! ```
//!
//! Chat triggers: a plugin that declares `"triggers": ["!poll"]` in its manifest and holds
//! `chat.triggers` gets [`events::Event::ChatTrigger`] when someone in the user's channel (on any
//! Mumble client) writes a line starting with that word, e.g. `!poll 2`; `args` is the rest of
//! the line (untrusted, at most 200 characters). No other chat text ever reaches a plugin, and a
//! word another plugin installed earlier already declares is not delivered.
//!
//! Every host call is checked against the permissions the user granted; a missing permission
//! returns [`types::Error::NotPermitted`]. The meaning and limits of each call are documented on
//! the generated functions and in the WIT files.

use std::fmt;

/// Generated bindings. Public only because [`export!`] refers to them; use the re-exports below.
#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        world: "plugin",
        // The canonical WIT lives in the plugins crate; the SDK never keeps its own copy.
        path: "../plugins/wit",
        pub_export_macro: true,
        default_bindings_module: "mumcord_plugin_sdk::bindings",
    });
}

/// The three functions every plugin exports: `start`, `on_event` and `stop`.
pub use bindings::Guest as Plugin;

/// Registers the type that implements [`Plugin`] as the plugin's exports:
/// `mumcord_plugin_sdk::export!(MyPlugin);` once per plugin crate.
pub use bindings::export;

pub use bindings::mumcord::plugin::{
    chat, clock, events, http, notify, random, secrets, theme, timers, types, webview,
};

pub mod i18n;
pub mod log;
pub mod peers;
pub mod storage;
pub mod ui;

/// The error of the JSON helpers in [`storage`] and [`peers`].
#[derive(Debug)]
pub enum JsonError {
    /// The host refused the call.
    Host(types::Error),
    /// The value could not be encoded or decoded as JSON.
    Json(serde_json::Error),
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(e) => write!(f, "host error: {e:?}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for JsonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Host(_) => None,
            Self::Json(e) => Some(e),
        }
    }
}

impl From<types::Error> for JsonError {
    fn from(e: types::Error) -> Self {
        Self::Host(e)
    }
}

impl From<serde_json::Error> for JsonError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
