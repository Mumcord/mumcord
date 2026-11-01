//! Registry and repository client (FR-6.55, FR-6.57, FR-6.70, FR-6.74 … FR-6.76, FR-6.80).
//!
//! Fetches and verifies the signed official index, third-party repository indexes and the
//! revocation list. The wire format is documented in `docs/plugin-registry-format.md`.
//!
//! Nothing here touches the store: callers pass what they remember (last sequence, pinned key)
//! and persist what comes back, so the client stays usable from any task.

mod cache;
mod fetch;
mod index;
mod keys;
mod media;
mod repo;
mod revoked;
mod sync;

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod test_server;

pub use fetch::{Client, Download, UrlPolicy};
pub use index::{
    IGNORED_PREFIX_TEXT, Ignored, IgnoredReason, Index, IndexKind, IndexPlugin, IndexVersion,
    KeyRotation, RepositoryInfo, Screenshot,
};
pub use keys::{key_fingerprint, key_fingerprint_prefix, normalize_key};
pub use media::{Image, MAX_ICON_BYTES, MAX_SCREENSHOT_BYTES, sniff_image};
pub use repo::{
    RepoOutcome, RepoPreview, check_repository_url, fetch_repository, preview_repository,
};
pub use revoked::{Revocation, RevocationList, Severity};
pub use sync::{Freshness, Loaded, RevokedLoaded, fetch_official, fetch_revoked, official_base};

use crate::verify::VerifyFailure;

/// Where the official registry lives (FR-6.55). The mirror setting replaces this base.
pub const OFFICIAL_BASE_URL: &str = "https://mumcord.github.io/plugins/v1/";
pub const INDEX_FILE: &str = "index.json";
pub const REVOKED_FILE: &str = "revoked.json";
/// Detached minisign signatures sit next to the signed file under this suffix.
pub const SIGNATURE_SUFFIX: &str = ".minisig";
/// The only wire format version this client reads.
pub const FORMAT_VERSION: u32 = 1;

/// The official index lists every reviewed plugin; generous, but bounded.
pub const MAX_OFFICIAL_INDEX_BYTES: usize = 8 * 1024 * 1024;
/// FR-6.76.
pub const MAX_REPOSITORY_INDEX_BYTES: usize = 2 * 1024 * 1024;
/// FR-6.76.
pub const MAX_REPOSITORY_PLUGINS: usize = 500;
pub const MAX_REVOKED_BYTES: usize = 1024 * 1024;
/// A minisign signature file is a few hundred bytes.
pub const MAX_SIGNATURE_BYTES: usize = 4 * 1024;

/// Spec §5.7, shown when there is neither a fresh nor a cached list.
pub const UNREACHABLE_TEXT: &str = "Can't load the plugin list. Check your connection.";
/// Spec §5.7, shown while the cached list is used.
pub const OFFLINE_TEXT: &str = "Showing the last known plugin list (offline)";

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// No connection and nothing cached.
    #[error("{UNREACHABLE_TEXT}")]
    Unreachable,
    /// The server answered with something other than 200/304.
    #[error("The plugin list server answered with status {0}")]
    Status(u16),
    #[error("The plugin list is larger than {0} bytes")]
    TooLarge(usize),
    #[error("The plugin list is not valid: {0}")]
    Format(String),
    #[error(transparent)]
    Verify(#[from] VerifyFailure),
    /// FR-6.74: https only; plain http only for localhost in Developer mode.
    #[error("Repository addresses must start with https://")]
    InsecureUrl,
    #[error("This is not a valid repository address")]
    InvalidUrl,
    /// FR-6.74: the key the repository presents differs from the "Expected key".
    #[error("The repository key doesn't match the expected key")]
    ExpectedKeyMismatch,
    /// FR-6.75: the repository now signs with another key and has no valid `key_rotation`.
    #[error("The repository key changed")]
    KeyChanged { new_key: String },
    #[error("The HTTP client could not be set up: {0}")]
    Setup(String),
    #[error("The plugin list cache could not be written: {0}")]
    Cache(#[from] std::io::Error),
}
