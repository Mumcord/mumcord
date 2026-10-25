//! "Now gaming" status (FR-1.59, QA.44).
//!
//! The app shell asks its [`GameSource`]s (best first) for the game being played and hands the
//! result to [`Engine::now_gaming_update`] every [`POLL_INTERVAL`]; nothing is read while the
//! setting is off. Today's sources are the running Steam game and running programs matched
//! against a bundled list (see [`detect`]); a Mumble Link source (game name and context from
//! positional audio, B30) plugs in as one more [`GameSource`] in front of them.
//!
//! The engine shares the game like "Now playing" shares a track: plugin data to Mumcord users
//! (see [`payload`]) on every change and again every [`RESYNC`], plus once to everyone who
//! joins; a "🎮 Game" line at the end of the own comment for official clients (see
//! [`crate::now_playing::comment`]), changed only after a game has run for a few seconds.
//! Received payloads become [`EngineEvent::NowGamingChanged`]; the own game is reported the
//! same way (with the own session). Game icons come from the Steam CDN through the guarded
//! link-preview fetcher, only while link previews are on. "Join game" hands a validated
//! `steam://connect/<host:port>` link of another person to the operating system.

pub mod detect;
pub mod payload;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mumcord_protocol::{Handle, PluginData};

use crate::now_playing::{apply_comment, comment};
use crate::session::{Engine, EngineEvent, SessionCtx, lock};
pub use payload::{DATA_ID, SharedGame};

/// How often the app shell should look for a game and call [`Engine::now_gaming_update`].
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Unchanged state is sent again this often, for receivers that missed an update.
const RESYNC: Duration = Duration::from_secs(60);
/// The comment line follows a change only once it has lasted this long.
const COMMENT_SETTLE: Duration = Duration::from_secs(5);
const ICON_CACHE_ENTRIES: usize = 64;
/// Game icons are at most this many pixels per side.
const ICON_SIZE: u32 = 240;

/// A game as found on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalGame {
    pub name: String,
    pub steam_app_id: Option<u32>,
    /// `steam://connect/<host:port>` when the game exposes a server to join.
    pub join: Option<String>,
}

/// One way of finding the game being played. The app shell asks its sources in order and uses
/// the first answer; `detect` may block briefly (file and process reads), so it runs off the
/// UI thread.
pub trait GameSource: Send {
    fn detect(&mut self) -> Option<LocalGame>;
}

/// The first game any of `sources` reports, asked best first.
pub fn detect_first(sources: &mut [Box<dyn GameSource>]) -> Option<LocalGame> {
    sources.iter_mut().find_map(|s| s.detect())
}

/// A game for the UI. `played_ms` is the play time when the view was made (event emitted,
/// command answered); the UI advances it with its own clock from there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowGamingView {
    pub name: String,
    /// An icon can be requested with [`Engine::now_gaming_icon`].
    pub has_icon: bool,
    pub played_ms: u64,
    /// "Join game" can be offered ([`Engine::now_gaming_join_link`]).
    pub can_join: bool,
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The Steam CDN header image of an app.
fn icon_url(app_id: u32) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/header.jpg")
}

/// The engine's now-gaming state; lives as long as the engine, reset per connection.
#[derive(Default)]
pub(crate) struct NowGaming {
    /// The setting; while off, nothing is shared and the comment line is removed.
    enabled: bool,
    /// The game being played, as shared (with its start time).
    local: Option<SharedGame>,
    /// What was last sent to everyone, and when.
    sent: Option<(SharedGame, Instant)>,
    /// People who joined since the last send and still need the current game.
    joined: Vec<u32>,
    /// The line in the own comment; `None` until checked on this connection.
    comment_applied: Option<Option<String>>,
    /// The line that should be there, and since when.
    comment_wanted: Option<String>,
    comment_since: Option<Instant>,
    /// Games of other people (`started_at_ms` moved to the local clock).
    remote: HashMap<u32, SharedGame>,
    /// Steam app id → icon `data:` URL (`None`: could not be loaded).
    icons: HashMap<u32, Option<String>>,
    /// Whether a failed send was already logged on this connection.
    send_failure_logged: bool,
}

impl NowGaming {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// Forgets everything tied to the connection (new connection or disconnect).
    fn reset_session(&mut self) {
        self.sent = None;
        self.joined.clear();
        self.send_failure_logged = false;
        self.comment_applied = None;
        self.comment_wanted = None;
        self.comment_since = None;
        self.remote.clear();
    }

    /// Takes the latest detection; a different game (or none) restarts the play time.
    fn set_local(&mut self, game: Option<LocalGame>) {
        let same = match (&self.local, &game) {
            (Some(current), Some(game)) => {
                current.name == game.name && current.steam_app_id == game.steam_app_id
            }
            _ => false,
        };
        match game {
            Some(game) if same => {
                if let Some(current) = self.local.as_mut() {
                    current.join = game.join.as_deref().and_then(payload::join_link);
                }
            }
            Some(game) => {
                tracing::info!(
                    steam = game.steam_app_id.is_some(),
                    join = game.join.is_some(),
                    "now gaming: game detected"
                );
                self.local = Some(SharedGame {
                    name: game.name,
                    steam_app_id: game.steam_app_id,
                    started_at_ms: unix_ms(),
                    join: game.join.as_deref().and_then(payload::join_link),
                });
            }
            None => {
                if self.local.take().is_some() {
                    tracing::info!("now gaming: no game running");
                }
            }
        }
    }

    /// Sends what changed; returns the comment line to apply, if it must change.
    fn tick(&mut self, ctx: &SessionCtx, now: Instant) -> Option<Option<String>> {
        let current = self.local.clone();
        if needs_send(self.sent.as_ref(), current.as_ref(), now) {
            let own = ctx.handle.with_state(|s| s.session);
            let everyone: Vec<u32> = ctx.handle.with_state(|s| {
                s.users
                    .keys()
                    .copied()
                    .filter(|id| Some(*id) != own)
                    .collect()
            });
            self.send(&ctx.handle, &everyone, current.as_ref());
            self.sent = current.clone().map(|g| (g, now));
            // The own game never comes back as plugin data; the UI gets it like everyone else's.
            if let Some(own) = own {
                ctx.events.emit(EngineEvent::NowGamingChanged {
                    session: own,
                    game: current.as_ref().map(view),
                });
            }
            self.joined.clear();
        } else if !self.joined.is_empty() {
            let joined = std::mem::take(&mut self.joined);
            if current.is_some() {
                self.send(&ctx.handle, &joined, current.as_ref());
            }
        }

        let wanted = current.as_ref().map(|g| comment::game_line(&g.name));
        if self.comment_since.is_none() || self.comment_wanted != wanted {
            self.comment_wanted = wanted;
            self.comment_since = Some(now);
        }
        let settled = !self.enabled
            || self
                .comment_since
                .is_some_and(|since| now.duration_since(since) >= COMMENT_SETTLE);
        if settled && self.comment_applied.as_ref() != Some(&self.comment_wanted) {
            self.comment_applied = Some(self.comment_wanted.clone());
            return Some(self.comment_wanted.clone());
        }
        None
    }

    fn send(&mut self, handle: &Handle, receivers: &[u32], game: Option<&SharedGame>) {
        if receivers.is_empty() {
            return;
        }
        if let Err(e) = handle.send_plugin_data(receivers, DATA_ID, payload::encode(game)) {
            // Old servers (< 1.4) cannot forward plugin data; the comment line still works there.
            if !self.send_failure_logged {
                self.send_failure_logged = true;
                tracing::info!(error = %e, "now gaming: plugin data not sent; comment line only");
            }
        }
    }
}

/// Whether `current` must be sent, given what was last sent.
fn needs_send(
    sent: Option<&(SharedGame, Instant)>,
    current: Option<&SharedGame>,
    now: Instant,
) -> bool {
    match (sent, current) {
        (None, None) => false,
        (None, Some(_)) | (Some(_), None) => true,
        (Some((sent, at)), Some(current)) => {
            sent != current || now.saturating_duration_since(*at) >= RESYNC
        }
    }
}

fn view(game: &SharedGame) -> NowGamingView {
    NowGamingView {
        name: game.name.clone(),
        has_icon: game.steam_app_id.is_some(),
        played_ms: unix_ms().saturating_sub(game.started_at_ms),
        can_join: game.join.is_some(),
    }
}

/// Plugin data from another person (session task).
pub(crate) fn on_plugin_data(state: &Mutex<NowGaming>, ctx: &SessionCtx, data: &PluginData) {
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
    let game = match payload::decode(&data.data) {
        Ok(game) => game,
        Err(e) => {
            tracing::debug!(error = %e, "ignoring a now-gaming payload");
            return;
        }
    };
    let update = {
        let mut state = lock(state);
        match game {
            Some(mut game) => {
                // A start time in the future (clock differences) counts as "just started".
                game.started_at_ms = game.started_at_ms.min(unix_ms());
                let update = view(&game);
                state.remote.insert(sender, game);
                Some(update)
            }
            None if state.remote.remove(&sender).is_some() => None,
            None => return,
        }
    };
    ctx.events.emit(EngineEvent::NowGamingChanged {
        session: sender,
        game: update,
    });
}

/// Someone joined the server: they get the current game with the next update.
pub(crate) fn on_user_joined(state: &Mutex<NowGaming>, ctx: &SessionCtx, session: u32) {
    if ctx.handle.with_state(|s| s.session) != Some(session) {
        lock(state).joined.push(session);
    }
}

pub(crate) fn on_user_removed(state: &Mutex<NowGaming>, session: u32) {
    lock(state).remote.remove(&session);
}

/// A connection (re)synced: everything is shared afresh and the comment checked again.
pub(crate) fn on_synced(state: &Mutex<NowGaming>) {
    lock(state).reset_session();
}

/// The user disconnects: remove the comment line while the connection still exists (best
/// effort; a line left behind is removed on the next connect).
pub(crate) fn on_disconnect(state: &Mutex<NowGaming>, handle: &Handle) {
    let mut state = lock(state);
    if matches!(state.comment_applied, Some(Some(_)))
        && let Some(current) = handle.with_state(|s| s.own_user().and_then(|u| u.comment.clone()))
    {
        let without = comment::with_line(&current, comment::GAME_MARKER, None);
        if without != current
            && let Err(e) = handle.set_comment(without)
        {
            tracing::debug!(error = %e, "removing the now-gaming line failed");
        }
    }
    state.reset_session();
}

impl Engine {
    /// Whether the user turned "Now gaming" on; the app shell looks for games only then.
    pub fn now_gaming_enabled(&self) -> bool {
        lock(self.now_gaming_state()).enabled
    }

    /// The game being played on this computer (`None`: none, or the setting is off). Called by
    /// the app shell every [`POLL_INTERVAL`], also when nothing changed.
    pub fn now_gaming_update(&self, game: Option<LocalGame>) {
        let now = Instant::now();
        let ctx = self.session_ctx().ok();
        let job = {
            let mut state = lock(self.now_gaming_state());
            let game = game.filter(|_| state.enabled);
            state.set_local(game);
            ctx.as_ref().and_then(|ctx| state.tick(ctx, now))
        };
        if let (Some(line), Some(ctx)) = (job, ctx) {
            self.runtime()
                .spawn(apply_comment(ctx, comment::GAME_MARKER, line));
        }
    }

    /// Applies the setting; turning it off stops sharing and removes the comment line now.
    pub(crate) fn now_gaming_set_enabled(&self, enabled: bool) {
        tracing::info!(enabled, "now gaming setting changed");
        lock(self.now_gaming_state()).enabled = enabled;
        if !enabled {
            self.now_gaming_update(None);
        }
    }

    /// The own game as shared (for the settings preview).
    pub fn now_gaming_local(&self) -> Option<NowGamingView> {
        lock(self.now_gaming_state()).local.as_ref().map(view)
    }

    /// The `steam://connect/…` link of a person's game, if they share one (never the own).
    pub fn now_gaming_join_link(&self, session: u32) -> Option<String> {
        lock(self.now_gaming_state())
            .remote
            .get(&session)
            .and_then(|g| g.join.clone())
    }

    /// Icon (`data:` URL) of a person's game, or of the own one; `None` while link previews
    /// are off (nothing is fetched then) or when there is no usable icon.
    pub async fn now_gaming_icon(&self, session: u32) -> Option<String> {
        match self.settings() {
            Ok(settings) if settings.link_previews => {}
            Ok(_) => return None,
            Err(e) => {
                tracing::warn!(error = %e, "reading the settings for a game icon failed");
                return None;
            }
        }
        let own = self
            .session_ctx()
            .ok()
            .and_then(|ctx| ctx.handle.with_state(|s| s.session));
        let app_id = {
            let state = lock(self.now_gaming_state());
            let game = if own == Some(session) {
                state.local.as_ref()
            } else {
                state.remote.get(&session)
            };
            let app_id = game.and_then(|g| g.steam_app_id)?;
            if let Some(cached) = state.icons.get(&app_id) {
                return cached.clone();
            }
            app_id
        };
        let icon = match self.link_previews().fetcher() {
            Ok(fetcher) => match fetcher.image(&icon_url(app_id)).await {
                Ok(image) => tokio::task::spawn_blocking(move || {
                    crate::web::thumbnail_data_url(&image, ICON_SIZE)
                })
                .await
                .ok()
                .flatten(),
                Err(e) => {
                    tracing::debug!(error = %e, "loading a game icon failed");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "the web fetcher is unavailable");
                None
            }
        };
        let mut state = lock(self.now_gaming_state());
        if state.icons.len() >= ICON_CACHE_ENTRIES {
            state.icons.clear();
        }
        state.icons.insert(app_id, icon.clone());
        icon
    }

    /// The own comment with the current now-gaming line kept at its end (`set_own_comment`).
    pub(crate) fn with_now_gaming_line(&self, html: &str) -> String {
        let line = lock(self.now_gaming_state())
            .comment_applied
            .clone()
            .flatten();
        comment::with_line(html, comment::GAME_MARKER, line.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(name: &str) -> SharedGame {
        SharedGame {
            name: name.into(),
            steam_app_id: Some(730),
            started_at_ms: 1000,
            join: None,
        }
    }

    #[test]
    fn sends_on_change_and_resync_only() {
        let t0 = Instant::now();
        let sent = (game("A"), t0);
        let later = |s: u64| t0 + Duration::from_secs(s);
        assert!(!needs_send(None, None, t0));
        assert!(needs_send(None, Some(&sent.0), t0));
        assert!(needs_send(Some(&sent), None, later(1)));
        assert!(!needs_send(Some(&sent), Some(&game("A")), later(30)));
        assert!(needs_send(Some(&sent), Some(&game("A")), later(60)));
        assert!(needs_send(Some(&sent), Some(&game("B")), later(5)));
        let joinable = SharedGame {
            join: Some("steam://connect/192.0.2.1:27015".into()),
            ..game("A")
        };
        assert!(needs_send(Some(&sent), Some(&joinable), later(5)));
    }

    #[test]
    fn play_time_restarts_only_for_another_game() {
        let mut state = NowGaming::new(true);
        let local = |name: &str, join: Option<&str>| LocalGame {
            name: name.into(),
            steam_app_id: None,
            join: join.map(str::to_owned),
        };
        state.set_local(Some(local("A", None)));
        let started = state.local.as_ref().expect("game").started_at_ms;
        state.local.as_mut().expect("game").started_at_ms = started - 60_000;
        state.set_local(Some(local("A", Some("steam://connect/192.0.2.1:27015"))));
        let kept = state.local.as_ref().expect("game");
        assert_eq!(kept.started_at_ms, started - 60_000);
        assert!(kept.join.is_some());
        state.set_local(Some(local("A", Some("steam://run/1"))));
        assert_eq!(state.local.as_ref().expect("game").join, None);
        state.set_local(Some(local("B", None)));
        assert!(state.local.as_ref().expect("game").started_at_ms >= started);
        state.set_local(None);
        assert!(state.local.is_none());
    }

    struct Fixed(Option<LocalGame>);

    impl GameSource for Fixed {
        fn detect(&mut self) -> Option<LocalGame> {
            self.0.clone()
        }
    }

    #[test]
    fn first_source_with_an_answer_wins() {
        let found = |name: &str| {
            Some(LocalGame {
                name: name.into(),
                steam_app_id: None,
                join: None,
            })
        };
        let mut sources: Vec<Box<dyn GameSource>> = vec![
            Box::new(Fixed(None)),
            Box::new(Fixed(found("Link game"))),
            Box::new(Fixed(found("Listed game"))),
        ];
        assert_eq!(
            detect_first(&mut sources).map(|g| g.name),
            Some("Link game".into())
        );
        assert_eq!(detect_first(&mut []), None);
    }

    #[test]
    fn views_tell_what_the_ui_can_offer() {
        let mut g = game("A");
        g.started_at_ms = unix_ms() - 4_800_000;
        let v = view(&g);
        assert!(v.has_icon);
        assert!(!v.can_join);
        assert!(v.played_ms >= 4_800_000);
        assert_eq!(
            icon_url(730),
            "https://cdn.cloudflare.steamstatic.com/steam/apps/730/header.jpg"
        );
    }
}
