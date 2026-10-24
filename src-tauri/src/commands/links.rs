//! Link previews and video player cards for links in messages (FR-1.49, FR-1.50).

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
    /// Set for YouTube and Twitch links: show a player card.
    pub video: Option<VideoEmbed>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum VideoSite {
    Youtube,
    Twitch,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct VideoEmbed {
    pub site: VideoSite,
    /// Embedded player URL; load it only after the user clicks play. Twitch needs
    /// `&parent=<host of the page>` appended.
    pub player_url: String,
    /// Channel or uploader name.
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
                },
                player_url: v.player_url,
                channel: v.channel,
            }),
        }
    }
}
