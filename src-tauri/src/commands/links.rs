//! Link previews, video player cards and inline images for links and images in messages
//! (FR-1.49, FR-1.50, FR-1.55).

use mumcord_engine::{
    AudioFormat as EngineAudioFormat, Engine, MusicKind as EngineMusicKind,
    MusicService as EngineMusicService, QueryOutcome, VideoProvider,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};

use super::servers::{DeepLinkOpened, LinkOutcome, parse_link};
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
    /// Set for YouTube and Twitch links: show a player card.
    pub video: Option<VideoEmbed>,
    /// Set for music service links and music pages: show a music card (issue #262).
    pub music: Option<MusicEmbed>,
    /// The link serves an image: show it inline (`inline_image`) instead of a card.
    pub is_image: bool,
    /// The link serves an audio file: show the audio player (`audio_info`) instead of a card.
    pub is_audio: bool,
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum MusicService {
    Spotify,
    Soundcloud,
    YoutubeMusic,
    AppleMusic,
    Tidal,
    Deezer,
    Bandcamp,
    /// Another page that says it is music.
    Other,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum MusicKind {
    Track,
    Album,
    Playlist,
    Artist,
    Episode,
    Show,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MusicEmbed {
    pub service: MusicService,
    pub kind: MusicKind,
    /// The service's official embed; load it only after the user clicks play. `null`: the
    /// service has no usable embed, the card only links to it.
    pub player_url: Option<String>,
    /// Artist or uploader, when the service names one.
    pub artist: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum AudioFormat {
    Mp3,
    Ogg,
    Aac,
    Mp4,
    Wav,
    Flac,
    Webm,
}

/// A direct audio link before the click (issue #260).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    /// The link as posted, without fragment.
    pub url: String,
    /// Sniffed from the file's first bytes.
    pub format: AudioFormat,
    /// File size in bytes, when the server said (capped at 4 GB; larger files are refused).
    pub size: Option<u32>,
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

/// What the public server query answered for a `mumble://` link (issue #175). `privacyOff` means
/// nothing was asked because the link-preview switch is off; `unreachable` means the server did not
/// answer within the query timeout. The figures are untrusted server data, shown as plain text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MumbleLinkStats {
    PrivacyOff,
    Unreachable,
    Answered {
        ping_ms: u32,
        users: u32,
        max_users: u32,
        version: String,
    },
}

/// Live figures for the card of a `mumble://` link. Only the engine contacts the network, the
/// query never connects, and nothing is asked while the link-preview switch is off (issue #175).
#[tauri::command]
#[specta::specta]
pub async fn mumble_link_stats(
    engine: State<'_, Engine>,
    url: String,
    refresh: bool,
) -> ApiResult<MumbleLinkStats> {
    let link = parse_link(&url)?;
    Ok(
        match engine.server_query(&link.address, link.port, refresh).await {
            QueryOutcome::Answered(query) => MumbleLinkStats::Answered {
                ping_ms: query.ping_ms,
                users: query.users,
                max_users: query.max_users,
                version: query.version.to_string(),
            },
            QueryOutcome::NotAllowed => MumbleLinkStats::PrivacyOff,
            QueryOutcome::Unreachable => MumbleLinkStats::Unreachable,
        },
    )
}

/// A card's Join: the same handling as a `mumble://` link that came from outside (FR-2.13), so the
/// server bar shows the server, a running connection joins the channel and an unknown server is
/// offered for saving. Only a click on the card gets here.
#[tauri::command]
#[specta::specta]
pub async fn mumble_link_join(app: AppHandle, url: String) -> ApiResult<LinkOutcome> {
    let link = parse_link(&url)?;
    let engine = app.state::<Engine>();
    let outcome = crate::deeplink::resolve(&engine, &link).await;
    crate::deeplink::deliver(
        &app,
        DeepLinkOpened {
            link: link.clone(),
            outcome: outcome.clone(),
        },
    );
    Ok(outcome)
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

/// Format and size of a direct audio link (issue #260), read from the start of the file;
/// `null` when previews are switched off in the settings (then nothing is fetched) or the link
/// is not a playable audio file (blocked address, too large, unsupported format).
#[tauri::command]
#[specta::specta]
pub async fn audio_info(engine: State<'_, Engine>, url: String) -> ApiResult<Option<AudioInfo>> {
    Ok(engine.audio_info(&url).await.map(|i| AudioInfo {
        url: i.url,
        format: i.format.into(),
        size: i.size.map(|n| u32::try_from(n).unwrap_or(u32::MAX)),
    }))
}

/// The audio file of a direct audio link as a `data:` URL of its sniffed type, fetched by the
/// engine after the user clicked play (issue #260); `null` when previews are switched off or
/// the file cannot be played.
#[tauri::command]
#[specta::specta]
pub async fn audio_file(engine: State<'_, Engine>, url: String) -> ApiResult<Option<String>> {
    Ok(engine.audio_file(&url).await.map(|f| f.data_url()))
}

/// Saves the audio file of a direct audio link to `path`, which the user picked in a save
/// dialog (issue #260). The engine fetches it again with the same checks as for playing.
#[tauri::command]
#[specta::specta]
pub async fn audio_save(engine: State<'_, Engine>, url: String, path: String) -> ApiResult<()> {
    let file = engine
        .audio_file(&url)
        .await
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidInput, "not a playable audio file"))?;
    std::fs::write(&path, file.bytes).map_err(|err| ApiError::new(ErrorCode::Io, err.to_string()))
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
                },
                player_url: v.player_url,
                channel: v.channel,
            }),
            music: p.music.map(|m| MusicEmbed {
                service: m.service.into(),
                kind: m.kind.into(),
                player_url: m.player_url,
                artist: m.artist,
            }),
            is_image: p.image_link,
            is_audio: p.audio_link,
        }
    }
}

impl From<EngineMusicService> for MusicService {
    fn from(s: EngineMusicService) -> Self {
        match s {
            EngineMusicService::Spotify => Self::Spotify,
            EngineMusicService::Soundcloud => Self::Soundcloud,
            EngineMusicService::YoutubeMusic => Self::YoutubeMusic,
            EngineMusicService::AppleMusic => Self::AppleMusic,
            EngineMusicService::Tidal => Self::Tidal,
            EngineMusicService::Deezer => Self::Deezer,
            EngineMusicService::Bandcamp => Self::Bandcamp,
            EngineMusicService::Other => Self::Other,
        }
    }
}

impl From<EngineMusicKind> for MusicKind {
    fn from(k: EngineMusicKind) -> Self {
        match k {
            EngineMusicKind::Track => Self::Track,
            EngineMusicKind::Album => Self::Album,
            EngineMusicKind::Playlist => Self::Playlist,
            EngineMusicKind::Artist => Self::Artist,
            EngineMusicKind::Episode => Self::Episode,
            EngineMusicKind::Show => Self::Show,
        }
    }
}

impl From<EngineAudioFormat> for AudioFormat {
    fn from(f: EngineAudioFormat) -> Self {
        match f {
            EngineAudioFormat::Mp3 => Self::Mp3,
            EngineAudioFormat::Ogg => Self::Ogg,
            EngineAudioFormat::Aac => Self::Aac,
            EngineAudioFormat::Mp4 => Self::Mp4,
            EngineAudioFormat::Wav => Self::Wav,
            EngineAudioFormat::Flac => Self::Flac,
            EngineAudioFormat::Webm => Self::Webm,
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
