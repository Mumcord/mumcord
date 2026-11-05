//! Mumcord's plugin host (Epic 3.1): a sandboxed WebAssembly runtime, the plugin manifest and
//! the `.mcplugin` package format.
//!
//! The plugin API is the WIT package in `wit/<version>/` (one folder per minor); the manifest
//! rules are also published as a JSON schema in `schema/manifest.schema.json`.

pub mod api;
pub mod cache;
pub mod chat;
pub mod diag;
pub mod grants;
mod health;
mod host;
pub mod i18n;
pub mod manifest;
pub mod messaging;
mod net_guard;
pub mod origins;
pub mod package;
pub mod patterns;
pub mod peer_key;
pub mod permissions;
pub mod registry;
mod runtime;
pub mod score;
pub mod service;
pub mod sounds;
pub mod stage;
pub mod suggestions;
pub mod test_hooks;
pub mod triggers;
pub mod verify;
pub mod version;

pub use api::io::PluginIo;
pub use diag::{DiagSnapshot, Diagnostics, LoadIssue};
pub use health::{BACKOFF, Health, Recovery};
pub use host::{HostError, HostEvent, LoadError, PluginHost, RunState};
pub use manifest::{Manifest, ManifestError, Source};
pub use package::{Digest, ExpectedDigest, Package, PackageError, PackageVerifier, VerifyError};
pub use runtime::{
    CallError, CompileError, HostContext, Level, LogBuffer, QUEUE_CAPACITY, TALKING_PER_SECOND, wit,
};
pub use version::{API_VERSION, ApiMinor, Incompatibility};
