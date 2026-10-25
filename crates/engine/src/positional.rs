//! Positional audio (FR-1.41–1.43, FR-1.72).
//!
//! While the setting is on, a task asks the position sources (Mumble Link today, the official
//! game plugins of B31 later) every [`POLL`] for a game; nothing is read while it is off. The
//! first source with an active game wins:
//! - its camera pose becomes the listener of the playback mixer, which places every voice that
//!   carries a position around it (`mumcord_audio::spatial`);
//! - its context and identity are announced with `UserState.plugin_context/plugin_identity`
//!   like the official client does; the server forwards positions only between users with the
//!   same context, so only people in the same game are placed;
//! - its avatar position goes into every outgoing voice packet, once the server knows the
//!   current context (both voice formats carry it, see `mumcord_protocol::voice`).
//!
//! Independently of the setting, people whose voice packets carry a position are reported as
//! "sending positions" (the "3D" icon) until [`SENDING_TIMEOUT`] passes without such a packet;
//! the own entry is reported while Mumcord itself sends positions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_audio::spatial::Listener;
use mumcord_positional::{Fetch, LinkSource, PositionSource};

use crate::session::{Engine, EngineEvent, lock};

/// How often the sources are asked for positions (the official client reads them with every
/// audio packet).
pub const POLL: Duration = Duration::from_millis(20);
/// A person counts as sending positions until this long after their last positional packet.
pub const SENDING_TIMEOUT: Duration = Duration::from_secs(3);

/// What the settings page shows (FR-1.43) and who has the "3D" icon (FR-1.72).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PositionalStatus {
    pub enabled: bool,
    /// The source delivering positions right now, e.g. "Mumble Link"; `None` if no game does.
    pub source: Option<String>,
    /// The game's own name, if it reports one.
    pub game: Option<String>,
    /// Sessions currently sending positions, the own one included.
    pub senders: Vec<u32>,
}

/// Lock-free state read by the audio threads.
#[derive(Debug, Default)]
pub(crate) struct PositionalShared {
    /// The playback mixer's listener.
    pub listener: Arc<Listener>,
    sending: AtomicBool,
    avatar: [AtomicU32; 3],
}

impl PositionalShared {
    /// The own avatar position for the next voice packet; `None` while no positions are sent.
    pub fn own_position(&self) -> Option<[f32; 3]> {
        self.sending.load(Ordering::Acquire).then(|| {
            self.avatar
                .each_ref()
                .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
        })
    }

    fn set_own_position(&self, position: Option<[f32; 3]>) {
        match position {
            Some(p) => {
                for (slot, v) in self.avatar.iter().zip(p) {
                    slot.store(v.to_bits(), Ordering::Relaxed);
                }
                self.sending.store(true, Ordering::Release);
            }
            None => self.sending.store(false, Ordering::Release),
        }
    }
}

type Sources = Arc<Mutex<Vec<Box<dyn PositionSource>>>>;

/// Positional audio of the engine.
pub(crate) struct Positional {
    pub shared: Arc<PositionalShared>,
    sources: Sources,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    enabled: bool,
    /// Stop switch of the running poll thread.
    poller: Option<Arc<AtomicBool>>,
    /// (source, game) of the active game.
    active: Option<(String, Option<String>)>,
    /// Context and identity the server has from us on this connection; `None` while not
    /// connected.
    announced: Option<(Vec<u8>, String)>,
    /// Last positional packet per remote session.
    senders: HashMap<u32, Instant>,
    /// The own session while Mumcord sends positions.
    own_sending: Option<u32>,
}

impl Positional {
    /// `shared` is the state the voice pipeline reads (`VoiceShared::positional`).
    pub fn new(
        shared: Arc<PositionalShared>,
        settings: &mumcord_store::PositionalSettings,
    ) -> Self {
        shared
            .listener
            .set_distances(settings.min_distance, settings.max_distance);
        Self {
            shared,
            sources: Arc::new(Mutex::new(vec![Box::new(LinkSource::new())])),
            state: Mutex::new(State {
                enabled: settings.enabled,
                ..State::default()
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        lock(&self.state).enabled
    }

    /// A new connection is synced: the server knows no context from us yet (an empty one is
    /// its default) and nobody sends positions.
    pub fn on_synced(&self) {
        let mut state = lock(&self.state);
        state.announced = Some(Default::default());
        state.senders.clear();
        state.own_sending = None;
        self.shared.set_own_position(None);
    }

    pub fn on_disconnect(&self) {
        let mut state = lock(&self.state);
        state.announced = None;
        state.senders.clear();
        state.own_sending = None;
        self.shared.set_own_position(None);
    }

    pub fn on_user_removed(&self, session: u32) {
        lock(&self.state).senders.remove(&session);
    }

    /// A voice packet of `session` arrived; returns the new "sending positions" flag if it
    /// changed.
    pub fn on_voice_packet(&self, session: u32, positional: bool, now: Instant) -> Option<bool> {
        let mut state = lock(&self.state);
        if positional {
            state.senders.insert(session, now).is_none().then_some(true)
        } else {
            state.senders.remove(&session).map(|_| false)
        }
    }

    /// Sessions whose positions timed out (now reported as not sending).
    pub fn expire(&self, now: Instant) -> Vec<u32> {
        let mut state = lock(&self.state);
        let expired: Vec<u32> = state
            .senders
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(**at) >= SENDING_TIMEOUT)
            .map(|(s, _)| *s)
            .collect();
        for session in &expired {
            state.senders.remove(session);
        }
        expired
    }

    fn status(&self) -> PositionalStatus {
        let state = lock(&self.state);
        let (source, game) = state.active.clone().unzip();
        let mut senders: Vec<u32> = state
            .senders
            .keys()
            .copied()
            .chain(state.own_sending)
            .collect();
        senders.sort_unstable();
        PositionalStatus {
            enabled: state.enabled,
            source,
            game: game.flatten(),
            senders,
        }
    }
}

/// What one poll changed, for events and the server.
#[derive(Debug, Default, PartialEq)]
struct Applied {
    /// The active source or game changed.
    status_changed: bool,
    /// Context and identity to announce.
    announce: Option<(Vec<u8>, String)>,
    /// The own "sending positions" flag changed: (own session, sending).
    own_sending: Option<(u32, bool)>,
}

impl Positional {
    /// Applies a poll result. `own` is the own session while connected.
    fn apply(&self, fetch: Option<&Fetch>, own: Option<u32>) -> Applied {
        let mut state = lock(&self.state);
        let fetch = fetch.filter(|_| state.enabled);
        let mut applied = Applied::default();

        let active = fetch.map(|f| (f.source.clone(), f.game.clone()));
        if active != state.active {
            if let Some((source, _)) = &active {
                tracing::info!(source, "positional audio: a game delivers positions");
            } else if state.active.is_some() {
                tracing::info!("positional audio: no game delivers positions any more");
            }
            state.active = active;
            applied.status_changed = true;
        }

        match fetch {
            Some(f) => {
                let camera = f.data.camera;
                self.shared
                    .listener
                    .set_pose(camera.position, camera.front, camera.top);
            }
            None => self.shared.listener.clear(),
        }

        let wanted = fetch
            .map(|f| (f.data.context.clone(), f.data.identity.clone()))
            .unwrap_or_default();
        if let Some(announced) = &mut state.announced
            && *announced != wanted
        {
            *announced = wanted.clone();
            applied.announce = Some(wanted);
        }

        // Positions go out only once the server has the matching context.
        let sending = own.filter(|_| fetch.is_some() && state.announced.is_some());
        self.shared
            .set_own_position(sending.and(fetch).map(|f| f.data.avatar.position));
        if sending != state.own_sending {
            applied.own_sending = match (state.own_sending, sending) {
                (_, Some(s)) => Some((s, true)),
                (Some(s), None) => Some((s, false)),
                (None, None) => None,
            };
            state.own_sending = sending;
        }
        applied
    }
}

impl Engine {
    /// Positional audio status for the settings page and the "3D" icons.
    pub fn positional_status(&self) -> PositionalStatus {
        self.positional().status()
    }

    /// Replaces the position sources (default: Mumble Link). The app shell adds the plugin
    /// host here; tests use fake games.
    pub fn set_position_sources(&self, sources: Vec<Box<dyn PositionSource>>) {
        *lock(&self.positional().sources) = sources;
    }

    /// Applies the settings: switches polling on or off and sets the distances.
    pub(crate) fn positional_configure(&self, settings: &mumcord_store::PositionalSettings) {
        let positional = self.positional();
        positional
            .shared
            .listener
            .set_distances(settings.min_distance, settings.max_distance);
        let changed = {
            let mut state = lock(&positional.state);
            let changed = state.enabled != settings.enabled;
            state.enabled = settings.enabled;
            changed
        };
        if changed {
            tracing::info!(
                enabled = settings.enabled,
                "positional audio setting changed"
            );
            self.positional_poll_restart();
            self.events()
                .emit(EngineEvent::PositionalStatusChanged(positional.status()));
        }
    }

    /// Starts the poll thread if the setting is on (call once at start and on changes). A
    /// thread rather than a task: a poll may wait for the engine's locks.
    pub(crate) fn positional_poll_restart(&self) {
        let positional = self.positional();
        let mut state = lock(&positional.state);
        if let Some(stop) = state.poller.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if !state.enabled {
            drop(state);
            // Clear the listener, own position and the announced context right away.
            self.positional_poll_once();
            return;
        }
        let weak = self.downgrade();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let spawned = std::thread::Builder::new()
            .name("mumcord-positional".into())
            .spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(POLL);
                    match weak.upgrade() {
                        Some(engine) if !thread_stop.load(Ordering::Relaxed) => {
                            engine.positional_poll_once();
                        }
                        _ => return,
                    }
                }
            });
        match spawned {
            Ok(_) => state.poller = Some(stop),
            Err(e) => tracing::error!(error = %e, "starting positional audio failed"),
        }
    }

    fn positional_poll_once(&self) {
        let positional = self.positional();
        let fetch = if positional.enabled() {
            let now = Instant::now();
            lock(&positional.sources)
                .iter_mut()
                .find_map(|s| s.fetch(now))
        } else {
            None
        };
        let ctx = self.session_ctx().ok();
        let own = ctx
            .as_ref()
            .and_then(|ctx| ctx.handle.with_state(|s| s.session));
        let applied = positional.apply(fetch.as_ref(), own);
        if let (Some((context, identity)), Some(ctx)) = (applied.announce, &ctx)
            && let Err(e) = ctx.handle.set_plugin_context(context, identity)
        {
            tracing::warn!(error = %e, "announcing the game context failed");
        }
        if applied.status_changed {
            self.events()
                .emit(EngineEvent::PositionalStatusChanged(positional.status()));
        }
        if let Some((session, sending)) = applied.own_sending {
            self.events()
                .emit(EngineEvent::PositionalSendingChanged { session, sending });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_positional::{Pose, Positional as Data};

    fn settings(enabled: bool) -> mumcord_store::PositionalSettings {
        mumcord_store::PositionalSettings {
            enabled,
            ..Default::default()
        }
    }

    fn game(context: &[u8]) -> Fetch {
        Fetch {
            source: "Mumble Link".into(),
            game: Some("Game".into()),
            data: Data {
                avatar: Pose {
                    position: [1.0, 2.0, 3.0],
                    ..Pose::default()
                },
                camera: Pose {
                    position: [0.0; 3],
                    front: [0.0, 0.0, 1.0],
                    top: [0.0, 1.0, 0.0],
                },
                context: context.to_vec(),
                identity: "me".into(),
            },
        }
    }

    #[test]
    fn a_game_is_announced_then_positions_are_sent() {
        let p = Positional::new(Arc::default(), &settings(true));
        p.on_synced();
        let applied = p.apply(Some(&game(b"Link\0a")), Some(7));
        assert!(applied.status_changed);
        assert_eq!(applied.announce, Some((b"Link\0a".to_vec(), "me".into())));
        assert_eq!(applied.own_sending, Some((7, true)));
        assert_eq!(p.shared.own_position(), Some([1.0, 2.0, 3.0]));
        assert!(p.shared.listener.state().is_some());
        assert_eq!(p.status().senders, [7]);
        assert_eq!(p.status().source.as_deref(), Some("Mumble Link"));

        // Unchanged: nothing to announce or report.
        assert_eq!(
            p.apply(Some(&game(b"Link\0a")), Some(7)),
            Applied::default()
        );

        // The game closes: empty context, positions stop.
        let applied = p.apply(None, Some(7));
        assert!(applied.status_changed);
        assert_eq!(applied.announce, Some(Default::default()));
        assert_eq!(applied.own_sending, Some((7, false)));
        assert_eq!(p.shared.own_position(), None);
        assert!(p.shared.listener.state().is_none());
    }

    #[test]
    fn nothing_is_used_while_disabled_or_disconnected() {
        let p = Positional::new(Arc::default(), &settings(false));
        p.on_synced();
        let applied = p.apply(Some(&game(b"x")), Some(7));
        assert_eq!(applied, Applied::default());
        assert_eq!(p.shared.own_position(), None);

        let p = Positional::new(Arc::default(), &settings(true));
        let applied = p.apply(Some(&game(b"x")), None);
        assert!(applied.announce.is_none(), "not connected");
        assert_eq!(p.shared.own_position(), None);
        assert!(
            p.shared.listener.state().is_some(),
            "listening works offline"
        );
    }

    #[test]
    fn remote_senders_follow_packets_and_time_out() {
        let p = Positional::new(Arc::default(), &settings(false));
        let t0 = Instant::now();
        assert_eq!(p.on_voice_packet(3, true, t0), Some(true));
        assert_eq!(p.on_voice_packet(3, true, t0), None);
        assert_eq!(p.status().senders, [3]);
        assert!(p.expire(t0 + Duration::from_secs(1)).is_empty());
        assert_eq!(p.expire(t0 + SENDING_TIMEOUT), [3]);
        assert_eq!(p.on_voice_packet(3, false, t0), None);
        assert_eq!(p.on_voice_packet(4, true, t0), Some(true));
        assert_eq!(p.on_voice_packet(4, false, t0), Some(false));
    }
}
