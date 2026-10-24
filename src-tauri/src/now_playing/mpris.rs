//! MPRIS (Media Player Remote Interfacing Specification) over the session D-Bus: every player
//! that registers `org.mpris.MediaPlayer2.*` reports status, metadata and position. A playing
//! player wins over a paused one, and Spotify over other players (browsers publish too).

use std::collections::HashMap;
use std::time::Duration;

use mumcord_engine::LocalTrack;
use zbus::Connection;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::InterfaceName;
use zbus::zvariant::{OwnedValue, Value};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
/// A player that does not answer within this time is skipped for this round.
const PLAYER_TIMEOUT: Duration = Duration::from_millis(700);

#[derive(Default)]
pub struct Reader {
    connection: Option<Connection>,
    /// The session bus could not be reached; reported once.
    reported: bool,
    /// Player names seen last time, to log only when the set of players changes.
    players: Vec<String>,
}

impl Reader {
    /// Drops the bus connection while the feature is off.
    pub fn release(&mut self) {
        self.connection = None;
    }

    pub async fn read(&mut self) -> Option<LocalTrack> {
        let connection = match &self.connection {
            Some(c) => c.clone(),
            None => match Connection::session().await {
                Ok(c) => {
                    self.connection = Some(c.clone());
                    c
                }
                Err(e) => {
                    if !self.reported {
                        tracing::warn!(error = %e, "session D-Bus unavailable; now playing off");
                        self.reported = true;
                    }
                    return None;
                }
            },
        };
        match current(&connection).await {
            Ok((names, track)) => {
                if names != self.players {
                    // Bus names identify the player application, not what plays.
                    tracing::info!(players = ?names, "now playing: media players found");
                    self.players = names;
                }
                track
            }
            Err(e) => {
                tracing::debug!(error = %e, "listing media players failed");
                self.connection = None;
                None
            }
        }
    }
}

/// The MPRIS players on the bus (sorted) and the track to share.
async fn current(connection: &Connection) -> zbus::Result<(Vec<String>, Option<LocalTrack>)> {
    let names = DBusProxy::new(connection).await?.list_names().await?;
    let mut players = Vec::new();
    let mut found: Vec<String> = names
        .iter()
        .map(|n| n.as_str())
        .filter(|n| n.starts_with(PREFIX))
        .map(|n| n.trim_start_matches(PREFIX).to_owned())
        .collect();
    found.sort();
    for name in names.iter().map(|n| n.as_str()) {
        if !name.starts_with(PREFIX) {
            continue;
        }
        match tokio::time::timeout(PLAYER_TIMEOUT, properties(connection, name)).await {
            Ok(Ok(props)) => {
                if let Some(player) = parse_player(&props) {
                    players.push((name.to_owned(), player));
                }
            }
            Ok(Err(e)) => tracing::debug!(error = %e, "reading a media player failed"),
            Err(_) => tracing::debug!("a media player did not answer"),
        }
    }
    Ok((found, pick(players)))
}

async fn properties(
    connection: &Connection,
    name: &str,
) -> zbus::Result<HashMap<String, OwnedValue>> {
    let proxy = PropertiesProxy::builder(connection)
        .destination(name)?
        .path(PATH)?
        .build()
        .await?;
    Ok(proxy
        .get_all(InterfaceName::from_static_str_unchecked(PLAYER))
        .await?)
}

/// Variant values inside `a{sv}` arrive boxed once more.
fn unboxed<'a>(value: &'a Value<'a>) -> &'a Value<'a> {
    match value {
        Value::Value(inner) => unboxed(inner),
        other => other,
    }
}

fn text(value: &Value<'_>) -> Option<String> {
    match unboxed(value) {
        Value::Str(s) => Some(s.as_str().trim().to_owned()).filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// A list of strings (`xesam:artist` is `as`), joined; some players send a single string.
fn names(value: &Value<'_>) -> Option<String> {
    match unboxed(value) {
        Value::Array(items) => {
            let joined = items.iter().filter_map(text).collect::<Vec<_>>().join(", ");
            Some(joined).filter(|s| !s.is_empty())
        }
        other => text(other),
    }
}

/// Microseconds (players disagree on the integer type) as milliseconds.
fn micros_as_ms(value: &Value<'_>) -> Option<u64> {
    let micros: i128 = match unboxed(value) {
        Value::I64(v) => (*v).into(),
        Value::U64(v) => (*v).into(),
        Value::I32(v) => (*v).into(),
        Value::U32(v) => (*v).into(),
        // Saturating cast; whole microseconds are plenty.
        Value::F64(v) if v.is_finite() => *v as i128,
        _ => return None,
    };
    u64::try_from(micros.max(0) / 1000).ok()
}

/// A player's state; `None` when it is stopped or has no title.
pub(crate) fn parse_player(props: &HashMap<String, OwnedValue>) -> Option<LocalTrack> {
    let playing = match props.get("PlaybackStatus").and_then(|v| text(v)).as_deref() {
        Some("Playing") => true,
        Some("Paused") => false,
        _ => return None,
    };
    let Value::Dict(metadata) = unboxed(props.get("Metadata")?) else {
        return None;
    };
    let mut title = None;
    let mut artist = None;
    let mut album = None;
    let mut art_url = None;
    let mut duration_ms = None;
    for (key, value) in metadata.iter() {
        let Value::Str(key) = unboxed(key) else {
            continue;
        };
        match key.as_str() {
            "xesam:title" => title = text(value),
            "xesam:artist" => artist = names(value),
            "xesam:album" => album = text(value),
            "mpris:artUrl" => art_url = text(value),
            "mpris:length" => duration_ms = micros_as_ms(value).filter(|d| *d > 0),
            _ => {}
        }
    }
    let position_ms = props
        .get("Position")
        .and_then(|v| micros_as_ms(v))
        .unwrap_or(0);
    Some(LocalTrack {
        title: title?,
        artist: artist.unwrap_or_default(),
        album,
        art_url,
        position_ms: duration_ms.map_or(position_ms, |d| position_ms.min(d)),
        duration_ms,
        playing,
    })
}

/// The player to show: playing before paused, then Spotify before others.
pub(crate) fn pick(players: Vec<(String, LocalTrack)>) -> Option<LocalTrack> {
    players
        .into_iter()
        .min_by_key(|(name, track)| {
            (
                !track.playing,
                !name.starts_with(&format!("{PREFIX}spotify")),
            )
        })
        .map(|(_, track)| track)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(value: Value<'_>) -> OwnedValue {
        OwnedValue::try_from(value).expect("owned value")
    }

    fn player(
        status: &str,
        metadata: HashMap<&str, Value<'static>>,
        position: i64,
    ) -> HashMap<String, OwnedValue> {
        HashMap::from([
            (
                "PlaybackStatus".to_owned(),
                owned(Value::from(status.to_owned())),
            ),
            ("Metadata".to_owned(), owned(Value::from(metadata))),
            ("Position".to_owned(), owned(Value::from(position))),
        ])
    }

    fn spotify_metadata() -> HashMap<&'static str, Value<'static>> {
        HashMap::from([
            ("xesam:title", Value::from("Song")),
            ("xesam:artist", Value::from(vec!["Band", "Guest"])),
            ("xesam:album", Value::from("Record")),
            ("mpris:artUrl", Value::from("https://i.scdn.co/image/abc")),
            ("mpris:length", Value::from(200_000_000_u64)),
            ("mpris:trackid", Value::from("spotify:track:1")),
        ])
    }

    #[test]
    fn reads_title_artists_cover_and_times() {
        let track =
            parse_player(&player("Playing", spotify_metadata(), 42_500_000)).expect("track");
        assert_eq!(
            track,
            LocalTrack {
                title: "Song".into(),
                artist: "Band, Guest".into(),
                album: Some("Record".into()),
                art_url: Some("https://i.scdn.co/image/abc".into()),
                position_ms: 42_500,
                duration_ms: Some(200_000),
                playing: true,
            }
        );
    }

    /// The shape Spotify's Linux client reports: track id as a plain string, extra keys with
    /// other types, artists as a one-element list and (in some versions) position 0.
    #[test]
    fn reads_spotify_metadata_with_extra_keys_and_zero_position() {
        let metadata = HashMap::from([
            (
                "mpris:trackid",
                Value::from("/com/spotify/track/19nAFg6Mb6jXN3yxPC2sSk"),
            ),
            ("mpris:length", Value::from(225_080_000_u64)),
            (
                "mpris:artUrl",
                Value::from("https://i.scdn.co/image/ab67616d0000b273"),
            ),
            ("xesam:album", Value::from("Record")),
            ("xesam:albumArtist", Value::from(vec!["Band"])),
            ("xesam:artist", Value::from(vec!["Band"])),
            ("xesam:autoRating", Value::from(0.51_f64)),
            ("xesam:discNumber", Value::from(1_i32)),
            ("xesam:title", Value::from("Song")),
            ("xesam:trackNumber", Value::from(3_i32)),
            (
                "xesam:url",
                Value::from("https://open.spotify.com/track/19nAFg6Mb6jXN3yxPC2sSk"),
            ),
        ]);
        let track = parse_player(&player("Playing", metadata, 0)).expect("track");
        assert_eq!(track.title, "Song");
        assert_eq!(track.artist, "Band");
        assert_eq!(track.duration_ms, Some(225_080));
        assert_eq!(track.position_ms, 0);
        assert_eq!(
            track.art_url.as_deref(),
            Some("https://i.scdn.co/image/ab67616d0000b273")
        );
        assert!(track.playing);
    }

    #[test]
    fn stopped_untitled_or_odd_players_are_skipped_or_tolerated() {
        assert!(parse_player(&player("Stopped", spotify_metadata(), 0)).is_none());
        let untitled = HashMap::from([("xesam:artist", Value::from(vec!["Band"]))]);
        assert!(parse_player(&player("Playing", untitled, 0)).is_none());
        // Single-string artist, signed length, no position, paused.
        let odd = HashMap::from([
            ("xesam:title", Value::from("Talk")),
            ("xesam:artist", Value::from("Host")),
            ("mpris:length", Value::from(-5_i64)),
        ]);
        let mut props = player("Paused", odd, -1);
        props.remove("Position");
        let track = parse_player(&props).expect("track");
        assert_eq!(track.artist, "Host");
        assert_eq!(track.duration_ms, None);
        assert_eq!(track.position_ms, 0);
        assert!(!track.playing);
    }

    #[test]
    fn prefers_playing_then_spotify() {
        let track = |title: &str, playing: bool| LocalTrack {
            title: title.into(),
            artist: String::new(),
            album: None,
            art_url: None,
            position_ms: 0,
            duration_ms: None,
            playing,
        };
        let pick_title = |players: Vec<(&str, LocalTrack)>| {
            pick(
                players
                    .into_iter()
                    .map(|(n, t)| (format!("{PREFIX}{n}"), t))
                    .collect(),
            )
            .map(|t| t.title)
        };
        assert_eq!(
            pick_title(vec![
                ("firefox.instance1", track("video", true)),
                ("spotify", track("song", false))
            ]),
            Some("video".into())
        );
        assert_eq!(
            pick_title(vec![
                ("vlc", track("film", true)),
                ("spotify", track("song", true))
            ]),
            Some("song".into())
        );
        assert_eq!(pick_title(vec![]), None);
    }

    /// Manual check against the players on this machine's session bus:
    /// `cargo test -p mumcord --lib live_session_bus -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "needs a session D-Bus with an MPRIS player"]
    async fn live_session_bus() {
        let track = Reader::default().read().await;
        println!("{track:#?}");
    }
}
