//! Third-party repositories (FR-6.74 … FR-6.76): address rules, the add preview with TOFU key
//! pinning and "Expected key", and fetching with key-change detection and signed rotation.

use std::path::Path;

use reqwest::Url;
use serde::Serialize;

use super::cache::Cache;
use super::fetch::{Client, UrlPolicy};
use super::index::{Ignored, Index, IndexKind, RepositoryInfo, declared_key};
use super::keys::{key_fingerprint, normalize_key};
use super::sync::{Freshness, Loaded, fetch_signed};
use super::{INDEX_FILE, MAX_REPOSITORY_INDEX_BYTES, RegistryError};
use crate::verify::{RegistryKeys, check_sequence};
use mumcord_store::PluginRepository;

/// Validates the address typed into the add dialog: the URL of the repository's `index.json`;
/// its signature lives at the same URL plus `.minisig`.
pub fn check_repository_url(text: &str, developer_mode: bool) -> Result<Url, RegistryError> {
    let url = Url::parse(text.trim()).map_err(|_| RegistryError::InvalidUrl)?;
    if url.fragment().is_some() {
        return Err(RegistryError::InvalidUrl);
    }
    UrlPolicy::new(developer_mode).check(&url)?;
    Ok(url)
}

/// What the add dialog shows before the user confirms (FR-6.74).
#[derive(Debug, Clone, Serialize)]
pub struct RepoPreview {
    pub url: String,
    /// Name, homepage, contact and the (normalised) key that will be pinned.
    pub info: RepositoryInfo,
    /// minisign key id, e.g. `1CB49831E26E61AD`.
    pub fingerprint: String,
    pub plugin_count: usize,
    pub ignored: Vec<Ignored>,
}

/// Fetches and verifies a repository that is about to be added. The key it is signed with is
/// the one to pin (trust on first use); with an "Expected key" it must be exactly that key.
pub async fn preview_repository(
    client: &Client,
    url: &str,
    expected_key: Option<&str>,
    is_official_id: &dyn Fn(&str) -> bool,
) -> Result<RepoPreview, RegistryError> {
    let url = check_repository_url(url, client.policy().allow_localhost_http)?;
    let bytes = client.get_body(&url, MAX_REPOSITORY_INDEX_BYTES).await?;
    let signature = client.get_signature(&url).await?;
    let key = declared_key(&bytes)?;
    if let Some(expected) = expected_key.map(str::trim).filter(|e| !e.is_empty())
        && normalize_key(expected).as_deref() != Some(key.as_str())
    {
        return Err(RegistryError::ExpectedKeyMismatch);
    }
    verifier(&key)?.verify(&bytes, &signature)?;
    let index = Index::parse(&bytes, &IndexKind::Repository { is_official_id })?;
    let info = index
        .repository
        .expect("repository indexes carry the block");
    Ok(RepoPreview {
        url: url.to_string(),
        fingerprint: key_fingerprint(&key).unwrap_or_default(),
        info,
        plugin_count: index.plugins.len(),
        ignored: index.ignored,
    })
}

/// Result of refreshing an added repository. The caller persists: `last_sequence` /
/// `last_checked` for `Loaded`, `replace_plugin_repository_key` for `Rotated`,
/// `pause_plugin_repository(.., "key-changed")` for `KeyChanged`.
#[derive(Debug)]
pub enum RepoOutcome {
    Loaded(Loaded),
    /// FR-6.75: the new key carries a `key_rotation` signed by the pinned key; pin `new_key`.
    Rotated {
        new_key: String,
        loaded: Loaded,
    },
    /// FR-6.75: pause the repository and ask the user.
    KeyChanged {
        new_key: String,
        fingerprint: String,
    },
}

/// Refreshes `repository`. `cache_dir` is this repository's own offline copy directory.
pub async fn fetch_repository(
    client: &Client,
    cache_dir: &Path,
    repository: &PluginRepository,
    is_official_id: &dyn Fn(&str) -> bool,
) -> Result<RepoOutcome, RegistryError> {
    let url = check_repository_url(&repository.url, client.policy().allow_localhost_http)?;
    let pinned = normalize_key(&repository.public_key)
        .ok_or_else(|| RegistryError::Format("the pinned key is not a minisign key".into()))?;
    let last_seen = u64::try_from(repository.last_sequence).ok();
    let accept = |bytes: &[u8], signature: &str| {
        accept_repository(bytes, signature, &pinned, last_seen, is_official_id)
    };
    let cache = Cache::new(cache_dir, INDEX_FILE);
    let result = fetch_signed(client, &url, MAX_REPOSITORY_INDEX_BYTES, &cache, accept).await;
    let key_changed = |new_key: String| RepoOutcome::KeyChanged {
        fingerprint: key_fingerprint(&new_key).unwrap_or_default(),
        new_key,
    };
    Ok(match result {
        Err(RegistryError::KeyChanged { new_key })
        | Ok((_, Freshness::Rejected(RegistryError::KeyChanged { new_key }))) => {
            key_changed(new_key)
        }
        Ok(((index, None), freshness)) => RepoOutcome::Loaded(Loaded { index, freshness }),
        Ok(((index, Some(new_key)), freshness)) => RepoOutcome::Rotated {
            new_key,
            loaded: Loaded { index, freshness },
        },
        Err(error) => return Err(error),
    })
}

/// Checks one copy of a repository index against the pinned key. Returns the index and, after a
/// valid rotation, the new key to pin.
fn accept_repository(
    bytes: &[u8],
    signature: &str,
    pinned: &str,
    last_seen: Option<u64>,
    is_official_id: &dyn Fn(&str) -> bool,
) -> Result<(Index, Option<String>), RegistryError> {
    let declared = declared_key(bytes)?;
    // The file must be signed by the key it declares, whichever key that is.
    verifier(&declared)?.verify(bytes, signature)?;
    let index = Index::parse(bytes, &IndexKind::Repository { is_official_id })?;
    let rotated = if declared == pinned {
        None
    } else if rotation_is_valid(&index, &declared, pinned) {
        Some(declared)
    } else {
        return Err(RegistryError::KeyChanged { new_key: declared });
    };
    check_sequence(index.sequence, last_seen)?;
    Ok((index, rotated))
}

/// FR-6.75: `key_rotation.signature` is the pinned key's signature over the new key's base64
/// line, and `new_key` is the key the index is signed with.
fn rotation_is_valid(index: &Index, declared: &str, pinned: &str) -> bool {
    let Some(rotation) = &index.key_rotation else {
        return false;
    };
    normalize_key(&rotation.new_key).as_deref() == Some(declared)
        && verifier(pinned)
            .and_then(|keys| Ok(keys.verify(declared.as_bytes(), &rotation.signature)?))
            .is_ok()
}

fn verifier(key: &str) -> Result<RegistryKeys, RegistryError> {
    RegistryKeys::pinned(key)
        .ok_or_else(|| RegistryError::Format("the repository key is not a minisign key".into()))
}
