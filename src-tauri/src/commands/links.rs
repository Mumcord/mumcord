//! Link previews, video player cards and inline images for links and images in messages
//! (FR-1.49, FR-1.50, FR-1.55).

use mumcord_engine::{Engine, VideoProvider};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::types::ApiResult;

/// Preview card of a link. All text is plain text, never HTML.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
    /// The link as posted, without fragment.
    pub url: String,
    pub site_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Preview image as a `data:` URL (fetched and re-encoded by the engine).
    pub image: Option<String>,
    /// Set for YouTube, Twitch and Spotify links: show a player card.
    pub video: Option<VideoEmbed>,
    /// The link serves an image: show it inline (`inline_image`) instead of a card.
    pub is_image: bool,
}

/// An external image or image link, ready to show inline (FR-1.55).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct InlineImage {
    /// The image link as posted, without fragment.
    pub url: String,
    /// The image as a `data:` URL (fetched by the engine; still images re-encoded, animated
    /// GIF/WebP unchanged).
    pub src: String,
    /// Size of `src` in pixels.
    pub width: u32,
    pub height: u32,
    pub animated: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum VideoSite {
    Youtube,
    Twitch,
    Spotify,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct VideoEmbed {
    pub site: VideoSite,
    /// Embedded player URL; load it only after the user clicks play. Twitch needs
    /// `&parent=<host of the page>` appended.
    pub player_url: String,
    /// Channel or uploader name; for Spotify the kind of item (`track`, `album`, `playlist`,
    /// `artist`, `episode`, `show`).
    pub channel: Option<String>,
}

/// Preview of an http(s) link in a message; `null` when previews are switched off in the
/// settings (then nothing is fetched) or there is nothing to show.
#[tauri::command]
#[specta::specta]
pub async fn link_preview(
    engine: State<'_, Engine>,
    url: String,
) -> ApiResult<Option<LinkPreview>> {
    Ok(engine.link_preview(&url).await.map(Into::into))
}

/// An external image (`<img src>` in a message) or direct image link, ready to show inline;
/// `null` when previews are switched off in the settings (then nothing is fetched) or the
/// image cannot be shown (blocked address, too large, unsupported format).
#[tauri::command]
#[specta::specta]
pub async fn inline_image(
    engine: State<'_, Engine>,
    url: String,
) -> ApiResult<Option<InlineImage>> {
    Ok(engine.inline_image(&url).await.map(|i| InlineImage {
        url: i.url,
        src: i.data_url,
        width: i.width,
        height: i.height,
        animated: i.animated,
    }))
}

/// Hides the preview card of `url` under a message for good (QA.42). Messages that were not
/// stored (history saving off) keep nothing; the UI hides the card for the session.
#[tauri::command]
#[specta::specta]
pub fn preview_dismiss(
    engine: State<'_, Engine>,
    message_id: String,
    url: String,
) -> ApiResult<()> {
    Ok(engine.preview_dismiss(&message_id, &url)?)
}

/// "Clear now": deletes all cached link previews, thumbnails and images. Returns the freed
/// size in bytes (capped at 4 GB; the cache is far smaller).
#[tauri::command]
#[specta::specta]
pub async fn preview_cache_clear(engine: State<'_, Engine>) -> ApiResult<u32> {
    let freed = engine.preview_cache_clear().await?;
    Ok(u32::try_from(freed).unwrap_or(u32::MAX))
}

impl From<mumcord_engine::LinkPreview> for LinkPreview {
    fn from(p: mumcord_engine::LinkPreview) -> Self {
        Self {
            url: p.url,
            site_name: p.site_name,
            title: p.title,
            description: p.description,
            image: p.image,
            video: p.video.map(|v| VideoEmbed {
                site: match v.provider {
                    VideoProvider::Youtube => VideoSite::Youtube,
                    VideoProvider::Twitch => VideoSite::Twitch,
                    VideoProvider::Spotify => VideoSite::Spotify,
                },
                player_url: v.player_url,
                channel: v.channel,
            }),
            is_image: p.image_link,
        }
    }
}
