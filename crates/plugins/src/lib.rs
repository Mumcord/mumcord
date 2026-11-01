//! Mumcord's plugin host (Epic 3.1): a sandboxed WebAssembly runtime, the plugin manifest and
//! the `.mcplugin` package format.
//!
//! The plugin API is the WIT package in `wit/`; the manifest rules are also published as a JSON
//! schema in `schema/manifest.schema.json`.

mod health;
mod host;
pub mod i18n;
pub mod manifest;
pub mod messaging;
pub mod package;
pub mod permissions;
mod runtime;
pub mod verify;
pub mod version;

pub use health::{Health, Recovery};
pub use host::{HostError, LoadError, PluginHost, PluginState};
pub use manifest::{Manifest, ManifestError, Source};
pub use package::{Digest, ExpectedDigest, Package, PackageError, PackageVerifier, VerifyError};
pub use runtime::{CallError, CompileError, Level, LogBuffer};
pub use version::{API_VERSION, Incompatibility};
