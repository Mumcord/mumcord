//! Link previews and video player cards for links in chat (FR-1.49, FR-1.50, QA.34).
//!
//! The engine fetches page metadata and preview images itself, so the webview never contacts
//! a remote origin: images reach the UI as re-encoded `data:` URLs. Results are cached in
//! memory and, for successful previews, on disk for a day. Fetching is guarded by
//! [`fetch::Fetcher`] (limits and SSRF protection) and can be reused for inline images.

pub mod embed;
pub mod fetch;
pub mod meta;

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, ImageReader, Limits};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

pub use embed::VideoProvider;
use fetch::{AddressPolicy, FetchError, FetchedImage, Fetcher, Timeouts};

use crate::session::lock;

/// Successful previews are reused for this long, also across restarts.
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Failures are retried after this long (memory only).
const FAILURE_TTL: Duration = Duration::from_secs(10 * 60);
const MEMORY_ENTRIES: usize = 512;
const DISK_ENTRIES: usize = 300;
/// Longest side of a preview image after re-encoding.
const THUMBNAIL_PX: u32 = 480;
const OEMBED_LIMIT: usize = 64 * 1024;

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
    /// Set for YouTube and Twitch links.
    pub video: Option<VideoEmbed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoEmbed {
    pub provider: VideoProvider,
    /// Embedded player, loaded only after a click; Twitch needs `parent` appended.
    pub player_url: String,
    /// Channel or uploader name.
    pub channel: Option<String>,
}

/// Fetches and caches link previews.
pub struct LinkPreviews {
    policy: AddressPolicy,
    timeouts: Timeouts,
    /// Created on first use; keeps the setup error so it is not retried on every link.
    fetcher: OnceLock<Result<Fetcher, FetchError>>,
    cache: PreviewCache,
}

impl LinkPreviews {
    pub fn new(cache_dir: Option<PathBuf>, policy: AddressPolicy, timeouts: Timeouts) -> Self {
        Self {
            policy,
            timeouts,
            fetcher: OnceLock::new(),
            cache: PreviewCache::new(cache_dir),
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
                let m = page_meta(fetcher, url).await?;
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
    #[derive(Deserialize)]
    struct Raw {
        title: Option<String>,
        author_name: Option<String>,
        thumbnail_url: Option<String>,
    }
    let endpoint = Url::parse_with_params(
        "https://www.youtube.com/oembed",
        [("format", "json"), ("url", url.as_str())],
    )
    .map_err(|_| FetchError::InvalidUrl)?;
    let response = fetcher
        .get(endpoint.as_str(), "application/json", OEMBED_LIMIT, false)
        .await?;
    let raw: Raw =
        serde_json::from_slice(&response.body).map_err(|_| FetchError::UnsupportedType)?;
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
    let format = match image.kind {
        fetch::ImageKind::Png => ImageFormat::Png,
        fetch::ImageKind::Jpeg => ImageFormat::Jpeg,
        fetch::ImageKind::Gif => ImageFormat::Gif,
        fetch::ImageKind::Webp => ImageFormat::WebP,
    };
    let mut reader = ImageReader::with_format(Cursor::new(&image.bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
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
    Some(format!(
        "data:{mime};base64,{}",
        crate::people::base64(&out)
    ))
}

struct Entry {
    at: SystemTime,
    preview: Option<LinkPreview>,
}

#[derive(Serialize, Deserialize)]
struct DiskEntry {
    /// Unix seconds.
    stored_at: u64,
    preview: LinkPreview,
}

/// Memory cache of every result plus a small on-disk cache of successful previews.
struct PreviewCache {
    dir: Option<PathBuf>,
    memory: Mutex<HashMap<String, Entry>>,
}

impl PreviewCache {
    fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            memory: Mutex::new(HashMap::new()),
        }
    }

    /// `Some(result)` on a fresh hit (the result may be "no preview").
    fn get(&self, key: &str) -> Option<Option<LinkPreview>> {
        let now = SystemTime::now();
        if let Some(entry) = lock(&self.memory).get(key) {
            let ttl = if entry.preview.is_some() {
                TTL
            } else {
                FAILURE_TTL
            };
            if age(entry.at, now) < ttl {
                return Some(entry.preview.clone());
            }
        }
        let path = self.path(key)?;
        let bytes = std::fs::read(&path).ok()?;
        let entry: DiskEntry = match serde_json::from_slice(&bytes) {
            Ok(entry) => entry,
            Err(_) => {
                remove(&path);
                return None;
            }
        };
        let at = UNIX_EPOCH + Duration::from_secs(entry.stored_at);
        if age(at, now) >= TTL {
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

    fn put(&self, key: &str, preview: Option<LinkPreview>) {
        let now = SystemTime::now();
        {
            let mut memory = lock(&self.memory);
            if memory.len() >= MEMORY_ENTRIES {
                memory.retain(|_, e| age(e.at, now) < FAILURE_TTL);
                if memory.len() >= MEMORY_ENTRIES {
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
        prune(dir, DISK_ENTRIES);
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

fn remove(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        tracing::debug!(error = %e, "removing a cached link preview failed");
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
