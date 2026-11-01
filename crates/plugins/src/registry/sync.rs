//! Fetching the official index and the revocation list with offline fallback (FR-6.55,
//! FR-6.57, FR-6.70).

use std::path::Path;

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

/// The official base URL, or the mirror from Settings → Plugins → Repositories → Advanced
/// (FR-6.57). Mirrored files are still verified with the official keys.
pub fn official_base(mirror: Option<&str>) -> Result<Url, RegistryError> {
    let text = mirror
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(OFFICIAL_BASE_URL);
    let text = if text.ends_with('/') {
        text.to_owned()
    } else {
        format!("{text}/")
    };
    Url::parse(&text).map_err(|_| RegistryError::InvalidUrl)
}

/// Fetches the official index from `base`. `last_seen` is `plugins.last_official_sequence`;
/// the caller stores `index.sequence` afterwards. `cache_dir` holds the offline copy.
pub async fn fetch_official(
    client: &Client,
    cache_dir: &Path,
    base: &Url,
    last_seen: Option<u64>,
) -> Result<Loaded, RegistryError> {
    fetch_official_with(
        client,
        cache_dir,
        base,
        last_seen,
        &RegistryKeys::official(),
    )
    .await
}

pub(super) async fn fetch_official_with(
    client: &Client,
    cache_dir: &Path,
    base: &Url,
    last_seen: Option<u64>,
    keys: &RegistryKeys,
) -> Result<Loaded, RegistryError> {
    let url = base
        .join(INDEX_FILE)
        .map_err(|_| RegistryError::InvalidUrl)?;
    let accept = |bytes: &[u8], signature: &str| {
        keys.verify(bytes, signature)?;
        let index = Index::parse(bytes, &IndexKind::Official)?;
        check_sequence(index.sequence, last_seen)?;
        Ok(index)
    };
    let cache = Cache::new(cache_dir, INDEX_FILE);
    let (index, freshness) =
        fetch_signed(client, &url, MAX_OFFICIAL_INDEX_BYTES, &cache, accept).await?;
    Ok(Loaded { index, freshness })
}

/// Fetches `revoked.json` (daily, even with updates off, FR-6.70). Never fails for lack of a
/// connection: the bundled copy always applies.
pub async fn fetch_revoked(
    client: &Client,
    cache_dir: &Path,
    base: &Url,
) -> Result<RevokedLoaded, RegistryError> {
    fetch_revoked_with(client, cache_dir, base, &RegistryKeys::official()).await
}

pub(super) async fn fetch_revoked_with(
    client: &Client,
    cache_dir: &Path,
    base: &Url,
    keys: &RegistryKeys,
) -> Result<RevokedLoaded, RegistryError> {
    let url = base
        .join(REVOKED_FILE)
        .map_err(|_| RegistryError::InvalidUrl)?;
    let cache = Cache::new(cache_dir, REVOKED_FILE);
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
    let freshness = match fetch_signed(client, &url, MAX_REVOKED_BYTES, &cache, accept).await {
        Ok((fetched, freshness)) => {
            list.merge(fetched);
            freshness
        }
        Err(RegistryError::Unreachable) => Freshness::Offline,
        Err(error) => Freshness::Rejected(error),
    };
    Ok(RevokedLoaded { list, freshness })
}

/// Shared ETag / verify / offline logic. `accept` checks signature, format and sequence; a
/// cached copy goes through the same check before it is used.
pub(super) async fn fetch_signed<T>(
    client: &Client,
    url: &Url,
    max: usize,
    cache: &Cache,
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
