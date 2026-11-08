//! macOS: the track the system shows in Control Center, from the private MediaRemote framework
//! (issue #658). Since macOS 15.4 only apps with a private entitlement that Apple grants to its
//! own apps get an answer from that framework (a Developer ID signature does not help), but Apple's own `osascript` still does, so a short JavaScript for Automation run
//! by it reads `MRNowPlayingRequest` and prints the track as JSON. This covers every player
//! (Music, Spotify, browsers, …) and needs no permission prompt. It relies on private behaviour:
//! when a macOS version lacks the request class, the feature reports itself unavailable instead
//! of failing quietly. MediaRemote hands out covers as image data, not links, so macOS shares
//! no cover (like Windows).

#[cfg(target_os = "macos")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mumcord_engine::LocalTrack;
use serde::Deserialize;

#[cfg(target_os = "macos")]
/// Reads the now-playing item through MediaRemote and prints it as JSON: `null` when nothing
/// plays, `{"error": …}` when this macOS has no usable MediaRemote.
const SCRIPT: &str = r#"
ObjC.import('Foundation');
function run() {
  const bundle = $.NSBundle.bundleWithPath('/System/Library/PrivateFrameworks/MediaRemote.framework/');
  if (bundle.isNil() || !bundle.load) return JSON.stringify({ error: 'framework' });
  const request = $.NSClassFromString('MRNowPlayingRequest');
  if (request.isNil()) return JSON.stringify({ error: 'request' });
  const item = request.localNowPlayingItem;
  if (item.isNil()) return 'null';
  const info = item.nowPlayingInfo;
  if (info.isNil()) return 'null';
  const value = (key) => {
    const v = info.objectForKey('kMRMediaRemoteNowPlayingInfo' + key);
    return v.isNil() ? null : ObjC.unwrap(v);
  };
  const stamp = value('Timestamp');
  return JSON.stringify({
    title: value('Title'),
    artist: value('Artist'),
    album: value('Album'),
    duration: value('Duration'),
    elapsed: value('ElapsedTime'),
    rate: value('PlaybackRate'),
    timestamp: stamp instanceof Date ? stamp.getTime() : null,
    playing: request.localIsPlaying === true,
  });
}
"#;

/// `osascript` normally answers within a fraction of a second; a hung run is ended.
#[cfg(target_os = "macos")]
const TIMEOUT: Duration = Duration::from_secs(5);

/// What the script prints for a track; times in seconds, `timestamp` in ms since 1970.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Info {
    error: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<f64>,
    elapsed: Option<f64>,
    rate: Option<f64>,
    timestamp: Option<f64>,
    playing: bool,
}

/// What one read gave.
#[derive(Debug, PartialEq)]
enum Read {
    Track(Option<LocalTrack>),
    /// This macOS has no usable MediaRemote; asking again will not help.
    Unavailable,
}

/// Turns the script's output into a track, with the position moved on to `now_ms`.
fn parse(output: &str, now_ms: f64) -> Result<Read, serde_json::Error> {
    let Some(info) = serde_json::from_str::<Option<Info>>(output.trim())? else {
        return Ok(Read::Track(None));
    };
    if info.error.is_some() {
        return Ok(Read::Unavailable);
    }
    let text = |v: Option<String>| v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let Some(title) = text(info.title) else {
        return Ok(Read::Track(None));
    };
    let ms =
        |seconds: f64| (seconds.is_finite() && seconds > 0.0).then_some((seconds * 1000.0) as u64);
    let duration_ms = info.duration.and_then(ms);
    // The elapsed time is as of `timestamp`; while playing it has moved on since by `rate`.
    let mut position = info.elapsed.unwrap_or(0.0);
    if info.playing
        && let Some(stamp) = info.timestamp
    {
        let rate = info
            .rate
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(1.0);
        position += ((now_ms - stamp) / 1000.0).max(0.0) * rate;
    }
    let mut position_ms = ms(position).unwrap_or(0);
    if let Some(duration) = duration_ms {
        position_ms = position_ms.min(duration);
    }
    Ok(Read::Track(Some(LocalTrack {
        title,
        artist: text(info.artist).unwrap_or_default(),
        album: text(info.album),
        art_url: None,
        position_ms,
        duration_ms,
        playing: info.playing,
    })))
}

#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct Reader {
    /// A failed run was logged; logged again only after a run worked.
    reported: bool,
}

#[cfg(target_os = "macos")]
impl Reader {
    pub fn release(&mut self) {}

    pub async fn read(&mut self) -> Option<LocalTrack> {
        if super::unavailable() {
            return None;
        }
        let mut command = tokio::process::Command::new("/usr/bin/osascript");
        command
            .args(["-l", "JavaScript", "-e", SCRIPT])
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let output = match tokio::time::timeout(TIMEOUT, command.output()).await {
            Ok(Ok(output)) if output.status.success() => output,
            Ok(Ok(output)) => return self.failed(&format!("exit status {}", output.status)),
            Ok(Err(e)) => return self.failed(&e.to_string()),
            Err(_) => return self.failed("timed out"),
        };
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
        match parse(&String::from_utf8_lossy(&output.stdout), now_ms) {
            Ok(Read::Track(track)) => {
                self.reported = false;
                track
            }
            Ok(Read::Unavailable) => {
                tracing::warn!("now playing: this macOS gives no access to MediaRemote");
                super::set_unavailable();
                None
            }
            // The error names a position only, never the track.
            Err(e) => self.failed(&format!("unreadable answer: {e}")),
        }
    }

    fn failed(&mut self, why: &str) -> Option<LocalTrack> {
        if !self.reported {
            tracing::warn!(why, "reading the macOS media player failed");
            self.reported = true;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_700_000_010_000.0;

    fn track(output: &str) -> Option<LocalTrack> {
        match parse(output, NOW).expect("valid JSON") {
            Read::Track(track) => track,
            Read::Unavailable => panic!("unexpectedly unavailable"),
        }
    }

    #[test]
    fn nothing_playing_is_no_track() {
        assert_eq!(track("null\n"), None);
        assert_eq!(track(r#"{"title":"  ","playing":true}"#), None);
    }

    #[test]
    fn a_missing_request_class_marks_the_feature_unavailable() {
        assert_eq!(
            parse(r#"{"error":"request"}"#, NOW).ok(),
            Some(Read::Unavailable)
        );
    }

    #[test]
    fn a_playing_track_moves_on_from_its_timestamp() {
        let t = track(
            r#"{"title":"Song","artist":"Band","album":"Record","duration":200.5,
                "elapsed":30,"rate":1,"timestamp":1700000000000,"playing":true}"#,
        )
        .expect("a track");
        assert_eq!(t.title, "Song");
        assert_eq!(t.artist, "Band");
        assert_eq!(t.album.as_deref(), Some("Record"));
        assert_eq!(t.duration_ms, Some(200_500));
        assert_eq!(t.position_ms, 40_000);
        assert!(t.playing);
        assert_eq!(t.art_url, None);
    }

    #[test]
    fn a_paused_track_keeps_its_position_and_never_passes_the_end() {
        let paused = track(
            r#"{"title":"Song","elapsed":30,"rate":0,"timestamp":1700000000000,"playing":false}"#,
        )
        .expect("a track");
        assert_eq!(paused.position_ms, 30_000);
        assert_eq!(paused.artist, "");
        assert_eq!(paused.duration_ms, None);
        let ended = track(
            r#"{"title":"Song","duration":35,"elapsed":30,"timestamp":1700000000000,"playing":true}"#,
        )
        .expect("a track");
        assert_eq!(ended.position_ms, 35_000);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse("execution error: -1728", NOW).is_err());
    }
}
