//! Link previews, video player cards and inline images for links and images in messages
//! (FR-1.49, FR-1.50, FR-1.55).

use mumcord_engine::{Engine, VideoProvider};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::types::{ApiError, ApiResult, ErrorCode};
use crate::watch_player::PlayerServer;

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

/// Opens a link from a message in the system browser after the user clicked it. Only
/// http(s) and mailto links open.
#[tauri::command]
#[specta::specta]
pub fn open_link(url: String) -> ApiResult<()> {
    if !crate::browser::is_openable(&url) {
        return Err(ApiError::new(
            ErrorCode::InvalidInput,
            "only http(s) and mailto links open",
        ));
    }
    crate::browser::open_url(url.trim())
        .map_err(|error| ApiError::new(ErrorCode::Io, error.to_string()))
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

/// Links of the saved GIF favorites, most recently used first.
#[tauri::command]
#[specta::specta]
pub fn gif_favorites(engine: State<'_, Engine>) -> ApiResult<Vec<String>> {
    Ok(engine.gif_favorites()?.into_iter().map(|f| f.url).collect())
}

/// Saves an http(s) image link as a favorite (at most 200; the least recently used go first).
#[tauri::command]
#[specta::specta]
pub fn gif_favorite_add(engine: State<'_, Engine>, url: String) -> ApiResult<()> {
    Ok(engine.gif_favorite_add(&url)?)
}

#[tauri::command]
#[specta::specta]
pub fn gif_favorite_remove(engine: State<'_, Engine>, url: String) -> ApiResult<()> {
    Ok(engine.gif_favorite_remove(&url)?)
}

/// Moves a favorite to the front of the list after it was sent.
#[tauri::command]
#[specta::specta]
pub fn gif_favorite_use(engine: State<'_, Engine>, url: String) -> ApiResult<()> {
    Ok(engine.gif_favorite_use(&url)?)
}

/// URL of the loopback page that plays a YouTube or Twitch card inline (FR-2.41a); starts its
/// server on first use. Every call needs a later `embed_player_close`.
#[tauri::command]
#[specta::specta]
pub async fn embed_player_open(player: State<'_, PlayerServer>) -> ApiResult<String> {
    player
        .open_embed()
        .map_err(|e| ApiError::new(ErrorCode::Io, format!("player server: {e}")))
}

/// The inline player of a chat card closed; the server stops once nothing plays.
#[tauri::command]
#[specta::specta]
pub fn embed_player_close(player: State<'_, PlayerServer>) -> ApiResult<()> {
    player.close_embed();
    Ok(())
}
