//! Mumcord's plugin host (Epic 3.1): the plugin manifest and the `.mcplugin` package format.
//!
//! The plugin API is the WIT package in `wit/`; the manifest rules are also published as a JSON
//! schema in `schema/manifest.schema.json`.

pub mod i18n;
pub mod manifest;
pub mod package;
pub mod version;

pub use manifest::{Manifest, ManifestError};
pub use package::{Digest, ExpectedDigest, Package, PackageError, PackageVerifier, VerifyError};
pub use version::{API_VERSION, Incompatibility};
