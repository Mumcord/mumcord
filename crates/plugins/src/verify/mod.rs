//! Signing and verification (FR-6.49 … FR-6.53).
//!
//! - [`keys`]: minisign repository keys. The official index and `revoked.json` are signed with the
//!   key compiled into Mumcord; third-party repositories use the key pinned when they were added.
//! - [`install`]: the install checks in the fixed order size → SHA-256 → author signature →
//!   manifest vs. index entry, and the official status that follows from them.
//! - [`integrity`]: installed plugins are re-hashed once per Mumcord version, so changed files
//!   disable the plugin.
//!
//! Failures carry the exact user-facing texts of spec §5.7.

pub mod install;
pub mod integrity;
pub mod keys;

pub use install::{AUTHOR_SIGNATURE_SUFFIX, IndexEntry, Verified, verify_package};
pub use integrity::{Integrity, TAMPERED_TEXT, app_version, fingerprint};
pub use keys::{RegistryKeys, check_sequence};

pub const MISMATCH_TEXT: &str = "Download didn't match the official list";
pub const SIGNATURE_TEXT: &str = "Signature check failed";
pub const PERMISSIONS_TEXT: &str = "This plugin asks for permissions not listed in the repository";

#[derive(Debug, thiserror::Error)]
pub enum VerifyFailure {
    /// Size, hash, id or version differ from the index entry.
    #[error("{MISMATCH_TEXT}")]
    Mismatch,
    /// Repository or author signature missing, malformed or made with another key.
    #[error("{SIGNATURE_TEXT}")]
    Signature,
    /// The manifest asks for a permission the index entry does not list.
    #[error("{PERMISSIONS_TEXT}")]
    Permissions,
    /// The index is older than one already seen (rollback protection, FR-6.55). The caller keeps
    /// its cached list.
    #[error("The plugin list is older than the one already seen")]
    Rollback,
    /// The bytes passed every check but are not a valid package.
    #[error(transparent)]
    Package(#[from] crate::PackageError),
}
