//! Fetching the official index and the revocation list with offline fallback (FR-6.55,
//! FR-6.57, FR-6.70), and the index path rules of FR-9.14 / FR-9.19.

use std::path::{Path, PathBuf};

use mumcord_store::{Store, UnixMillis, VersionHistoryEntry};
use reqwest::Url;

use super::cache::{Cache, Cached};
use super::fetch::{Client, Response};
use super::index::{Index, IndexKind};
use super::revoked::RevocationList;
use super::{
    INDEX_FILE, MAX_OFFICIAL_INDEX_BYTES, MAX_REVOKED_BYTES, OFFICIAL_BASE_URL, REVOKED_FILE,
    RegistryError,
};
use crate::verify::{RegistryKeys, check_sequence};

/// The offline cache for a path, or a no-op cache when the user turned it off
/// (spec 12 FR-12.11).
fn cache(dir: &Path, name: &'static str, enabled: bool) -> Cache {
    if enabled {
        Cache::new(dir, name)
    } else {
        Cache::disabled(dir, name)
    }
}

/// Where a loaded list came from.
#[derive(Debug)]
pub enum Freshness {
    /// Downloaded and verified just now.
    Fresh,
    /// The server confirmed the cached copy (ETag).
    NotModified,
    /// The server could not be reached; the cached copy is shown (spec §5.7 offline text).
    Offline,
    /// A new copy arrived but failed a check (signature, rollback, format, size); the cached
    /// copy is kept (AC-6.18).
    Rejected(RegistryError),
}

#[derive(Debug)]
pub struct Loaded {
    pub index: Index,
    pub freshness: Freshness,
}

#[derive(Debug)]
pub struct RevokedLoaded {
    /// The bundled copy merged with the fetched or cached one.
    pub list: RevocationList,
    pub freshness: Freshness,
}

/// Replaces the official base URL in development builds, so E2E runs can use a local fixture
/// registry. Release builds never read it.
pub const TEST_URL_ENV: &str = "MUMCORD_PLUGIN_REGISTRY_URL";

/// The official base URL, or the mirror from Settings → Plugins → Repositories → Advanced
/// (FR-6.57). Mirrored files are still verified with the official keys. Development builds
/// also take the base from [`TEST_URL_ENV`]; a mirror setting wins over it.
pub fn official_base(mirror: Option<&str>) -> Result<Url, RegistryError> {
    #[cfg(debug_assertions)]
    let test_url = std::env::var(TEST_URL_ENV).ok();
    #[cfg(not(debug_assertions))]
    let test_url: Option<String> = None;
    official_base_with(mirror, test_url.as_deref())
}

fn official_base_with(mirror: Option<&str>, test_url: Option<&str>) -> Result<Url, RegistryError> {
    let text = [mirror, test_url]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|m| !m.is_empty())
        .unwrap_or(OFFICIAL_BASE_URL);
    let text = if text.ends_with('/') {
        text.to_owned()
    } else {
        format!("{text}/")
    };
    Url::parse(&text).map_err(|_| RegistryError::InvalidUrl)
}

/// The folder of the format-2 index next to a `v1/` folder (FR-9.14).
pub(super) const V1_1_DIR: &str = "v1.1";

/// The `v1.1` folder next to `url` when `url`'s path ends in `/v1/` + `file`, e.g.
/// `…/v1/index.json` → `…/v1.1/index.json` (FR-9.14, FR-9.19).
pub(super) fn v1_1_sibling(url: &Url, file: &str) -> Option<Url> {
    let path = url.path().strip_suffix(file)?.strip_suffix("v1/")?;
    if !path.ends_with('/') {
        return None;
    }
    let mut sibling = url.clone();
    sibling.set_path(&format!("{path}{V1_1_DIR}/{file}"));
    Some(sibling)
}

/// The offline copy of one (source, path): the `v1.1` copy gets its own folder, so each path
/// keeps its own cached copy and rollback floor (spec §7.1).
pub(super) fn path_cache_dir(source_dir: &Path, v1_1: bool) -> PathBuf {
    if v1_1 {
        source_dir.join(V1_1_DIR)
    } else {
        source_dir.to_owned()
    }
}

/// The rollback floor of one (source, path): the sequence of its verified cached copy, raised to
/// `stored` (what the caller remembers for this path, if anything).
pub(super) fn path_floor(
    cache: &Cache,
    read: impl Fn(&[u8], &str) -> Result<u64, RegistryError>,
    stored: Option<u64>,
) -> Option<u64> {
    let cached = cache
        .load()
        .and_then(|cached| read(&cached.bytes, &cached.signature).ok());
    cached.max(stored)
}

/// Fetches the official index. With a base ending in `/v1/` (the default, and mirrors with the
/// same layout) that is the format-2 `v1.1/index.json` next to it (FR-9.14); any other base is
/// read as is. `last_seen` is `plugins.last_official_sequence`; both official indexes carry the
/// same sequence, so it applies to either path. The caller stores `index.sequence` afterwards.
/// `cache_dir` holds the offline copies.
pub async fn fetch_official(
    client: &Client,
    cache_dir: &Path,
    base: &Url,
    last_seen: Option<u64>,
    cache_enabled: bool,
) -> Result<Loaded, RegistryError> {
    fetch_official_with(
        client,
        cache_dir,
        base,
        last_seen,
        cache_enabled,
        &RegistryKeys::official(),
    )
    .await
}

pub(super) async fn fetch_official_with(
    client: &Client,
    cache_dir: &Path,
    base: &Url,
    last_seen: Option<u64>,
    cache_enabled: bool,
    keys: &RegistryKeys,
) -> Result<Loaded, RegistryError> {
    let added = base
        .join(INDEX_FILE)
        .map_err(|_| RegistryError::InvalidUrl)?;
    let sibling = v1_1_sibling(&added, INDEX_FILE);
    let cache = cache(
        &path_cache_dir(cache_dir, sibling.is_some()),
        INDEX_FILE,
        cache_enabled,
    );
    let url = sibling.unwrap_or(added);
    let verify = |bytes: &[u8], signature: &str| {
        keys.verify(bytes, signature)?;
        Index::parse(bytes, &IndexKind::Official)
    };
    let floor = path_floor(&cache, |b, s| verify(b, s).map(|i| i.sequence), last_seen);
    let accept = |bytes: &[u8], signature: &str| {
        let index = verify(bytes, signature)?;
        check_sequence(index.sequence, floor)?;
        Ok(index)
    };
    let (index, freshness) = fetch_signed(
        client,
        &url,
        MAX_OFFICIAL_INDEX_BYTES,
        &cache,
        Missing::Offline,
        accept,
    )
    .await?;
    Ok(Loaded { index, freshness })
}

/// Fetches `revoked.json` (daily, even with updates off, FR-6.70). Never fails for lack of a
/// connection: the bundled copy always applies.
pub async fn fetch_revoked(
    client: &Client,
    cache_dir: &Path,
    base: &Url,
    cache_enabled: bool,
) -> Result<RevokedLoaded, RegistryError> {
    fetch_revoked_with(
        client,
        cache_dir,
        base,
        cache_enabled,
        &RegistryKeys::official(),
    )
    .await
}

pub(super) async fn fetch_revoked_with(
    client: &Client,
    cache_dir: &Path,
    base: &Url,
    cache_enabled: bool,
    keys: &RegistryKeys,
) -> Result<RevokedLoaded, RegistryError> {
    let url = base
        .join(REVOKED_FILE)
        .map_err(|_| RegistryError::InvalidUrl)?;
    let cache = cache(cache_dir, REVOKED_FILE, cache_enabled);
    let verify = |bytes: &[u8], signature: &str| {
        keys.verify(bytes, signature)?;
        RevocationList::parse(bytes)
    };
    let cached_sequence = cache
        .load()
        .and_then(|cached| verify(&cached.bytes, &cached.signature).ok())
        .map(|list| list.sequence);
    let accept = |bytes: &[u8], signature: &str| {
        let list = verify(bytes, signature)?;
        check_sequence(list.sequence, cached_sequence)?;
        Ok(list)
    };
    let mut list = RevocationList::bundled();
    let fetched = fetch_signed(
        client,
        &url,
        MAX_REVOKED_BYTES,
        &cache,
        Missing::Offline,
        accept,
    )
    .await;
    let freshness = match fetched {
        Ok((fetched, freshness)) => {
            list.merge(fetched);
            freshness
        }
        Err(RegistryError::Unreachable) => Freshness::Offline,
        Err(error) => Freshness::Rejected(error),
    };
    Ok(RevokedLoaded { list, freshness })
}

/// What an HTTP 404 means when a cached copy exists.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Missing {
    /// The server is having trouble; show the cached copy (offline).
    Offline,
    /// The file is gone on purpose (a removed `v1.1` sibling, FR-9.19); report the 404.
    Error,
}

/// Records what a loaded index shows for the security score: the first time this device saw
/// each key (key age, FR-9.74) and each listed version with its permissions (history, FR-9.73
/// H; the index has no publication dates). Rows that exist keep their first-seen time, so
/// calling this on every load is safe.
pub fn note_index(
    store: &Store,
    index: &Index,
    now: UnixMillis,
) -> Result<(), mumcord_store::Error> {
    if let Some(repository) = &index.repository {
        store.note_repo_key_seen(&repository.public_key, now)?;
    }
    for plugin in &index.plugins {
        for key in &plugin.author_keys {
            store.note_repo_key_seen(key, now)?;
        }
        for version in &plugin.versions {
            store.record_plugin_version(&VersionHistoryEntry {
                plugin_id: plugin.id.clone(),
                version: version.version.clone(),
                permissions: serde_json::Value::from(version.permissions.clone()).to_string(),
                origins: serde_json::Value::from(version.origins.clone()).to_string(),
                seen_at: now,
            })?;
        }
    }
    Ok(())
}

/// Shared ETag / verify / offline logic. `accept` checks signature, format and sequence; a
/// cached copy goes through the same check before it is used.
pub(super) async fn fetch_signed<T>(
    client: &Client,
    url: &Url,
    max: usize,
    cache: &Cache,
    missing: Missing,
    accept: impl Fn(&[u8], &str) -> Result<T, RegistryError>,
) -> Result<(T, Freshness), RegistryError> {
    let cached = cache
        .load()
        .and_then(|cached| match accept(&cached.bytes, &cached.signature) {
            Ok(value) => Some((value, cached.etag)),
            Err(error) => {
                tracing::warn!("ignoring the cached plugin list: {error}");
                None
            }
        });
    let etag = cached.as_ref().and_then(|(_, etag)| etag.as_deref());

    let fetched = match client.get(url, max, etag).await {
        Ok(Response::NotModified) => None,
        Ok(Response::Body { bytes, etag }) => match client.get_signature(url).await {
            Ok(signature) => Some(Ok(Cached {
                bytes,
                signature,
                etag,
            })),
            Err(error) => Some(Err(error)),
        },
        Err(error) => Some(Err(error)),
    };

    match (fetched, cached) {
        (None, Some((value, _))) => Ok((value, Freshness::NotModified)),
        (None, None) => Err(RegistryError::Unreachable),
        (Some(Ok(new)), cached) => match accept(&new.bytes, &new.signature) {
            Ok(value) => {
                cache.store(&new)?;
                Ok((value, Freshness::Fresh))
            }
            Err(error) => match cached {
                Some((value, _)) => Ok((value, Freshness::Rejected(error))),
                None => Err(error),
            },
        },
        (Some(Err(RegistryError::Status(404))), Some(_)) if missing == Missing::Error => {
            Err(RegistryError::Status(404))
        }
        (Some(Err(error)), Some((value, _))) => Ok((
            value,
            if matches!(error, RegistryError::Unreachable | RegistryError::Status(_)) {
                Freshness::Offline
            } else {
                Freshness::Rejected(error)
            },
        )),
        (Some(Err(error)), None) => Err(error),
    }
}

#[cfg(test)]
mod base_tests {
    use super::*;

    #[test]
    fn v1_1_sibling_only_for_v1_folders() {
        let sibling = |text: &str| {
            v1_1_sibling(&Url::parse(text).expect("test step"), INDEX_FILE).map(String::from)
        };
        assert_eq!(
            sibling("https://mumcord.github.io/plugins/v1/index.json").as_deref(),
            Some("https://mumcord.github.io/plugins/v1.1/index.json")
        );
        assert_eq!(
            sibling("https://foo.example/v1/index.json?x=1").as_deref(),
            Some("https://foo.example/v1.1/index.json?x=1")
        );
        for other in [
            "https://foo.example/index.json",
            "https://foo.example/xv1/index.json",
            "https://foo.example/v1/list.json",
            "https://foo.example/v1.1/index.json",
        ] {
            assert_eq!(sibling(other), None, "{other}");
        }
    }

    #[test]
    fn base_defaults_to_official() {
        let base = official_base_with(None, None).expect("test step");
        assert_eq!(base.as_str(), OFFICIAL_BASE_URL);
    }

    #[test]
    fn test_url_replaces_official_base() {
        let base = official_base_with(None, Some("http://127.0.0.1:9/v1")).expect("test step");
        assert_eq!(base.as_str(), "http://127.0.0.1:9/v1/");
    }

    #[test]
    fn mirror_wins_over_test_url() {
        let base = official_base_with(
            Some("http://127.0.0.1:1/m/"),
            Some("http://127.0.0.1:9/v1/"),
        )
        .expect("test step");
        assert_eq!(base.as_str(), "http://127.0.0.1:1/m/");
    }

    #[test]
    fn blank_values_are_ignored() {
        let base = official_base_with(Some(" "), Some("")).expect("test step");
        assert_eq!(base.as_str(), OFFICIAL_BASE_URL);
    }

    #[test]
    fn note_index_keeps_the_first_seen_time() {
        use super::super::index::tests::{AUTHOR, plugin};
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(dir.path()).expect("store");
        let bytes = serde_json::to_vec(&serde_json::json!({
            "format": 1, "sequence": 1,
            "repository": {"name": "Repo", "public_key": AUTHOR},
            "plugins": [plugin("dice")]
        }))
        .expect("test step");
        let none = |_: &str| false;
        let index = Index::parse(
            &bytes,
            &IndexKind::Repository {
                is_official_id: &none,
            },
        )
        .expect("index");

        note_index(&store, &index, 10).expect("first load");
        note_index(&store, &index, 20).expect("second load");

        assert_eq!(store.repo_key_first_seen(AUTHOR).expect("key"), Some(10));
        let history = store.plugin_version_history("dice").expect("history");
        let rows: Vec<(&str, &str, i64)> = history
            .iter()
            .map(|h| (h.version.as_str(), h.permissions.as_str(), h.seen_at))
            .collect();
        assert_eq!(
            rows,
            vec![("1.0.0", r#"["chat.send"]"#, 10), ("1.2.0", "[]", 10)]
        );
    }
}
