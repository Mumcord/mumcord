//! Link previews and video player cards for links in chat (FR-1.49, FR-1.50, QA.34).
//!
//! The engine fetches page metadata and preview images itself, so the webview never contacts
//! a remote origin: images reach the UI as re-encoded `data:` URLs. Results are cached in
//! memory and, for successful previews, on disk until they pass the configurable maximum age
//! (default 7 days, QA.42). Preview images are stored inside the cached previews. Fetching is guarded by
//! [`fetch::Fetcher`] (limits and SSRF protection), which also loads inline images in messages
//! ([`inline`], FR-1.55); those are cached the same way and cleaned up with the previews.

pub mod embed;
pub mod fetch;
pub mod inline;
pub mod meta;

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, ImageReader, Limits};
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

pub use embed::VideoProvider;
use fetch::{AddressPolicy, FetchError, FetchedImage, Fetcher, Timeouts};
pub use inline::InlineImage;

use crate::session::lock;

/// Default for how long successful previews are reused, also across restarts (QA.42).
pub const DEFAULT_MAX_AGE_DAYS: u32 = 7;
const SECS_PER_DAY: u64 = 24 * 60 * 60;
/// Stored in [`PreviewCache`] when previews never expire.
const NEVER: u64 = u64::MAX;
/// Failures are retried after this long (memory only).
const FAILURE_TTL: Duration = Duration::from_secs(10 * 60);
/// Cache sizes for previews; inline images are far larger, so fewer of them are kept.
const PREVIEW_LIMITS: CacheLimits = CacheLimits {
    memory: 512,
    disk: 300,
};
const IMAGE_LIMITS: CacheLimits = CacheLimits {
    memory: 48,
    disk: 150,
};
/// Longest side of a preview image after re-encoding.
const THUMBNAIL_PX: u32 = 480;
const OEMBED_LIMIT: usize = 64 * 1024;
/// A link may be a page (preview card) or an image (shown inline).
const PAGE_OR_IMAGE_ACCEPT: &str = "text/html,application/xhtml+xml;q=0.9,image/png,image/jpeg,image/gif,image/webp;q=0.8,*/*;q=0.1";

/// What a link preview card shows. All text is plain (never HTML).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkPreview {
    /// The link as posted (without fragment).
    pub url: String,
    pub site_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Preview image as a `data:` URL.
    pub image: Option<String>,
    /// Set for YouTube, Twitch and Spotify links.
    pub video: Option<VideoEmbed>,
    /// The link itself serves an image: show it inline ([`LinkPreviews::inline_image`])
    /// instead of a card.
    #[serde(default)]
    pub image_link: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoEmbed {
    pub provider: VideoProvider,
    /// Embedded player, loaded only after a click; Twitch needs `parent` appended.
    pub player_url: String,
    /// Channel or uploader name; for Spotify the kind of item (`track`, `album`, …).
    pub channel: Option<String>,
}

/// Fetches and caches link previews.
pub struct LinkPreviews {
    policy: AddressPolicy,
    timeouts: Timeouts,
    /// Created on first use; keeps the setup error so it is not retried on every link.
    fetcher: OnceLock<Result<Fetcher, FetchError>>,
    cache: Cache<LinkPreview>,
    /// Inline images, in the `images` subdirectory of the preview cache.
    images: Cache<InlineImage>,
}

impl LinkPreviews {
    pub fn new(cache_dir: Option<PathBuf>, policy: AddressPolicy, timeouts: Timeouts) -> Self {
        Self {
            policy,
            timeouts,
            fetcher: OnceLock::new(),
            images: Cache::new(cache_dir.as_ref().map(|d| d.join("images")), IMAGE_LIMITS),
            cache: Cache::new(cache_dir, PREVIEW_LIMITS),
        }
    }

    /// The address policy of the app: public addresses only. E2E builds can restrict it to a
    /// local test page with `MUMCORD_TEST_PREVIEW_ADDRESSES` (comma-separated IPs).
    pub fn app_policy() -> AddressPolicy {
        #[cfg(feature = "test-backend")]
        if let Ok(list) = std::env::var("MUMCORD_TEST_PREVIEW_ADDRESSES") {
            let ips = list
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            return AddressPolicy::Only(std::sync::Arc::new(ips));
        }
        AddressPolicy::PublicOnly
    }

    /// Sets how long cached previews and inline images are reused (`None`: never expire).
    /// Older ones are removed by [`Self::prune_expired`].
    pub fn set_max_age_days(&self, days: Option<u32>) {
        self.cache.set_max_age_days(days);
        self.images.set_max_age_days(days);
    }

    /// Deletes cached previews and inline images older than the maximum age (memory and
    /// disk); returns the bytes freed on disk. Blocking file access: call it off the UI thread.
    pub fn prune_expired(&self) -> u64 {
        self.cache.prune_expired() + self.images.prune_expired()
    }

    /// Deletes every cached preview and inline image (memory and disk); returns the bytes
    /// freed on disk. Blocking file access: call it off the UI thread.
    pub fn clear_cache(&self) -> u64 {
        self.cache.clear() + self.images.clear()
    }

    /// The guarded client, shared with other fetchers of web content (inline images).
    pub fn fetcher(&self) -> Result<&Fetcher, FetchError> {
        self.fetcher
            .get_or_init(|| Fetcher::new(self.policy.clone(), self.timeouts))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The preview for `url`, or `None` when there is nothing to show (not http(s), blocked
    /// address, fetch failed, no title).
    pub async fn preview(&self, url: &str) -> Option<LinkPreview> {
        let mut url = Url::parse(url.trim()).ok()?;
        url.set_fragment(None);
        let key = url.to_string();
        if let Some(hit) = self.cache.get(&key) {
            return hit;
        }
        let result = self.build(&url).await;
        if let Err(e) = &result {
            // Never the URL: links are chat content.
            tracing::debug!(error = %e, "no link preview");
        }
        let preview = result.ok();
        self.cache.put(&key, preview.clone());
        preview
    }

    /// The image at `url`, ready to show inline in a message (FR-1.55), or `None` when it
    /// cannot be shown (not http(s), blocked address, too large, not a supported image).
    pub async fn inline_image(&self, url: &str) -> Option<InlineImage> {
        let mut url = Url::parse(url.trim()).ok()?;
        url.set_fragment(None);
        let key = url.to_string();
        if let Some(hit) = self.images.get(&key) {
            return hit;
        }
        let result = match self.fetcher() {
            Ok(fetcher) => inline::load(fetcher, &url).await,
            Err(e) => Err(e),
        };
        if let Err(e) = &result {
            // Never the URL: links are chat content.
            tracing::debug!(error = %e, "no inline image");
        }
        let image = result.ok();
        self.images.put(&key, image.clone());
        image
    }

    async fn build(&self, url: &Url) -> Result<LinkPreview, FetchError> {
        let fetcher = self.fetcher()?;
        fetch::check_url(url, &self.policy)?;
        let video = embed::recognise(url);
        let mut preview = LinkPreview {
            url: url.to_string(),
            site_name: None,
            title: None,
            description: None,
            image: None,
            video: None,
            image_link: false,
        };
        let image_url = match &video {
            Some(link) if link.provider == VideoProvider::Youtube => {
                preview.site_name = Some("YouTube".into());
                // The card works without a title; the player is what matters.
                match youtube_oembed(fetcher, url).await {
                    Ok(o) => {
                        preview.title = o.title;
                        preview.video = Some(embed_of(link, o.author));
                        o.thumbnail
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "no YouTube details");
                        preview.video = Some(embed_of(link, None));
                        None
                    }
                }
            }
            Some(link) if link.provider == VideoProvider::Spotify => {
                preview.site_name = Some("Spotify".into());
                preview.video = Some(embed_of(link, link.channel.clone()));
                // oEmbed has title and cover; the page's OpenGraph description names the
                // artist or owner. Either may fail, the card still works.
                let oembed = spotify_oembed(fetcher, url).await;
                if let Err(e) = &oembed {
                    tracing::debug!(error = %e, "no Spotify oEmbed details");
                }
                let oembed = oembed.ok();
                let page = match page_meta(fetcher, url).await {
                    Ok(m) => Some(m),
                    Err(e) => {
                        tracing::debug!(error = %e, "no Spotify page details");
                        None
                    }
                };
                preview.description = page.as_ref().and_then(|m| m.description.clone());
                preview.title = oembed
                    .as_ref()
                    .and_then(|o| o.title.clone())
                    .or_else(|| page.as_ref().and_then(|m| m.title.clone()));
                oembed
                    .and_then(|o| o.thumbnail)
                    .or_else(|| page.and_then(|m| m.image))
            }
            Some(link) => {
                preview.site_name = Some("Twitch".into());
                preview.video = Some(embed_of(link, link.channel.clone()));
                match page_meta(fetcher, url).await {
                    Ok(m) => {
                        preview.title = m.title;
                        preview.description = m.description;
                        m.image
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "no Twitch details");
                        None
                    }
                }
            }
            None => {
                let page = fetcher
                    .get(url.as_str(), PAGE_OR_IMAGE_ACCEPT, fetch::PAGE_LIMIT, true)
                    .await?;
                if page.media_type.starts_with("image/") {
                    preview.image_link = true;
                    return Ok(preview);
                }
                if !fetch::is_html(&page.media_type) {
                    return Err(FetchError::UnsupportedType);
                }
                let m = meta::parse(&page.body, &page.url);
                if m.title.is_none() {
                    return Err(FetchError::UnsupportedType);
                }
                preview.site_name = m.site_name;
                preview.title = m.title;
                preview.description = m.description;
                m.image
            }
        };
        if let Some(image_url) = image_url {
            match fetcher.image(image_url.as_str()).await {
                Ok(image) => preview.image = thumbnail_data_url(&image, THUMBNAIL_PX),
                Err(e) => tracing::debug!(error = %e, "no preview image"),
            }
        }
        Ok(preview)
    }
}

fn embed_of(link: &embed::VideoLink, channel: Option<String>) -> VideoEmbed {
    VideoEmbed {
        provider: link.provider,
        player_url: link.player_url.clone(),
        channel,
    }
}

async fn page_meta(fetcher: &Fetcher, url: &Url) -> Result<meta::PageMeta, FetchError> {
    let page = fetcher.page(url.as_str()).await?;
    Ok(meta::parse(&page.body, &page.url))
}

struct OEmbed {
    title: Option<String>,
    author: Option<String>,
    thumbnail: Option<Url>,
}

/// Title, uploader and thumbnail from YouTube's public oEmbed endpoint.
async fn youtube_oembed(fetcher: &Fetcher, url: &Url) -> Result<OEmbed, FetchError> {
    oembed(fetcher, "https://www.youtube.com/oembed", url).await
}

/// Title and cover from Spotify's public oEmbed endpoint (no key needed).
async fn spotify_oembed(fetcher: &Fetcher, url: &Url) -> Result<OEmbed, FetchError> {
    oembed(fetcher, "https://open.spotify.com/oembed", url).await
}

async fn oembed(fetcher: &Fetcher, endpoint: &str, url: &Url) -> Result<OEmbed, FetchError> {
    let endpoint = Url::parse_with_params(endpoint, [("format", "json"), ("url", url.as_str())])
        .map_err(|_| FetchError::InvalidUrl)?;
    let response = fetcher
        .get(endpoint.as_str(), "application/json", OEMBED_LIMIT, false)
        .await?;
    parse_oembed(&response.body)
}

fn parse_oembed(body: &[u8]) -> Result<OEmbed, FetchError> {
    #[derive(Deserialize)]
    struct Raw {
        title: Option<String>,
        author_name: Option<String>,
        thumbnail_url: Option<String>,
    }
    let raw: Raw = serde_json::from_slice(body).map_err(|_| FetchError::UnsupportedType)?;
    Ok(OEmbed {
        title: raw.title.and_then(|t| meta::clean(&t, 200)),
        author: raw.author_name.and_then(|a| meta::clean(&a, 80)),
        thumbnail: raw
            .thumbnail_url
            .and_then(|t| Url::parse(&t).ok())
            .filter(|t| t.scheme() == "https"),
    })
}

/// Decodes the image (with size limits), scales it to at most `max` pixels per side and
/// re-encodes it, so the webview only ever decodes a small image Mumcord produced itself.
/// Animated images keep their first frame.
pub fn thumbnail_data_url(image: &FetchedImage, max: u32) -> Option<String> {
    reencode(image, max).map(|r| r.data_url)
}

/// An image Mumcord encoded itself.
pub(crate) struct Reencoded {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

/// Largest side an image may have to be decoded at all.
pub(crate) const DECODE_MAX_SIDE: u32 = 8192;

pub(crate) fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(DECODE_MAX_SIDE);
    limits.max_image_height = Some(DECODE_MAX_SIDE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits
}

pub(crate) fn image_format(kind: fetch::ImageKind) -> ImageFormat {
    match kind {
        fetch::ImageKind::Png => ImageFormat::Png,
        fetch::ImageKind::Jpeg => ImageFormat::Jpeg,
        fetch::ImageKind::Gif => ImageFormat::Gif,
        fetch::ImageKind::Webp => ImageFormat::WebP,
    }
}

/// See [`thumbnail_data_url`]; also returns the size of the encoded image.
pub(crate) fn reencode(image: &FetchedImage, max: u32) -> Option<Reencoded> {
    let mut reader = ImageReader::with_format(Cursor::new(&image.bytes), image_format(image.kind));
    reader.limits(decode_limits());
    let decoded = reader.decode().ok()?;
    let scaled = if decoded.width() > max || decoded.height() > max {
        decoded.thumbnail(max, max)
    } else {
        decoded
    };
    let mut out = Vec::new();
    let mime = if scaled.color().has_alpha() {
        scaled
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .ok()?;
        "image/png"
    } else {
        scaled
            .to_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, 82))
            .ok()?;
        "image/jpeg"
    };
    Some(Reencoded {
        data_url: format!("data:{mime};base64,{}", crate::people::base64(&out)),
        width: scaled.width(),
        height: scaled.height(),
    })
}

#[derive(Clone, Copy)]
struct CacheLimits {
    memory: usize,
    disk: usize,
}

struct Entry<T> {
    at: SystemTime,
    preview: Option<T>,
}

#[derive(Serialize, Deserialize)]
struct DiskEntry<T> {
    /// Unix seconds.
    stored_at: u64,
    preview: T,
}

/// Memory cache of every result plus a small on-disk cache of successful ones.
struct Cache<T> {
    dir: Option<PathBuf>,
    limits: CacheLimits,
    memory: Mutex<HashMap<String, Entry<T>>>,
    /// Maximum age of a successful result in seconds, [`NEVER`] for no limit.
    max_age_secs: AtomicU64,
}

impl<T: Clone + Serialize + DeserializeOwned> Cache<T> {
    fn new(dir: Option<PathBuf>, limits: CacheLimits) -> Self {
        Self {
            dir,
            limits,
            memory: Mutex::new(HashMap::new()),
            max_age_secs: AtomicU64::new(u64::from(DEFAULT_MAX_AGE_DAYS) * SECS_PER_DAY),
        }
    }

    fn set_max_age_days(&self, days: Option<u32>) {
        let secs = days.map_or(NEVER, |d| u64::from(d) * SECS_PER_DAY);
        self.max_age_secs.store(secs, Ordering::Relaxed);
    }

    fn max_age(&self) -> Duration {
        Duration::from_secs(self.max_age_secs.load(Ordering::Relaxed))
    }

    /// Removes cache files and memory entries past the maximum age; returns the freed bytes.
    fn prune_expired(&self) -> u64 {
        let now = SystemTime::now();
        let max_age = self.max_age();
        lock(&self.memory).retain(|_, e| {
            age(e.at, now)
                < if e.preview.is_some() {
                    max_age
                } else {
                    FAILURE_TTL
                }
        });
        self.remove_files(|modified| age(modified, now) >= max_age)
    }

    /// Removes every cache file and memory entry; returns the freed bytes.
    fn clear(&self) -> u64 {
        lock(&self.memory).clear();
        self.remove_files(|_| true)
    }

    /// Removes the cache files whose write time satisfies `doomed`; returns their total size.
    fn remove_files(&self, doomed: impl Fn(SystemTime) -> bool) -> u64 {
        let Some(Ok(read)) = self.dir.as_ref().map(std::fs::read_dir) else {
            return 0;
        };
        let mut freed = 0;
        for entry in read.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if doomed(meta.modified().unwrap_or(UNIX_EPOCH)) && remove(&path) {
                freed += meta.len();
            }
        }
        freed
    }

    /// `Some(result)` on a fresh hit (the result may be "no preview").
    fn get(&self, key: &str) -> Option<Option<T>> {
        let now = SystemTime::now();
        if let Some(entry) = lock(&self.memory).get(key) {
            let ttl = if entry.preview.is_some() {
                self.max_age()
            } else {
                FAILURE_TTL
            };
            if age(entry.at, now) < ttl {
                return Some(entry.preview.clone());
            }
        }
        let path = self.path(key)?;
        let bytes = std::fs::read(&path).ok()?;
        let entry: DiskEntry<T> = match serde_json::from_slice(&bytes) {
            Ok(entry) => entry,
            Err(_) => {
                remove(&path);
                return None;
            }
        };
        let at = UNIX_EPOCH + Duration::from_secs(entry.stored_at);
        if age(at, now) >= self.max_age() {
            remove(&path);
            return None;
        }
        lock(&self.memory).insert(
            key.to_owned(),
            Entry {
                at,
                preview: Some(entry.preview.clone()),
            },
        );
        Some(Some(entry.preview))
    }

    fn put(&self, key: &str, preview: Option<T>) {
        let now = SystemTime::now();
        {
            let mut memory = lock(&self.memory);
            if memory.len() >= self.limits.memory {
                memory.retain(|_, e| age(e.at, now) < FAILURE_TTL);
                if memory.len() >= self.limits.memory {
                    memory.clear();
                }
            }
            memory.insert(
                key.to_owned(),
                Entry {
                    at: now,
                    preview: preview.clone(),
                },
            );
        }
        let (Some(preview), Some(path), Some(dir)) = (preview, self.path(key), &self.dir) else {
            return;
        };
        let entry = DiskEntry {
            stored_at: now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            preview,
        };
        let written = std::fs::create_dir_all(dir).and_then(|()| {
            let json = serde_json::to_vec(&entry).map_err(std::io::Error::other)?;
            std::fs::write(&path, json)
        });
        if let Err(e) = written {
            tracing::warn!(error = %e, "writing the link preview cache failed");
            return;
        }
        prune(dir, self.limits.disk);
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        let hash = Sha1::digest(key.as_bytes());
        let name: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        self.dir.as_ref().map(|d| d.join(format!("{name}.json")))
    }
}

fn age(at: SystemTime, now: SystemTime) -> Duration {
    now.duration_since(at).unwrap_or_default()
}

/// Whether the file is gone.
fn remove(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!(error = %e, "removing a cached link preview failed");
            false
        }
    }
}

/// Keeps the newest `keep` cache files.
fn prune(dir: &Path, keep: usize) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = read
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            (modified, e.path())
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, path) in files.into_iter().skip(keep) {
        remove(&path);
    }
}

#[cfg(test)]
mod tests;
