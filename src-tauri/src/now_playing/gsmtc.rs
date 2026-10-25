//! Windows system media session (GlobalSystemMediaTransportControlsSessionManager): every app
//! that shows media controls in the volume flyout reports title, artist, album and timeline.
//! Covers come only as image streams, not links, so Windows shares no cover.

use std::time::{SystemTime, UNIX_EPOCH};

use mumcord_engine::LocalTrack;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};

/// 100 ns ticks between 1601-01-01 (Windows `DateTime`) and 1970-01-01.
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

#[derive(Default)]
pub struct Reader {
    reported: bool,
}

impl Reader {
    pub fn release(&mut self) {}

    pub async fn read(&mut self) -> Option<LocalTrack> {
        // The WinRT calls block; keep them off the async workers.
        match tokio::task::spawn_blocking(current).await {
            Ok(Ok(track)) => {
                self.reported = false;
                track
            }
            Ok(Err(e)) => {
                if !self.reported {
                    tracing::warn!(error = %e, "reading the system media session failed");
                    self.reported = true;
                }
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "the media session reader stopped");
                None
            }
        }
    }
}

fn current() -> windows::core::Result<Option<LocalTrack>> {
    let manager = Manager::RequestAsync()?.join()?;
    let mut best: Option<(bool, bool, Session)> = None;
    for session in manager.GetSessions()? {
        let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;
        let spotify = session
            .SourceAppUserModelId()
            .map(|id| id.to_string_lossy().to_lowercase().contains("spotify"))
            .unwrap_or(false);
        let better = best
            .as_ref()
            .is_none_or(|(p, s, _)| (playing, spotify) > (*p, *s));
        if better {
            best = Some((playing, spotify, session));
        }
    }
    let Some((_, _, session)) = best else {
        return Ok(None);
    };
    read_session(&session)
}

fn read_session(session: &Session) -> windows::core::Result<Option<LocalTrack>> {
    let status = session.GetPlaybackInfo()?.PlaybackStatus()?;
    let playing = match status {
        Status::Playing => true,
        Status::Paused => false,
        _ => return Ok(None),
    };
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let title = props.Title()?.to_string_lossy().trim().to_owned();
    if title.is_empty() {
        return Ok(None);
    }
    let artist = props.Artist()?.to_string_lossy().trim().to_owned();
    let album =
        Some(props.AlbumTitle()?.to_string_lossy().trim().to_owned()).filter(|a| !a.is_empty());
    let timeline = session.GetTimelineProperties()?;
    let ticks_ms = |ticks: i64| u64::try_from(ticks.max(0) / 10_000).unwrap_or(0);
    let start = timeline.StartTime()?.Duration;
    let duration_ms = Some(ticks_ms(timeline.EndTime()?.Duration - start)).filter(|d| *d > 0);
    // The position is as of the last update; move it on while playing.
    let mut position = timeline.Position()?.Duration - start;
    if playing {
        position += (now_ticks() - timeline.LastUpdatedTime()?.UniversalTime).max(0);
    }
    let position_ms = ticks_ms(position);
    Ok(Some(LocalTrack {
        title,
        artist,
        album,
        art_url: None,
        position_ms: duration_ms.map_or(position_ms, |d| position_ms.min(d)),
        duration_ms,
        playing,
    }))
}

fn now_ticks() -> i64 {
    let since_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX));
    UNIX_EPOCH_TICKS.saturating_add(since_unix)
}
