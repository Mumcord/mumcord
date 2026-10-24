//! The plugin data payload that shares a track with other Mumcord users (FR-1.58).
//!
//! Small versioned JSON: `{"v":1,"title":…,"artist":…,"album":…,"art":…,"pos_ms":…,
//! "dur_ms":…,"playing":…,"ts":…}`. `{"v":1}` without a title means "nothing playing".
//! Standard servers drop plugin data over [`PLUGIN_DATA_MAX`] bytes, so the encoder shortens
//! or drops fields until it fits. Received payloads come from other clients and are untrusted:
//! the decoder caps every field and keeps only http(s) cover links.

use mumcord_protocol::PLUGIN_DATA_MAX;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::web::meta::clean;

/// Plugin data id of the payload; other ids are ignored.
pub const DATA_ID: &str = "mumcord.nowplaying";
/// The only payload version this client speaks.
pub const VERSION: u32 = 1;
/// Longest title, artist or album kept (characters).
pub const MAX_TEXT: usize = 120;
/// Longest cover link kept (bytes); longer links are dropped, a cut link is useless.
pub const MAX_ART: usize = 300;

/// One shared track; `position_ms` is the playback position at `ts_ms` (sender's clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedTrack {
    pub title: String,
    /// Empty when the player does not report one.
    pub artist: String,
    pub album: Option<String>,
    /// http(s) cover image link.
    pub art: Option<String>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub playing: bool,
    pub ts_ms: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PayloadError {
    #[error("payload too large")]
    TooLarge,
    #[error("payload is not valid JSON")]
    Malformed,
    #[error("unknown payload version {0}")]
    Version(u32),
}

#[derive(Serialize, Deserialize, Default)]
struct Wire {
    v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    art: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pos_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dur_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    playing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ts: Option<u64>,
}

/// An http(s) link of at most [`MAX_ART`] bytes, else `None`.
pub fn art_link(url: &str) -> Option<String> {
    if url.len() > MAX_ART {
        return None;
    }
    let parsed = Url::parse(url).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.to_string())
}

fn text(value: &str, max: usize) -> Option<String> {
    clean(value, max)
}

/// The payload for `track`, or the "nothing playing" payload for `None`. Always at most
/// [`PLUGIN_DATA_MAX`] bytes: drops the cover link, then the album, then shortens title and
/// artist until it fits.
pub fn encode(track: Option<&SharedTrack>) -> Vec<u8> {
    let Some(track) = track else {
        return to_json(&Wire {
            v: VERSION,
            ..Wire::default()
        });
    };
    let mut wire = Wire {
        v: VERSION,
        title: Some(text(&track.title, MAX_TEXT).unwrap_or_else(|| "?".into())),
        artist: text(&track.artist, MAX_TEXT),
        album: track.album.as_deref().and_then(|a| text(a, MAX_TEXT)),
        art: track.art.as_deref().and_then(art_link),
        pos_ms: Some(track.position_ms),
        dur_ms: track.duration_ms,
        playing: Some(track.playing),
        ts: Some(track.ts_ms),
    };
    let mut max = MAX_TEXT;
    loop {
        let json = to_json(&wire);
        if json.len() <= PLUGIN_DATA_MAX {
            return json;
        }
        if wire.art.take().is_some() || wire.album.take().is_some() {
            continue;
        }
        max /= 2;
        wire.title = wire.title.as_deref().and_then(|t| text(t, max));
        wire.artist = wire.artist.as_deref().and_then(|a| text(a, max));
    }
}

fn to_json(wire: &Wire) -> Vec<u8> {
    // Serializing plain strings and numbers cannot fail.
    serde_json::to_vec(wire).unwrap_or_default()
}

/// Reads a received payload: `Ok(None)` means the sender stopped sharing.
pub fn decode(data: &[u8]) -> Result<Option<SharedTrack>, PayloadError> {
    if data.len() > PLUGIN_DATA_MAX {
        return Err(PayloadError::TooLarge);
    }
    let wire: Wire = serde_json::from_slice(data).map_err(|_| PayloadError::Malformed)?;
    if wire.v != VERSION {
        return Err(PayloadError::Version(wire.v));
    }
    let Some(title) = wire.title.as_deref().and_then(|t| text(t, MAX_TEXT)) else {
        return Ok(None);
    };
    let duration_ms = wire.dur_ms.filter(|d| *d > 0);
    let position_ms = wire.pos_ms.unwrap_or(0);
    Ok(Some(SharedTrack {
        title,
        artist: wire
            .artist
            .as_deref()
            .and_then(|a| text(a, MAX_TEXT))
            .unwrap_or_default(),
        album: wire.album.as_deref().and_then(|a| text(a, MAX_TEXT)),
        art: wire.art.as_deref().and_then(art_link),
        position_ms: duration_ms.map_or(position_ms, |d| position_ms.min(d)),
        duration_ms,
        playing: wire.playing.unwrap_or(false),
        ts_ms: wire.ts.unwrap_or(0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> SharedTrack {
        SharedTrack {
            title: "Song".into(),
            artist: "Band".into(),
            album: Some("Record".into()),
            art: Some("https://i.scdn.co/image/abc".into()),
            position_ms: 42_000,
            duration_ms: Some(180_000),
            playing: true,
            ts_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn round_trips_a_track() {
        let data = encode(Some(&track()));
        let json: serde_json::Value = serde_json::from_slice(&data).expect("json");
        assert_eq!(json["v"], 1);
        assert_eq!(json["title"], "Song");
        assert_eq!(json["pos_ms"], 42_000);
        assert_eq!(json["dur_ms"], 180_000);
        assert_eq!(json["playing"], true);
        assert_eq!(json["ts"], 1_700_000_000_000_u64);
        assert_eq!(decode(&data), Ok(Some(track())));
    }

    #[test]
    fn stop_is_a_bare_version() {
        let data = encode(None);
        assert_eq!(data, br#"{"v":1}"#);
        assert_eq!(decode(&data), Ok(None));
    }

    #[test]
    fn long_fields_are_cut_to_the_server_limit() {
        let long = "🎸".repeat(500);
        let mut t = track();
        t.title = long.clone();
        t.artist = long.clone();
        t.album = Some(long);
        t.art = Some(format!("https://example.org/{}", "a".repeat(250)));
        let data = encode(Some(&t));
        assert!(data.len() <= PLUGIN_DATA_MAX, "{} bytes", data.len());
        let decoded = decode(&data).expect("valid").expect("track");
        assert!(decoded.title.starts_with('🎸'));
        assert!(decoded.artist.starts_with('🎸'));
        assert_eq!(decoded.position_ms, 42_000);
    }

    #[test]
    fn untrusted_input_is_capped_and_filtered() {
        assert_eq!(
            decode(&vec![b' '; PLUGIN_DATA_MAX + 1]),
            Err(PayloadError::TooLarge)
        );
        assert_eq!(decode(b"not json"), Err(PayloadError::Malformed));
        assert_eq!(
            decode(br#"{"v":2,"title":"x"}"#),
            Err(PayloadError::Version(2))
        );
        let decoded = decode(
            br#"{"v":1,"title":"a\u0000b\u202e","art":"javascript:alert(1)","pos_ms":9000,"dur_ms":5000}"#,
        )
        .expect("valid")
        .expect("track");
        assert_eq!(decoded.title, "a b");
        assert_eq!(decoded.art, None);
        assert_eq!(
            decoded.position_ms, 5000,
            "position clamped to the duration"
        );
        assert!(!decoded.playing);
        assert_eq!(decode(br#"{"v":1,"title":"  "}"#), Ok(None));
    }

    #[test]
    fn only_short_http_links_are_kept() {
        assert_eq!(
            art_link("https://i.scdn.co/image/x").as_deref(),
            Some("https://i.scdn.co/image/x")
        );
        assert_eq!(art_link("file:///home/me/cover.png"), None);
        assert_eq!(
            art_link(&format!("https://x.org/{}", "a".repeat(400))),
            None
        );
    }
}
