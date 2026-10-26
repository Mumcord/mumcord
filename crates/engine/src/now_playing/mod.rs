//! "Now playing" status (FR-1.58, QA.40).
//!
//! The app shell reads the current track from the system's media player and hands it to
//! [`Engine::now_playing_update`] every [`POLL_INTERVAL`]; nothing is read while the setting
//! is off. The engine shares it with the other people on the server as plugin data (see
//! [`payload`]): on every change (new track, play/pause, seek) and again every 30 s, plus once
//! to everyone who joins. Official clients get a "🎵 Artist – Title" line at the end of the own
//! comment instead (see [`comment`]); it changes only after a track has played for a few
//! seconds, so skipping through a playlist does not flood the server with comment updates.
//!
//! Received payloads become [`EngineEvent::NowPlayingChanged`] right away; the own track is
//! reported the same way (with the own session) whenever it is shared. Receivers anchor the
//! position to their own time of arrival, not the sender's clock (`ts`), so clock differences
//! between computers do not skew the progress bar; [`extrapolate`] advances it while playing. Covers are fetched
//! on request through the guarded link-preview fetcher, only while link previews are on.

pub mod comment;
pub mod payload;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mumcord_protocol::{Handle, PluginData};

use crate::ServerId;
use crate::session::{Engine, EngineEvent, SessionCtx, lock};
pub use payload::{DATA_ID, SharedTrack};

/// How often the app shell should read the media player and call
/// [`Engine::now_playing_update`].
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Unchanged state is sent again this often, for receivers that missed an update.
const RESYNC: Duration = Duration::from_secs(30);
/// A position this far off the expected one counts as a seek and is shared right away.
const SEEK_TOLERANCE_MS: u64 = 3000;
/// The comment line follows a change only once it has lasted this long.
const COMMENT_SETTLE: Duration = Duration::from_secs(5);
/// How long to wait for the own comment when the server announced it only by hash.
const COMMENT_WAIT: Duration = Duration::from_secs(3);
const COVER_CACHE_ENTRIES: usize = 64;
/// Cover thumbnails are at most this many pixels per side.
const COVER_SIZE: u32 = 160;

/// A track as read from the system's media player; `position_ms` is the position when read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTrack {
    pub title: String,
    /// Empty when the player does not report one.
    pub artist: String,
    pub album: Option<String>,
    /// Cover image link as the player reports it (only http(s) links are shared).
    pub art_url: Option<String>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub playing: bool,
}

/// A track for the UI. `position_ms` is the position when the view was made (event emitted,
/// command answered); while `playing`, the UI advances it with its own clock from there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlayingView {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// A cover can be requested with [`Engine::now_playing_cover`].
    pub has_cover: bool,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub playing: bool,
}

/// The playback position `elapsed_ms` after it was `position_ms`: advanced while playing and
/// clamped to the duration when known.
pub fn extrapolate(
    position_ms: u64,
    duration_ms: Option<u64>,
    playing: bool,
    elapsed_ms: u64,
) -> u64 {
    let position = if playing {
        position_ms.saturating_add(elapsed_ms)
    } else {
        position_ms
    };
    duration_ms.map_or(position, |d| position.min(d))
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// The engine's now-playing state; lives as long as the engine. The track is shared on every
/// connected server (FR-2.4), each with its own [`Share`].
#[derive(Default)]
pub(crate) struct NowPlaying {
    /// The setting; while off, nothing is shared and the comment line is removed.
    enabled: bool,
    /// The latest track from the app shell and when it was read.
    local: Option<(LocalTrack, Instant)>,
    servers: HashMap<ServerId, Share>,
    /// Cover link → thumbnail `data:` URL (`None`: could not be loaded).
    covers: HashMap<String, Option<String>>,
}

/// What was shared on one connection; reset per connection.
#[derive(Default)]
struct Share {
    /// What was last sent to everyone, with its position at `at`.
    sent: Option<(LocalTrack, Instant)>,
    /// People who joined since the last send and still need the current track.
    joined: Vec<u32>,
    comment: CommentSync,
    /// Tracks of other people (positions as of their arrival).
    remote: HashMap<u32, SharedTrack>,
    /// Whether a failed send was already logged on this connection.
    send_failure_logged: bool,
}

#[derive(Default)]
struct CommentSync {
    /// The line in the own comment; `None` until checked on this connection.
    applied: Option<Option<String>>,
    /// The line that should be there, and since when.
    wanted: Option<String>,
    since: Option<Instant>,
}

impl NowPlaying {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// Sends what changed on `ctx`'s server; returns the comment line to apply there, if it
    /// must change.
    fn tick(&mut self, ctx: &SessionCtx, now: Instant) -> Option<Option<String>> {
        let current = self.local.as_ref().map(|(t, at)| advanced(t, *at, now));
        let enabled = self.enabled;
        self.servers
            .entry(ctx.server_id)
            .or_default()
            .tick(ctx, current, enabled, now)
    }
}

impl Share {
    fn tick(
        &mut self,
        ctx: &SessionCtx,
        current: Option<LocalTrack>,
        enabled: bool,
        now: Instant,
    ) -> Option<Option<String>> {
        if needs_send(self.sent.as_ref(), current.as_ref(), now) {
            let own = ctx.handle.with_state(|s| s.session);
            let everyone: Vec<u32> = ctx.handle.with_state(|s| {
                s.users
                    .keys()
                    .copied()
                    .filter(|id| Some(*id) != own)
                    .collect()
            });
            let changed = match (self.sent.as_ref(), current.as_ref()) {
                (Some((sent, _)), Some(current)) => !same_track(sent, current),
                (None, None) => false,
                _ => true,
            };
            if changed {
                tracing::info!(
                    receivers = everyone.len(),
                    playing = current.as_ref().map(|t| t.playing),
                    "now playing: sharing a changed track state"
                );
            }
            self.send(&ctx.handle, &everyone, current.as_ref());
            self.sent = current.clone().map(|t| (t, now));
            // The own track never comes back as plugin data; the UI gets it the same way as
            // everyone else's, so the own rows, preview, card and user panel can show it.
            if let Some(own) = own {
                ctx.events.emit(EngineEvent::NowPlayingChanged {
                    session: own,
                    track: current.as_ref().map(|t| view(&shared(t))),
                });
            }
            self.joined.clear();
        } else if !self.joined.is_empty() {
            let joined = std::mem::take(&mut self.joined);
            if current.is_some() {
                self.send(&ctx.handle, &joined, current.as_ref());
            }
        }

        let wanted = current
            .as_ref()
            .filter(|t| t.playing)
            .map(|t| comment::line(&t.artist, &t.title));
        if self.comment.since.is_none() || self.comment.wanted != wanted {
            self.comment.wanted = wanted;
            self.comment.since = Some(now);
        }
        let settled = !enabled
            || self
                .comment
                .since
                .is_some_and(|since| now.duration_since(since) >= COMMENT_SETTLE);
        if settled && self.comment.applied.as_ref() != Some(&self.comment.wanted) {
            self.comment.applied = Some(self.comment.wanted.clone());
            return Some(self.comment.wanted.clone());
        }
        None
    }

    fn send(&mut self, handle: &Handle, receivers: &[u32], track: Option<&LocalTrack>) {
        if receivers.is_empty() {
            return;
        }
        let data = payload::encode(track.map(shared).as_ref());
        if let Err(e) = handle.send_plugin_data(receivers, DATA_ID, data) {
            // Old servers (< 1.4) cannot forward plugin data; the comment line still works there.
            if !self.send_failure_logged {
                self.send_failure_logged = true;
                tracing::info!(error = %e, "now playing: plugin data not sent; comment line only");
            }
        }
    }
}

/// `track` with its position moved from `read_at` to `now`.
fn advanced(track: &LocalTrack, read_at: Instant, now: Instant) -> LocalTrack {
    LocalTrack {
        position_ms: extrapolate(
            track.position_ms,
            track.duration_ms,
            track.playing,
            millis(now.saturating_duration_since(read_at)),
        ),
        ..track.clone()
    }
}

/// Logs when the detected track changes; never titles or names (personal data).
fn log_local_change(before: Option<&LocalTrack>, after: Option<&LocalTrack>) {
    match (before, after) {
        (_, Some(after)) if before.is_none_or(|b| !same_track(b, after)) => tracing::info!(
            duration_s = after.duration_ms.map(|d| d / 1000),
            playing = after.playing,
            has_artist = !after.artist.is_empty(),
            has_cover = after.art_url.is_some(),
            "now playing: track detected"
        ),
        (Some(_), None) => tracing::info!("now playing: nothing playing"),
        _ => {}
    }
}

fn same_track(a: &LocalTrack, b: &LocalTrack) -> bool {
    a.title == b.title
        && a.artist == b.artist
        && a.album == b.album
        && a.art_url == b.art_url
        && a.duration_ms == b.duration_ms
        && a.playing == b.playing
}

/// Whether `current` (position as of `now`) must be sent, given what was last sent.
fn needs_send(
    sent: Option<&(LocalTrack, Instant)>,
    current: Option<&LocalTrack>,
    now: Instant,
) -> bool {
    match (sent, current) {
        (None, None) => false,
        (None, Some(_)) | (Some(_), None) => true,
        (Some((sent, at)), Some(current)) => {
            if !same_track(sent, current) {
                return true;
            }
            let expected = advanced(sent, *at, now).position_ms;
            expected.abs_diff(current.position_ms) > SEEK_TOLERANCE_MS
                || now.saturating_duration_since(*at) >= RESYNC
        }
    }
}

fn shared(track: &LocalTrack) -> SharedTrack {
    SharedTrack {
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        art: track.art_url.as_deref().and_then(payload::art_link),
        position_ms: track.position_ms,
        duration_ms: track.duration_ms,
        playing: track.playing,
        ts_ms: unix_ms(),
    }
}

fn view(track: &SharedTrack) -> NowPlayingView {
    NowPlayingView {
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        has_cover: track.art.is_some(),
        position_ms: track.position_ms,
        duration_ms: track.duration_ms,
        playing: track.playing,
    }
}

/// Plugin data from another person (session task).
pub(crate) fn on_plugin_data(state: &Mutex<NowPlaying>, ctx: &SessionCtx, data: &PluginData) {
    if data.data_id != DATA_ID {
        return;
    }
    let Some(sender) = data.sender else {
        return;
    };
    let (own, known) = ctx
        .handle
        .with_state(|s| (s.session, s.users.contains_key(&sender)));
    if own == Some(sender) || !known {
        return;
    }
    let track = match payload::decode(&data.data) {
        Ok(track) => track,
        Err(e) => {
            tracing::debug!(error = %e, "ignoring a now-playing payload");
            return;
        }
    };
    let update = {
        let mut state = lock(state);
        match track {
            Some(track) => {
                let update = view(&track);
                state
                    .servers
                    .entry(ctx.server_id)
                    .or_default()
                    .remote
                    .insert(sender, track);
                Some(update)
            }
            None if state
                .servers
                .get_mut(&ctx.server_id)
                .is_some_and(|share| share.remote.remove(&sender).is_some()) =>
            {
                None
            }
            None => return,
        }
    };
    ctx.events.emit(EngineEvent::NowPlayingChanged {
        session: sender,
        track: update,
    });
}

/// Someone joined the server: they get the current track with the next update.
pub(crate) fn on_user_joined(state: &Mutex<NowPlaying>, ctx: &SessionCtx, session: u32) {
    if ctx.handle.with_state(|s| s.session) != Some(session) {
        lock(state)
            .servers
            .entry(ctx.server_id)
            .or_default()
            .joined
            .push(session);
    }
}

pub(crate) fn on_user_removed(state: &Mutex<NowPlaying>, server_id: ServerId, session: u32) {
    if let Some(share) = lock(state).servers.get_mut(&server_id) {
        share.remote.remove(&session);
    }
}

/// A connection (re)synced: everything is shared afresh and the comment checked again.
pub(crate) fn on_synced(state: &Mutex<NowPlaying>, server_id: ServerId) {
    lock(state).servers.insert(server_id, Share::default());
}

/// The user disconnects from a server: remove the comment line while the connection still
/// exists (best effort; a line left behind is removed on the next connect).
pub(crate) fn on_disconnect(state: &Mutex<NowPlaying>, server_id: ServerId, handle: &Handle) {
    let Some(share) = lock(state).servers.remove(&server_id) else {
        return;
    };
    if matches!(share.comment.applied, Some(Some(_)))
        && let Some(current) = handle.with_state(|s| s.own_user().and_then(|u| u.comment.clone()))
    {
        let stripped = comment::strip(&current);
        if stripped != current
            && let Err(e) = handle.set_comment(stripped.to_owned())
        {
            tracing::debug!(error = %e, "removing the now-playing line failed");
        }
    }
}

/// The own comment, fetched from the server if it was announced only by hash.
async fn own_comment(ctx: &SessionCtx) -> Option<String> {
    let read = || {
        ctx.handle.with_state(|s| {
            s.own_user()
                .map(|u| (u.session, u.comment.clone(), u.comment_hash.is_some()))
        })
    };
    let (session, comment, hashed) = read()?;
    if let Some(comment) = comment {
        return Some(comment);
    }
    if !hashed {
        return Some(String::new());
    }
    ctx.handle.request_blobs(&[], &[session], &[]).ok()?;
    let deadline = Instant::now() + COMMENT_WAIT;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some((_, Some(comment), _)) = read() {
            return Some(comment);
        }
    }
    None
}

/// Serialises status line updates, so "Now playing" and "Now gaming" never overwrite each
/// other's line with a stale comment.
static COMMENT_UPDATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Puts the status `line` for `marker` at the end of the own comment (or removes it for `None`).
pub(crate) async fn apply_comment(ctx: SessionCtx, marker: &'static str, line: Option<String>) {
    let _serial = COMMENT_UPDATE.lock().await;
    let Some(current) = own_comment(&ctx).await else {
        tracing::warn!("the own comment did not arrive; status line not updated");
        return;
    };
    let next = comment::with_line(&current, marker, line.as_deref());
    if next == current {
        return;
    }
    if line.is_some() && crate::chat::check_text_size(&ctx, &next).is_err() {
        tracing::info!("comment too long for the server with the status line; left out");
        return;
    }
    match ctx.handle.set_comment(next.clone()) {
        Ok(()) => {
            tracing::info!(marker, line = line.is_some(), "comment status line updated");
            wait_for_comment(&ctx, &next).await;
        }
        Err(e) => tracing::warn!(error = %e, "updating the comment status line failed"),
    }
}

/// Waits briefly until the server confirms `comment`, so the next update builds on it.
async fn wait_for_comment(ctx: &SessionCtx, comment: &str) {
    let deadline = Instant::now() + COMMENT_WAIT;
    while Instant::now() < deadline {
        let confirmed = ctx.handle.with_state(|s| {
            s.own_user()
                .is_some_and(|u| u.comment.as_deref() == Some(comment))
        });
        if confirmed {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

impl Engine {
    /// Whether the user turned "Now playing" on; the app shell reads the media player only then.
    pub fn now_playing_enabled(&self) -> bool {
        lock(self.now_playing_state()).enabled
    }

    /// The track currently playing on this computer (`None`: nothing, or the setting is off).
    /// Called by the app shell every [`POLL_INTERVAL`], also when nothing changed.
    pub fn now_playing_update(&self, track: Option<LocalTrack>) {
        let now = Instant::now();
        let ctxs = self.session_ctxs();
        let jobs: Vec<_> = {
            let mut state = lock(self.now_playing_state());
            let track = track.filter(|_| state.enabled);
            log_local_change(state.local.as_ref().map(|(t, _)| t), track.as_ref());
            state.local = track.map(|t| (t, now));
            ctxs.into_iter()
                .filter_map(|ctx| state.tick(&ctx, now).map(|line| (ctx, line)))
                .collect()
        };
        for (ctx, line) in jobs {
            self.runtime()
                .spawn(apply_comment(ctx, comment::MARKER, line));
        }
    }

    /// Applies the setting; turning it off stops sharing and removes the comment line now.
    pub(crate) fn now_playing_set_enabled(&self, enabled: bool) {
        tracing::info!(enabled, "now playing setting changed");
        lock(self.now_playing_state()).enabled = enabled;
        if !enabled {
            self.now_playing_update(None);
        }
    }

    /// The own track as shared (for the settings preview).
    pub fn now_playing_local(&self) -> Option<NowPlayingView> {
        let state = lock(self.now_playing_state());
        let (track, at) = state.local.as_ref()?;
        let current = advanced(track, *at, Instant::now());
        Some(view(&shared(&current)))
    }

    /// Cover thumbnail (`data:` URL) of a person's track on `server_id`, or of the own one;
    /// `None` while link previews are off (nothing is fetched then) or when there is no usable
    /// cover.
    pub async fn now_playing_cover(&self, server_id: ServerId, session: u32) -> Option<String> {
        match self.settings() {
            Ok(settings) if settings.link_previews => {}
            Ok(_) => return None,
            Err(e) => {
                tracing::warn!(error = %e, "reading the settings for a cover failed");
                return None;
            }
        }
        let own = self
            .session_ctx_for(server_id)
            .ok()
            .and_then(|ctx| ctx.handle.with_state(|s| s.session));
        let url = {
            let state = lock(self.now_playing_state());
            let url = if own == Some(session) {
                state
                    .local
                    .as_ref()
                    .and_then(|(t, _)| t.art_url.as_deref().and_then(payload::art_link))
            } else {
                state
                    .servers
                    .get(&server_id)
                    .and_then(|share| share.remote.get(&session))
                    .and_then(|t| t.art.clone())
            }?;
            if let Some(cached) = state.covers.get(&url) {
                return cached.clone();
            }
            url
        };
        let thumbnail = match self.link_previews().fetcher() {
            Ok(fetcher) => match fetcher.image(&url).await {
                Ok(image) => tokio::task::spawn_blocking(move || {
                    crate::web::thumbnail_data_url(&image, COVER_SIZE)
                })
                .await
                .ok()
                .flatten(),
                Err(e) => {
                    tracing::debug!(error = %e, "loading a cover failed");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "the web fetcher is unavailable");
                None
            }
        };
        let mut state = lock(self.now_playing_state());
        if state.covers.len() >= COVER_CACHE_ENTRIES {
            state.covers.clear();
        }
        state.covers.insert(url, thumbnail.clone());
        thumbnail
    }

    /// The own comment on `server_id` with the current now-playing line kept at its end
    /// (`set_own_comment`).
    pub(crate) fn with_now_playing_line(&self, server_id: ServerId, html: &str) -> String {
        let line = lock(self.now_playing_state())
            .servers
            .get(&server_id)
            .and_then(|share| share.comment.applied.clone().flatten());
        comment::with_line(html, comment::MARKER, line.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, position_ms: u64, playing: bool) -> LocalTrack {
        LocalTrack {
            title: title.into(),
            artist: "Band".into(),
            album: None,
            art_url: None,
            position_ms,
            duration_ms: Some(200_000),
            playing,
        }
    }

    #[test]
    fn extrapolates_only_while_playing_and_stops_at_the_end() {
        assert_eq!(extrapolate(1000, Some(5000), true, 1500), 2500);
        assert_eq!(extrapolate(1000, Some(5000), false, 1500), 1000);
        assert_eq!(extrapolate(4000, Some(5000), true, 9000), 5000);
        assert_eq!(extrapolate(4000, None, true, 9000), 13_000);
    }

    #[test]
    fn sends_on_change_seek_and_resync_only() {
        let t0 = Instant::now();
        let sent = (track("A", 10_000, true), t0);
        let later = |s: u64| t0 + Duration::from_secs(s);
        // Nothing playing before and now.
        assert!(!needs_send(None, None, t0));
        // Started and stopped.
        assert!(needs_send(None, Some(&sent.0), t0));
        assert!(needs_send(Some(&sent), None, later(1)));
        // Playing on as expected: quiet until the resync.
        assert!(!needs_send(
            Some(&sent),
            Some(&track("A", 20_000, true)),
            later(10)
        ));
        assert!(needs_send(
            Some(&sent),
            Some(&track("A", 40_000, true)),
            later(30)
        ));
        // New track, pause, seek.
        assert!(needs_send(
            Some(&sent),
            Some(&track("B", 0, true)),
            later(2)
        ));
        assert!(needs_send(
            Some(&sent),
            Some(&track("A", 12_000, false)),
            later(2)
        ));
        assert!(needs_send(
            Some(&sent),
            Some(&track("A", 90_000, true)),
            later(2)
        ));
    }

    #[test]
    fn position_follows_the_time_since_reading() {
        let t0 = Instant::now();
        let moved = advanced(
            &track("A", 1000, true),
            t0,
            t0 + Duration::from_millis(2500),
        );
        assert_eq!(moved.position_ms, 3500);
    }
}
