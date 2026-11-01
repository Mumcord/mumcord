//! Notification sounds (FR-1.27): short, soft tones for people joining or leaving your channel,
//! your own mute/deafen changes, new messages, your own join of a channel for voice (QA.23) and
//! hanging up (QA.19). No speech.
//!
//! The [`SoundHub`] observes the engine's own event flow ([`crate::session::Events::emit`]),
//! tracks who is in your channel, maps events to a [`Sound`] and, if the per-event toggle in
//! the settings allows it, writes the embedded 48 kHz mono samples into the mixer's mix-in
//! port. Sounds only play while audio is open (a voice pipeline attached its port) and not
//! while you are deafened, except the undeafen sound itself.
//!
//! Self mute/deafen sounds follow the server's echo of your own user state, so they match
//! what the server (and everyone else) sees. New-message sounds are requested by the chat
//! code through [`SoundHub::play`] with [`Sound::Message`], or [`Sound::Mention`] when the
//! message mentions the user (the UI owns the matching rules and reports the result).
//!
//! The own join and hang-up sounds follow the user's action: the session code calls
//! [`SoundHub::voice_join`] for an explicit join and [`SoundHub::hang_up`] before it tears
//! down voice for a user-initiated disconnect. A join that moves to another channel sounds only
//! once the server confirms the move, so a refused join (e.g. a missing channel password) stays
//! silent. Automatic arrivals (read-only landing, reconnect
//! restore) and quitting never call them.
//!
//! Quitting while in a voice channel has its own sound (#325): the engine's shutdown asks
//! [`SoundHub::quit`] for it and waits at most [`QUIT_WAIT_MAX`] (see [`quit_wait`]) before the
//! app exits.
//!
//! Being kicked or banned (#205) has its own sounds: the session code calls
//! [`SoundHub::removed`] when the server removes the own session, before voice is torn down.
//! A plain disconnect or a server shutdown removes nobody and stays silent.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mumcord_store::{SoundSettings, Store};

use crate::session::EngineEvent;

/// Mix-in gain of the sounds on top of their already low level.
pub const SOUND_GAIN: f32 = 1.0;

/// A notification sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sound {
    UserJoined,
    UserLeft,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    /// New message in a followed channel or private chat.
    Message,
    /// A message mentions you; plays in place of [`Sound::Message`] (QA.41).
    Mention,
    /// You join a channel for voice yourself.
    VoiceJoin,
    /// You disconnect from a server yourself.
    HangUp,
    /// You quit Mumcord while in a voice channel (#325).
    Quit,
    /// Someone starts a watch party in your channel (FR-2.77).
    PartyStarted,
    PartyJoined,
    PartyLeft,
    /// Someone starts a screen share in your channel (FR-2.77).
    ShareStarted,
    ShareStopped,
    /// Someone starts watching your screen share (FR-2.77).
    ViewerStarted,
    ViewerStopped,
    /// The server kicks you (#205).
    Kicked,
    /// The server bans you (#205).
    Banned,
}

/// Longest the quit waits for its sound (#325), so quitting never feels stuck.
pub const QUIT_WAIT_MAX: Duration = Duration::from_millis(1500);

/// Extra time after the quit sound's last sample, so the device plays out its buffer.
const QUIT_WAIT_MARGIN: Duration = Duration::from_millis(150);

/// How long the quit waits for a quit sound of `samples` 48 kHz samples: the sound plus a short
/// margin for the device buffer, never more than [`QUIT_WAIT_MAX`]. No sound, no wait.
pub fn quit_wait(samples: usize) -> Duration {
    if samples == 0 {
        return Duration::ZERO;
    }
    let sound = Duration::from_secs_f64(samples as f64 / 48_000.0);
    (sound + QUIT_WAIT_MARGIN).min(QUIT_WAIT_MAX)
}

/// At most one joining or leaving activity sound per this span (FR-2.77).
const ACTIVITY_GAP: Duration = Duration::from_secs(1);

impl Sound {
    fn wav(self) -> &'static [u8] {
        match self {
            Self::UserJoined => include_bytes!("../../../assets/sounds/user_joined.wav"),
            Self::UserLeft => include_bytes!("../../../assets/sounds/user_left.wav"),
            Self::Mute => include_bytes!("../../../assets/sounds/mute.wav"),
            Self::Unmute => include_bytes!("../../../assets/sounds/unmute.wav"),
            Self::Deafen => include_bytes!("../../../assets/sounds/deafen.wav"),
            Self::Undeafen => include_bytes!("../../../assets/sounds/undeafen.wav"),
            Self::Message => include_bytes!("../../../assets/sounds/message.wav"),
            Self::Mention => include_bytes!("../../../assets/sounds/mention.wav"),
            Self::VoiceJoin => include_bytes!("../../../assets/sounds/voice_join.wav"),
            Self::HangUp | Self::ShareStopped => {
                include_bytes!("../../../assets/sounds/hang_up.wav")
            }
            // Activity sounds reuse the bundled ones: starts sound like the own join,
            // coming and going like people joining and leaving the channel.
            Self::PartyStarted | Self::ShareStarted => {
                include_bytes!("../../../assets/sounds/voice_join.wav")
            }
            Self::PartyJoined | Self::ViewerStarted => {
                include_bytes!("../../../assets/sounds/user_joined.wav")
            }
            Self::PartyLeft | Self::ViewerStopped => {
                include_bytes!("../../../assets/sounds/user_left.wav")
            }
            Self::Quit => include_bytes!("../../../assets/sounds/quit.wav"),
            Self::Kicked => include_bytes!("../../../assets/sounds/kicked.wav"),
            Self::Banned => include_bytes!("../../../assets/sounds/banned.wav"),
        }
    }

    /// Whether the settings enable this sound.
    pub fn enabled(self, s: &SoundSettings) -> bool {
        match self {
            Self::UserJoined => s.user_joined,
            Self::UserLeft => s.user_left,
            Self::Mute | Self::Unmute => s.self_mute,
            Self::Deafen | Self::Undeafen => s.self_deafen,
            Self::Message => s.new_message,
            Self::Mention => s.mention,
            Self::VoiceJoin => s.voice_join,
            Self::HangUp => s.hang_up,
            Self::Quit => s.quit,
            Self::PartyStarted => s.party_started,
            Self::PartyJoined => s.party_joined,
            Self::PartyLeft => s.party_left,
            Self::ShareStarted => s.share_started,
            Self::ShareStopped => s.share_stopped,
            Self::ViewerStarted => s.viewer_started,
            Self::ViewerStopped => s.viewer_stopped,
            Self::Kicked => s.kicked,
            Self::Banned => s.banned,
        }
    }

    /// The sound for the server removing the own session: a ban, or a kick by someone.
    /// A removal without an actor and without a ban is no kick and stays silent.
    pub fn for_removal(actor: Option<u32>, ban: bool) -> Option<Self> {
        if ban {
            Some(Self::Banned)
        } else {
            actor.map(|_| Self::Kicked)
        }
    }

    /// Whether a burst of people coming and going may only sound once per [`ACTIVITY_GAP`].
    fn rate_limited(self) -> bool {
        matches!(
            self,
            Self::PartyJoined | Self::PartyLeft | Self::ViewerStarted | Self::ViewerStopped
        )
    }

    /// 48 kHz mono samples of the sound.
    pub fn samples(self) -> Vec<f32> {
        decode_wav(self.wav())
    }

    /// Samples with the mix-in gain applied.
    fn mixed(self) -> Vec<f32> {
        self.samples().into_iter().map(|s| s * SOUND_GAIN).collect()
    }

    /// A notification about messages or activity, which Do Not Disturb silences (FR-3.111).
    /// Voice presence (people joining or leaving) and feedback for the user's own actions
    /// keep playing: voice keeps working during Do Not Disturb.
    fn notification(self) -> bool {
        matches!(
            self,
            Self::Message
                | Self::Mention
                | Self::PartyStarted
                | Self::PartyJoined
                | Self::PartyLeft
                | Self::ShareStarted
                | Self::ShareStopped
                | Self::ViewerStarted
                | Self::ViewerStopped
        )
    }

    /// Feedback for the user's own action, so it plays even while deafened.
    fn own_action(self) -> bool {
        matches!(self, Self::Undeafen | Self::VoiceJoin | Self::HangUp)
    }
}

/// Decodes the embedded 16-bit mono WAV files (RIFF chunks as written by ffmpeg); anything
/// unexpected yields silence-free empty output.
fn decode_wav(bytes: &[u8]) -> Vec<f32> {
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body = pos + 8;
        if &bytes[pos..pos + 4] == b"data" {
            let end = (body + size).min(bytes.len());
            return bytes[body..end]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| f32::from(i16::from_le_bytes(*c)) / 32768.0)
                .collect();
        }
        pos = body + size + (size & 1);
    }
    Vec::new()
}

#[derive(Clone, Copy, Default)]
struct Known {
    channel: u32,
    mute: bool,
    deaf: bool,
}

/// The own mute/deafen state an explicit join is heading for. The server echoes the join's
/// voice change in up to two steps (undeafen, then unmute); their cues are replaced by the
/// join sound.
#[derive(Clone, Copy)]
struct Joining {
    mute: bool,
    deaf: bool,
    echoes_left: u8,
}

/// Follows users and the own state to turn events into sounds.
#[derive(Default)]
struct Tracker {
    own: Option<u32>,
    users: HashMap<u32, Known>,
    joining: Option<Joining>,
    /// The watch party in the own channel: who takes part.
    party: Option<BTreeSet<u32>>,
    /// The screen shares in the own channel: sharer -> viewers.
    shares: HashMap<u32, BTreeSet<u32>>,
}

impl Tracker {
    fn own_channel(&self) -> Option<u32> {
        self.own.and_then(|s| self.users.get(&s)).map(|k| k.channel)
    }

    fn own_known(&self) -> Option<Known> {
        self.own.and_then(|s| self.users.get(&s)).copied()
    }

    fn own_deaf(&self) -> bool {
        self.own_known().is_some_and(|k| k.deaf)
    }

    /// An explicit join to `mute`/`deaf` was requested: the echoes of that change stay silent.
    fn expect_join(&mut self, mute: bool, deaf: bool) {
        let unchanged = self
            .own_known()
            .is_some_and(|k| k.mute == mute && k.deaf == deaf);
        self.joining = (!unchanged).then_some(Joining {
            mute,
            deaf,
            echoes_left: 2,
        });
    }

    /// Whether an own voice change to `mute`/`deaf` is an echo of the pending join.
    fn join_echo(&mut self, mute: bool, deaf: bool) -> bool {
        let Some(mut joining) = self.joining.take() else {
            return false;
        };
        joining.echoes_left -= 1;
        let reached = joining.mute == mute && joining.deaf == deaf;
        if !reached && joining.echoes_left > 0 {
            self.joining = Some(joining);
        }
        true
    }

    /// Watch party and screen share changes of other people (FR-2.77) as sounds. The user's
    /// own actions stay silent, and so does leaving for another channel.
    fn activity(&mut self, event: &EngineEvent) -> Vec<Sound> {
        let own = self.own;
        let mut sounds = Vec::new();
        match event {
            EngineEvent::ServerSynced(_) => {
                self.party = None;
                self.shares.clear();
            }
            EngineEvent::UserUpserted(u) if Some(u.session) == own => {
                let moved = self
                    .users
                    .get(&u.session)
                    .is_some_and(|k| k.channel != u.channel_id);
                if moved {
                    self.party = None;
                    self.shares.clear();
                }
            }
            EngineEvent::WatchPartyChanged(view) => {
                let next = view
                    .as_ref()
                    .map(|v| v.participants.iter().copied().collect::<BTreeSet<_>>());
                let prev = std::mem::replace(&mut self.party, next.clone());
                match (prev, next, view) {
                    (None, Some(now), Some(v)) => {
                        if !v.joined && own.is_none_or(|o| !now.contains(&o)) {
                            sounds.push(Sound::PartyStarted);
                        }
                    }
                    (Some(before), Some(now), _) => {
                        let others = |set: &BTreeSet<u32>, other: &BTreeSet<u32>| {
                            set.difference(other).filter(|s| Some(**s) != own).count()
                        };
                        sounds.extend((0..others(&now, &before)).map(|_| Sound::PartyJoined));
                        sounds.extend((0..others(&before, &now)).map(|_| Sound::PartyLeft));
                    }
                    _ => {}
                }
            }
            EngineEvent::ScreenSharesChanged(views) => {
                let next: HashMap<u32, BTreeSet<u32>> = views
                    .iter()
                    .map(|v| (v.session, v.viewers.iter().copied().collect()))
                    .collect();
                let prev = std::mem::replace(&mut self.shares, next.clone());
                for sharer in next.keys().filter(|s| Some(**s) != own) {
                    if !prev.contains_key(sharer) {
                        sounds.push(Sound::ShareStarted);
                    }
                }
                for sharer in prev.keys().filter(|s| Some(**s) != own) {
                    if !next.contains_key(sharer) {
                        sounds.push(Sound::ShareStopped);
                    }
                }
                if let Some(o) = own
                    && let (Some(before), Some(now)) = (prev.get(&o), next.get(&o))
                {
                    sounds.extend(now.difference(before).map(|_| Sound::ViewerStarted));
                    sounds.extend(before.difference(now).map(|_| Sound::ViewerStopped));
                }
            }
            _ => {}
        }
        sounds
    }

    /// Updates the state from `event`; returns the sound it calls for and whether the own
    /// user was deafened before it.
    fn observe(&mut self, event: &EngineEvent) -> Option<(Sound, bool)> {
        match event {
            EngineEvent::ServerSynced(snapshot) => {
                self.own = Some(snapshot.self_session);
                self.joining = None;
                self.users = snapshot
                    .users
                    .iter()
                    .map(|u| {
                        let known = Known {
                            channel: u.channel_id,
                            mute: u.self_mute,
                            deaf: u.self_deaf,
                        };
                        (u.session, known)
                    })
                    .collect();
                None
            }
            EngineEvent::UserUpserted(u) => {
                let was_deaf = self.own_deaf();
                let own_channel = self.own_channel();
                let before = self.users.insert(
                    u.session,
                    Known {
                        channel: u.channel_id,
                        mute: u.self_mute,
                        deaf: u.self_deaf,
                    },
                );
                if Some(u.session) == self.own {
                    let before = before?;
                    let sound = if before.deaf != u.self_deaf {
                        if u.self_deaf {
                            Sound::Deafen
                        } else {
                            Sound::Undeafen
                        }
                    } else if before.mute != u.self_mute {
                        if u.self_mute {
                            Sound::Mute
                        } else {
                            Sound::Unmute
                        }
                    } else {
                        return None;
                    };
                    if self.join_echo(u.self_mute, u.self_deaf) {
                        return None;
                    }
                    return Some((sound, was_deaf));
                }
                let own_channel = own_channel?;
                let was_here = before.is_some_and(|k| k.channel == own_channel);
                let is_here = u.channel_id == own_channel;
                match (was_here, is_here) {
                    (false, true) => Some((Sound::UserJoined, was_deaf)),
                    (true, false) => Some((Sound::UserLeft, was_deaf)),
                    _ => None,
                }
            }
            EngineEvent::UserRemoved(session) => {
                let was_deaf = self.own_deaf();
                let own_channel = self.own_channel();
                let removed = self.users.remove(session)?;
                (Some(*session) != self.own && Some(removed.channel) == own_channel)
                    .then_some((Sound::UserLeft, was_deaf))
            }
            _ => None,
        }
    }
}

/// Where samples go: the mixer's mix-in port. Returns `false` if there was no room.
pub type SampleSink = Box<dyn FnMut(&[f32]) -> bool + Send>;

struct State {
    tracker: Tracker,
    out: Option<SampleSink>,
    /// The own join sound, waiting for audio that is still starting (link join on connect).
    pending_join: bool,
    /// The channel an explicit join moves to; its sound waits for the server's confirmation.
    awaiting_move: Option<u32>,
    /// When the last rate-limited activity sound was due.
    last_burst: Option<Instant>,
}

/// Maps engine events to sounds and plays them. Cheap to share.
pub struct SoundHub {
    settings: Box<dyn Fn() -> SoundSettings + Send + Sync>,
    state: Mutex<State>,
    /// Do Not Disturb is in effect (manual switch or the followed system state).
    do_not_disturb: AtomicBool,
}

impl SoundHub {
    /// `settings` returns the current per-event toggles.
    pub fn new(settings: impl Fn() -> SoundSettings + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            settings: Box::new(settings),
            state: Mutex::new(State {
                tracker: Tracker::default(),
                out: None,
                pending_join: false,
                awaiting_move: None,
                last_burst: None,
            }),
            do_not_disturb: AtomicBool::new(false),
        })
    }

    /// A hub reading the toggles from the store. Emitting can happen while the store is
    /// locked on the same thread; then the last read value is used instead of blocking.
    pub fn from_store(store: Arc<Mutex<Store>>) -> Arc<Self> {
        let cache = Mutex::new(SoundSettings::default());
        Self::new(move || {
            let mut cached = cache.lock().unwrap_or_else(PoisonError::into_inner);
            if let Ok(store) = store.try_lock()
                && let Ok(settings) = store.settings()
            {
                *cached = settings.sounds;
            }
            cached.clone()
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Do Not Disturb turned on or off: notification sounds stay silent while it is on.
    pub fn set_do_not_disturb(&self, on: bool) {
        self.do_not_disturb.store(on, Ordering::Relaxed);
    }

    fn silenced(&self, sound: Sound) -> bool {
        sound.notification() && self.do_not_disturb.load(Ordering::Relaxed)
    }

    /// Audio is open: sounds go to `out` from now on.
    pub fn attach(&self, out: SampleSink) {
        let mut state = self.state();
        state.out = Some(out);
        if std::mem::take(&mut state.pending_join) {
            self.play_locked(&mut state, Sound::VoiceJoin, false);
        }
    }

    /// Audio closed: no sounds until the next [`attach`](Self::attach).
    pub fn detach(&self) {
        self.state().out = None;
    }

    /// Feeds one engine event (called for every emitted event).
    pub fn observe(&self, event: &EngineEvent) {
        let mut state = self.state();
        if matches!(event, EngineEvent::ServerSynced(_)) {
            state.pending_join = false;
            state.awaiting_move = None;
        }
        // The activity diff runs first: it must see the user's own channel before the
        // channel tracking below moves on.
        let activity = state.tracker.activity(event);
        if let Some((sound, was_deaf)) = state.tracker.observe(event) {
            self.play_locked(&mut state, sound, was_deaf);
        }
        if state.awaiting_move.is_some() && state.awaiting_move == state.tracker.own_channel() {
            state.awaiting_move = None;
            self.join_sound(&mut state);
        }
        let deaf = state.tracker.own_deaf();
        for sound in activity {
            if sound.rate_limited() && !self.burst_allowed(&mut state, sound) {
                continue;
            }
            self.play_locked(&mut state, sound, deaf);
        }
    }

    /// Plays `sound` now (for events the hub cannot see, such as [`Sound::Message`]),
    /// subject to the toggles and to deafness.
    pub fn play(&self, sound: Sound) {
        let mut state = self.state();
        let deaf = state.tracker.own_deaf();
        self.play_locked(&mut state, sound, deaf);
    }

    /// Plays `sound` now whatever the toggles and the deaf state say: the "Test sound" button
    /// of the output popover must be audible so the user can judge the chosen device and volume.
    pub fn play_test(&self, sound: Sound) {
        let mut state = self.state();
        if let Some(out) = state.out.as_mut()
            && !out(&sound.mixed())
        {
            tracing::debug!("test sound skipped: mixer input is full");
        }
    }

    /// The user joins a channel for voice themselves (QA.23); `mute`/`deaf` is the own state
    /// the join turns on. Plays the join sound in place of the mute/deafen cues the server's
    /// echo of that change would cause. With a `target` channel to move to, the sound waits
    /// until the server put the user there; if audio is still starting, it plays once it is up.
    pub fn voice_join(&self, target: Option<u32>, mute: bool, deaf: bool) {
        let mut state = self.state();
        state.tracker.expect_join(mute, deaf);
        state.awaiting_move = target.filter(|t| state.tracker.own_channel() != Some(*t));
        if state.awaiting_move.is_none() {
            self.join_sound(&mut state);
        }
    }

    fn join_sound(&self, state: &mut State) {
        if state.out.is_some() {
            self.play_locked(state, Sound::VoiceJoin, false);
        } else {
            state.pending_join = true;
        }
    }

    /// The user is about to end the connection themselves (QA.19). Returns the hang-up samples
    /// (gain applied) if voice is on and the toggle allows it. The caller plays them on a path
    /// that outlives the voice pipeline, which is torn down right after.
    pub fn hang_up(&self) -> Option<Vec<f32>> {
        let mut state = self.state();
        state.pending_join = false;
        state.awaiting_move = None;
        state.tracker.joining = None;
        (state.out.is_some() && Sound::HangUp.enabled(&(self.settings)()))
            .then(|| Sound::HangUp.mixed())
    }

    /// The user quits Mumcord (#325). Returns the quit samples (gain applied) if `in_voice` (a
    /// server carries voice, not just read-only), audio is open, the user is not deafened and
    /// the toggle allows it. Muting does not matter: it only concerns the microphone. Like
    /// [`hang_up`](Self::hang_up), the caller plays them on a path that outlives the voice
    /// pipeline.
    pub fn quit(&self, in_voice: bool) -> Option<Vec<f32>> {
        let state = self.state();
        (in_voice
            && state.out.is_some()
            && !state.tracker.own_deaf()
            && Sound::Quit.enabled(&(self.settings)()))
        .then(|| Sound::Quit.mixed())
    }

    /// The server removed the own session (#205). Returns the kick or ban samples (gain
    /// applied) if voice is on, the user is not deafened and the toggle allows it. Like
    /// [`hang_up`](Self::hang_up), the caller plays them on a path that outlives the voice
    /// pipeline.
    pub fn removed(&self, actor: Option<u32>, ban: bool) -> Option<Vec<f32>> {
        let mut state = self.state();
        state.pending_join = false;
        state.awaiting_move = None;
        state.tracker.joining = None;
        let sound = Sound::for_removal(actor, ban)?;
        (state.out.is_some() && !state.tracker.own_deaf() && sound.enabled(&(self.settings)()))
            .then(|| sound.mixed())
    }

    /// Whether a rate-limited `sound` is not too close to the last one. A sound that would
    /// stay silent anyway does not use up the gap.
    fn burst_allowed(&self, state: &mut State, sound: Sound) -> bool {
        if !sound.enabled(&(self.settings)()) || state.tracker.own_deaf() || self.silenced(sound) {
            return true;
        }
        let now = Instant::now();
        if state
            .last_burst
            .is_some_and(|last| now.duration_since(last) < ACTIVITY_GAP)
        {
            return false;
        }
        state.last_burst = Some(now);
        true
    }

    fn play_locked(&self, state: &mut State, sound: Sound, was_deaf: bool) {
        if was_deaf && !sound.own_action() {
            return;
        }
        if !sound.enabled(&(self.settings)()) || self.silenced(sound) {
            return;
        }
        if let Some(out) = state.out.as_mut()
            && !out(&sound.mixed())
        {
            tracing::debug!("notification sound skipped: mixer input is full");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::session::{Snapshot, UserView};
    use mumcord_audio::mixer::{Playback, PlaybackConfig, PlaybackSink};
    use mumcord_audio::{FRAME_SAMPLES, Frame};

    const ALL: [Sound; 20] = [
        Sound::UserJoined,
        Sound::UserLeft,
        Sound::Mute,
        Sound::Unmute,
        Sound::Deafen,
        Sound::Undeafen,
        Sound::Message,
        Sound::Mention,
        Sound::VoiceJoin,
        Sound::HangUp,
        Sound::Quit,
        Sound::PartyStarted,
        Sound::PartyJoined,
        Sound::PartyLeft,
        Sound::ShareStarted,
        Sound::ShareStopped,
        Sound::ViewerStarted,
        Sound::ViewerStopped,
        Sound::Kicked,
        Sound::Banned,
    ];

    fn user(session: u32, channel_id: u32, mute: bool, deaf: bool) -> UserView {
        UserView {
            session,
            channel_id,
            name: format!("u{session}"),
            cert_hash: None,
            registered: false,
            self_mute: mute,
            self_deaf: deaf,
            server_mute: false,
            server_deaf: false,
            suppressed: false,
            listening_channel_ids: Vec::new(),
            has_avatar: false,
            avatar_hash: None,
            has_comment: false,
            volume: 100,
            local_mute: false,
            mumcord_features: None,
        }
    }

    fn synced(users: Vec<UserView>) -> EngineEvent {
        EngineEvent::ServerSynced(Box::new(Snapshot {
            server_id: 0,
            self_session: 1,
            info: crate::session::ServerInfoView {
                version: String::new(),
                release: None,
                os: None,
                follow_supported: true,
                welcome_text: None,
                default_channel_id: None,
                max_message_length: 5000,
                max_image_message_length: 0,
                allow_html: false,
                max_bandwidth: None,
                max_users: None,
                recording_allowed: false,
                server_label: None,
                server_address: None,
                server_port: None,
                tls_fingerprint: None,
                users_online: 0,
                session_id: None,
                registered: false,
                current_channel_permissions: None,
                mumcord_users: 0,
                tls_trusted: true,
                tls_issuer: None,
                tls_valid_from: None,
                tls_valid_until: None,
                tls_certificate_pem: None,
            },
            channels: Vec::new(),
            users,
            followed_channel_ids: Vec::new(),
            unread: Vec::new(),
            read_only: false,
            rejoin_channel_id: None,
        }))
    }

    fn tracker() -> Tracker {
        let mut t = Tracker::default();
        t.observe(&synced(vec![
            user(1, 10, false, false),
            user(2, 10, false, false),
        ]));
        t
    }

    fn sound(t: &mut Tracker, e: EngineEvent) -> Option<Sound> {
        t.observe(&e).map(|(s, _)| s)
    }

    #[test]
    fn event_to_sound_mapping() {
        let mut t = tracker();
        let up = EngineEvent::UserUpserted;
        assert_eq!(
            sound(&mut t, up(user(3, 10, false, false))),
            Some(Sound::UserJoined)
        );
        assert_eq!(sound(&mut t, up(user(3, 10, true, false))), None);
        assert_eq!(
            sound(&mut t, up(user(3, 11, false, false))),
            Some(Sound::UserLeft)
        );
        assert_eq!(sound(&mut t, up(user(4, 11, false, false))), None);
        assert_eq!(sound(&mut t, EngineEvent::UserRemoved(4)), None);
        assert_eq!(
            sound(&mut t, EngineEvent::UserRemoved(2)),
            Some(Sound::UserLeft)
        );
        assert_eq!(
            sound(&mut t, up(user(1, 10, true, false))),
            Some(Sound::Mute)
        );
        assert_eq!(
            sound(&mut t, up(user(1, 10, false, false))),
            Some(Sound::Unmute)
        );
        assert_eq!(
            sound(&mut t, up(user(1, 10, true, true))),
            Some(Sound::Deafen)
        );
        assert_eq!(
            sound(&mut t, up(user(1, 10, true, false))),
            Some(Sound::Undeafen)
        );
        // Own channel change: no sound, and the new channel is what others are compared to.
        assert_eq!(sound(&mut t, up(user(1, 12, true, false))), None);
        assert_eq!(
            sound(&mut t, up(user(5, 12, false, false))),
            Some(Sound::UserJoined)
        );
    }

    #[test]
    fn toggles_gate_each_sound() {
        let off = SoundSettings {
            user_joined: false,
            user_left: false,
            self_mute: false,
            self_deafen: false,
            new_message: false,
            mention: false,
            mention_notify: false,
            voice_join: false,
            hang_up: false,
            quit: false,
            party_started: false,
            party_joined: false,
            party_left: false,
            share_started: false,
            share_stopped: false,
            viewer_started: false,
            viewer_stopped: false,
            kicked: false,
            banned: false,
        };
        for s in ALL {
            assert!(s.enabled(&SoundSettings::default()), "{s:?}");
            assert!(!s.enabled(&off), "{s:?}");
        }
        let only_mute = SoundSettings {
            self_mute: true,
            ..off
        };
        assert!(Sound::Mute.enabled(&only_mute) && Sound::Unmute.enabled(&only_mute));
        assert!(!Sound::Deafen.enabled(&only_mute));
    }

    fn hub_with_log(settings: SoundSettings) -> (Arc<SoundHub>, Arc<Mutex<Vec<usize>>>) {
        let hub = SoundHub::new(move || settings.clone());
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = log.clone();
        hub.attach(Box::new(move |s| {
            l.lock().unwrap().push(s.len());
            true
        }));
        (hub, log)
    }

    #[test]
    fn test_sound_plays_even_when_deafened_and_toggles_are_off() {
        let (hub, log) = hub_with_log(SoundSettings {
            new_message: false,
            ..SoundSettings::default()
        });
        hub.observe(&synced(vec![user(1, 10, true, true)]));
        hub.play(Sound::Message);
        assert!(log.lock().unwrap().is_empty());
        hub.play_test(Sound::Message);
        assert_eq!(log.lock().unwrap().len(), 1);
    }

    #[test]
    fn deafened_suppresses_all_but_undeafen() {
        let (hub, log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![
            user(1, 10, false, false),
            user(2, 10, false, false),
        ]));
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, true)));
        assert_eq!(log.lock().unwrap().len(), 1, "deafen cue itself plays");
        hub.observe(&EngineEvent::UserUpserted(user(3, 10, false, false)));
        hub.observe(&EngineEvent::UserRemoved(2));
        hub.play(Sound::Message);
        assert_eq!(log.lock().unwrap().len(), 1, "nothing while deafened");
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, false)));
        assert_eq!(log.lock().unwrap().len(), 2, "undeafen plays");
        hub.play(Sound::Message);
        assert_eq!(log.lock().unwrap().len(), 3);
    }

    #[test]
    fn do_not_disturb_silences_notifications_but_not_voice_or_own_actions() {
        let (hub, log) = hub_with_log(SoundSettings::default());
        hub.set_do_not_disturb(true);
        for sound in ALL {
            log.lock().unwrap().clear();
            hub.play(sound);
            let played = !log.lock().unwrap().is_empty();
            assert_eq!(played, !sound.notification(), "{sound:?}");
        }
        assert!(!Sound::UserJoined.notification() && !Sound::Mute.notification());
        assert!(Sound::Mention.notification() && Sound::ShareStarted.notification());
        log.lock().unwrap().clear();
        hub.set_do_not_disturb(false);
        hub.play(Sound::Mention);
        assert_eq!(log.lock().unwrap().len(), 1, "plays again once it is off");
    }

    #[test]
    fn nothing_without_audio_or_with_toggle_off() {
        let (hub, log) = hub_with_log(SoundSettings {
            self_mute: false,
            ..SoundSettings::default()
        });
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, false)));
        assert!(log.lock().unwrap().is_empty());
        hub.detach();
        hub.observe(&EngineEvent::UserUpserted(user(2, 10, false, false)));
        assert!(log.lock().unwrap().is_empty());
    }

    fn peak_dbfs(samples: &[f32]) -> f32 {
        let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        20.0 * peak.log10()
    }

    #[test]
    fn wavs_decode_short_and_quiet() {
        for s in ALL {
            let wav = s.wav();
            assert_eq!(&wav[..4], b"RIFF");
            // 1 channel, 48 kHz, 16 bit.
            assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
            assert_eq!(
                u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
                48_000
            );
            assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
            let samples = s.samples();
            assert!(samples.len() > 48_00, "{s:?} too short");
            // The kick, ban and quit sounds are at most 1 s long; the connect sound (also used for the party and share starts)
            // is a 1.2 s chime and the new-message sound a 1 s one, the mention 0.7 s, the people joining and leaving 0.8 s and the hang up (also the share stop) 0.9 s; every other cue stays short.
            let max_ms = match s {
                Sound::Banned | Sound::Kicked | Sound::Quit => 1000,
                Sound::VoiceJoin | Sound::PartyStarted | Sound::ShareStarted => 1200,
                Sound::Message => 1000,
                Sound::Mention => 700,
                Sound::UserJoined
                | Sound::UserLeft
                | Sound::PartyJoined
                | Sound::PartyLeft
                | Sound::ViewerStarted
                | Sound::ViewerStopped => 800,
                Sound::HangUp | Sound::ShareStopped => 900,
                _ => 400,
            };
            assert!(
                samples.len() <= 48_000 * max_ms / 1000,
                "{s:?} longer than {max_ms} ms"
            );
            assert!(peak_dbfs(&samples) <= -12.0, "{s:?} too loud");
            assert!(peak_dbfs(&samples) > -40.0, "{s:?} inaudible");
        }
    }

    /// Lengths of the played sounds; compared with [`lens`] of the expected ones. Paired
    /// sounds (mute/unmute …) share a length, the join and hang-up sounds have their own.
    fn played(log: &Mutex<Vec<usize>>) -> Vec<usize> {
        log.lock().unwrap().clone()
    }

    fn lens(sounds: &[Sound]) -> Vec<usize> {
        sounds.iter().map(|s| s.samples().len()).collect()
    }

    #[test]
    fn own_and_join_sounds_are_distinct() {
        let len = |s: Sound| s.samples().len();
        for s in [
            Sound::UserJoined,
            Sound::Mute,
            Sound::Deafen,
            Sound::Message,
            Sound::HangUp,
        ] {
            assert_ne!(len(s), len(Sound::VoiceJoin), "{s:?}");
        }
        for s in [
            Sound::UserJoined,
            Sound::Mute,
            Sound::Deafen,
            Sound::Message,
        ] {
            assert_ne!(len(s), len(Sound::HangUp), "{s:?}");
        }
    }

    /// Joining from the read-only landing: the join sound plays although still deafened, and
    /// replaces the undeafen and unmute cues of the server's two-step echo.
    #[test]
    fn join_from_read_only_plays_join_instead_of_echo_cues() {
        let (hub, log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![
            user(1, 10, true, true),
            user(2, 11, false, false),
        ]));
        hub.voice_join(Some(11), false, false);
        assert!(played(&log).is_empty());
        hub.observe(&EngineEvent::UserUpserted(user(1, 11, true, true)));
        assert_eq!(played(&log), lens(&[Sound::VoiceJoin]));
        hub.observe(&EngineEvent::UserUpserted(user(1, 11, true, false)));
        hub.observe(&EngineEvent::UserUpserted(user(1, 11, false, false)));
        assert_eq!(
            played(&log),
            lens(&[Sound::VoiceJoin]),
            "echo cues replaced"
        );
        hub.observe(&EngineEvent::UserUpserted(user(1, 11, true, false)));
        assert_eq!(
            played(&log),
            lens(&[Sound::VoiceJoin, Sound::Mute]),
            "later own changes sound again"
        );
    }

    /// The join sound follows the server's confirmation of the move: a refused join (e.g. a
    /// channel password is missing, FR-2.78) stays silent, a confirmed one plays it once.
    #[test]
    fn join_sound_waits_for_the_confirmed_move() {
        let (hub, log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.voice_join(Some(12), false, false);
        assert!(played(&log).is_empty(), "not before the server confirms");
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, false, false)));
        assert!(played(&log).is_empty(), "refused: still outside");

        hub.voice_join(Some(12), false, false);
        hub.observe(&EngineEvent::UserUpserted(user(1, 12, false, false)));
        hub.observe(&EngineEvent::UserUpserted(user(1, 12, false, false)));
        assert_eq!(played(&log), lens(&[Sound::VoiceJoin]), "once on arrival");
    }

    #[test]
    fn a_new_connection_drops_an_unconfirmed_join() {
        let (hub, log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.voice_join(Some(12), false, false);
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.observe(&EngineEvent::UserUpserted(user(1, 12, false, false)));
        assert!(played(&log).is_empty());
    }

    #[test]
    fn join_without_voice_change_keeps_later_cues() {
        let (hub, log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.voice_join(Some(12), false, false);
        hub.observe(&EngineEvent::UserUpserted(user(1, 12, false, false)));
        hub.observe(&EngineEvent::UserUpserted(user(1, 12, true, false)));
        assert_eq!(played(&log), lens(&[Sound::VoiceJoin, Sound::Mute]));
    }

    #[test]
    fn unreached_join_stops_swallowing_after_two_echoes() {
        let (hub, log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![user(1, 10, true, true)]));
        hub.voice_join(None, false, false);
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, false)));
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, true)));
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, false)));
        assert_eq!(played(&log), lens(&[Sound::VoiceJoin, Sound::Undeafen]));
    }

    /// Arrivals the user did not ask for never play the join sound: the read-only landing of a
    /// fresh connect and the restore of channel and mute state after a reconnect.
    #[test]
    fn no_join_sound_for_read_only_landing_or_reconnect_restore() {
        let (hub, log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, true)));
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.observe(&EngineEvent::UserUpserted(user(1, 14, false, false)));
        hub.observe(&EngineEvent::UserUpserted(user(1, 14, true, false)));
        assert!(!played(&log).contains(&Sound::VoiceJoin.samples().len()));
    }

    #[test]
    fn join_sound_waits_for_starting_audio() {
        let hub = SoundHub::new(SoundSettings::default);
        let log = Arc::new(Mutex::new(Vec::new()));
        let attach = |hub: &SoundHub| {
            let l = log.clone();
            hub.attach(Box::new(move |s| {
                l.lock().unwrap().push(s.len());
                true
            }));
        };
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.voice_join(None, true, false);
        attach(&hub);
        assert_eq!(
            played(&log),
            lens(&[Sound::VoiceJoin]),
            "plays once audio is up"
        );
        hub.detach();
        attach(&hub);
        assert_eq!(played(&log), lens(&[Sound::VoiceJoin]), "only once");

        hub.detach();
        hub.voice_join(None, false, false);
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        attach(&hub);
        assert_eq!(
            played(&log),
            lens(&[Sound::VoiceJoin]),
            "a new connection drops a stale join"
        );
    }

    #[test]
    fn join_sound_toggle() {
        let (hub, log) = hub_with_log(SoundSettings {
            voice_join: false,
            ..SoundSettings::default()
        });
        hub.observe(&synced(vec![user(1, 10, true, true)]));
        hub.voice_join(None, false, false);
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, false)));
        assert!(
            played(&log).is_empty(),
            "off: no join sound, echo still replaced"
        );
    }

    #[test]
    fn hang_up_needs_voice_and_toggle() {
        let (hub, _log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![user(1, 10, true, true)]));
        let samples = hub.hang_up().expect("voice on, toggle on, also read-only");
        assert_eq!(samples.len(), Sound::HangUp.samples().len());

        let (off, _log) = hub_with_log(SoundSettings {
            hang_up: false,
            ..SoundSettings::default()
        });
        assert!(off.hang_up().is_none(), "toggle off");

        hub.detach();
        assert!(hub.hang_up().is_none(), "no voice, nothing to hang up");
    }

    #[test]
    fn quit_sound_is_distinct() {
        let len = |s: Sound| s.samples().len();
        for s in [Sound::HangUp, Sound::UserLeft, Sound::Kicked, Sound::Banned] {
            assert_ne!(len(s), len(Sound::Quit), "{s:?}");
        }
    }

    /// Muting does not silence the quit sound (it only concerns the microphone); deafening
    /// does, like the other sounds that are not the user's own deafen action.
    #[test]
    fn quit_needs_voice_toggle_and_hearing() {
        let (hub, _log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![user(1, 10, true, false)]));
        let samples = hub.quit(true).expect("in voice, toggle on, muted");
        assert_eq!(samples.len(), Sound::Quit.samples().len());
        assert!(hub.quit(false).is_none(), "read-only, not in voice");

        let (off, _log) = hub_with_log(SoundSettings {
            quit: false,
            ..SoundSettings::default()
        });
        off.observe(&synced(vec![user(1, 10, false, false)]));
        assert!(off.quit(true).is_none(), "toggle off");

        hub.observe(&synced(vec![user(1, 10, true, true)]));
        assert!(hub.quit(true).is_none(), "deafened");

        hub.observe(&synced(vec![user(1, 10, false, false)]));
        hub.detach();
        assert!(hub.quit(true).is_none(), "no audio open");
    }

    #[test]
    fn quit_waits_for_the_sound_but_never_long() {
        assert_eq!(quit_wait(0), Duration::ZERO, "no sound, no wait");
        let quit = Sound::Quit.samples().len();
        let wait = quit_wait(quit);
        assert!(
            wait > Duration::from_secs_f64(quit as f64 / 48_000.0),
            "{wait:?}"
        );
        assert!(wait <= QUIT_WAIT_MAX);
        assert_eq!(quit_wait(48_000 * 10), QUIT_WAIT_MAX, "capped");
    }

    #[test]
    fn removal_maps_to_kick_or_ban() {
        assert_eq!(Sound::for_removal(Some(2), false), Some(Sound::Kicked));
        assert_eq!(Sound::for_removal(Some(2), true), Some(Sound::Banned));
        assert_eq!(
            Sound::for_removal(None, true),
            Some(Sound::Banned),
            "a ban sounds even without an actor"
        );
        assert_eq!(
            Sound::for_removal(None, false),
            None,
            "a removal by nobody is no kick"
        );
    }

    #[test]
    fn removed_needs_voice_toggle_and_hearing() {
        let (hub, _log) = hub_with_log(SoundSettings::default());
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        let kicked = hub.removed(Some(2), false).expect("kicked");
        assert_eq!(kicked.len(), Sound::Kicked.samples().len());
        let banned = hub.removed(Some(2), true).expect("banned");
        assert_eq!(banned.len(), Sound::Banned.samples().len());
        assert!(hub.removed(None, false).is_none(), "no kick, no sound");

        let (off, _log) = hub_with_log(SoundSettings {
            kicked: false,
            ..SoundSettings::default()
        });
        assert!(off.removed(Some(2), false).is_none(), "kick toggle off");
        assert!(off.removed(Some(2), true).is_some(), "ban toggle stays on");

        let (deaf, _log) = hub_with_log(SoundSettings::default());
        deaf.observe(&synced(vec![user(1, 10, true, true)]));
        assert!(deaf.removed(Some(2), true).is_none(), "deafened");

        hub.detach();
        assert!(hub.removed(Some(2), true).is_none(), "no voice");
    }

    #[test]
    fn hang_up_drops_a_pending_join() {
        let hub = SoundHub::new(SoundSettings::default);
        hub.voice_join(None, false, false);
        assert!(hub.hang_up().is_none());
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = log.clone();
        hub.attach(Box::new(move |s| {
            l.lock().unwrap().push(s.len());
            true
        }));
        assert!(log.lock().unwrap().is_empty());
    }

    struct Capture(Arc<Mutex<Vec<f32>>>);
    impl PlaybackSink for Capture {
        fn write_frame(&mut self, frame: &Frame) -> bool {
            self.0.lock().unwrap().extend_from_slice(frame);
            true
        }
        fn queued_samples(&mut self) -> usize {
            0
        }
    }

    /// The hub's output reaches the mixer's output: a mute toggle makes a sound appear.
    #[test]
    fn mute_toggle_is_audible_in_mixer_output() {
        let out = Arc::new(Mutex::new(Vec::new()));
        let (_playback, ports) =
            Playback::start(Capture(out.clone()), PlaybackConfig::default()).expect("playback");
        let mut mix_in = ports.mix_in;
        let hub = SoundHub::new(SoundSettings::default);
        hub.attach(Box::new(move |samples| {
            if mix_in.slots() < samples.len() {
                return false;
            }
            samples.iter().for_each(|&s| {
                let _ = mix_in.push(s);
            });
            true
        }));
        hub.observe(&synced(vec![user(1, 10, false, false)]));
        std::thread::sleep(std::time::Duration::from_millis(300));
        let silent_peak = out
            .lock()
            .unwrap()
            .iter()
            .fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(silent_peak < 1e-4, "no sound before the toggle");

        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, false)));
        std::thread::sleep(std::time::Duration::from_millis(600));
        let samples = out.lock().unwrap().clone();
        assert!(samples.len() >= FRAME_SAMPLES);
        let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05, "mute sound missing in output (peak {peak})");
        assert!(peak_dbfs(&samples) <= -12.0);
    }
    fn party(participants: &[u32], joined: bool) -> EngineEvent {
        EngineEvent::WatchPartyChanged(Some(crate::watch_party::WatchPartyView {
            channel_id: 10,
            video_id: "v".into(),
            playing: true,
            position_ms: 0,
            participants: participants.to_vec(),
            last_action: None,
            joined,
            current_entry: 0,
            playlist: Vec::new(),
            skipped_by: None,
            positions: Vec::new(),
            host: 1,
            history: Vec::new(),
        }))
    }

    fn shares(list: &[(u32, &[u32])]) -> EngineEvent {
        EngineEvent::ScreenSharesChanged(
            list.iter()
                .map(|(session, viewers)| crate::screen_share::ScreenShareView {
                    session: *session,
                    paused: false,
                    sound: false,
                    sound_available: false,
                    viewers: viewers.to_vec(),
                    blocked: None,
                    kind: None,
                    elapsed_ms: 0,
                })
                .collect(),
        )
    }

    fn activity_hub(settings: SoundSettings) -> (Arc<SoundHub>, Arc<Mutex<Vec<usize>>>) {
        let (hub, log) = hub_with_log(settings);
        hub.observe(&synced(vec![
            user(1, 10, false, false),
            user(2, 10, false, false),
            user(3, 10, false, false),
        ]));
        (hub, log)
    }

    /// Lets the rate limit pass without waiting.
    fn open_gap(hub: &SoundHub) {
        hub.state().last_burst = None;
    }

    #[test]
    fn party_events_of_others_play_sounds() {
        let (hub, log) = activity_hub(SoundSettings::default());
        hub.observe(&party(&[2], false));
        assert_eq!(played(&log), lens(&[Sound::PartyStarted]));
        log.lock().unwrap().clear();
        hub.observe(&party(&[2, 3], false));
        assert_eq!(played(&log), lens(&[Sound::PartyJoined]));
        open_gap(&hub);
        log.lock().unwrap().clear();
        hub.observe(&party(&[2], false));
        assert_eq!(played(&log), lens(&[Sound::PartyLeft]));
    }

    #[test]
    fn own_party_actions_are_silent() {
        let (hub, log) = activity_hub(SoundSettings::default());
        hub.observe(&party(&[1], true));
        hub.observe(&party(&[1, 2], true));
        assert_eq!(
            played(&log),
            lens(&[Sound::PartyJoined]),
            "only the other joining"
        );
        log.lock().unwrap().clear();
        open_gap(&hub);
        hub.observe(&party(&[2], false));
        assert!(played(&log).is_empty(), "own leave is silent");
        hub.observe(&party(&[1, 2], true));
        assert!(played(&log).is_empty(), "own join is silent");
    }

    #[test]
    fn share_events_play_sounds() {
        let (hub, log) = activity_hub(SoundSettings::default());
        hub.observe(&shares(&[(2, &[])]));
        assert_eq!(played(&log), lens(&[Sound::ShareStarted]));
        log.lock().unwrap().clear();
        hub.observe(&shares(&[]));
        assert_eq!(played(&log), lens(&[Sound::ShareStopped]));
    }

    #[test]
    fn own_share_and_its_viewers() {
        let (hub, log) = activity_hub(SoundSettings::default());
        hub.observe(&shares(&[(1, &[])]));
        assert!(played(&log).is_empty(), "own share start is silent");
        hub.observe(&shares(&[(1, &[2])]));
        assert_eq!(played(&log), lens(&[Sound::ViewerStarted]));
        open_gap(&hub);
        log.lock().unwrap().clear();
        hub.observe(&shares(&[(1, &[])]));
        assert_eq!(played(&log), lens(&[Sound::ViewerStopped]));
        log.lock().unwrap().clear();
        hub.observe(&shares(&[]));
        assert!(played(&log).is_empty(), "own share stop is silent");
    }

    #[test]
    fn toggle_off_silences_that_event_only() {
        let (hub, log) = activity_hub(SoundSettings {
            share_started: false,
            ..SoundSettings::default()
        });
        hub.observe(&shares(&[(2, &[])]));
        assert!(played(&log).is_empty());
        hub.observe(&shares(&[]));
        assert_eq!(played(&log), lens(&[Sound::ShareStopped]));
    }

    #[test]
    fn deafened_user_hears_no_activity() {
        let (hub, log) = activity_hub(SoundSettings::default());
        hub.observe(&EngineEvent::UserUpserted(user(1, 10, true, true)));
        log.lock().unwrap().clear();
        hub.observe(&party(&[2], false));
        hub.observe(&shares(&[(2, &[])]));
        assert!(played(&log).is_empty());
    }

    #[test]
    fn joins_and_leaves_are_rate_limited_to_one_per_second() {
        let (hub, log) = activity_hub(SoundSettings::default());
        hub.observe(&party(&[1], true));
        hub.observe(&party(&[1, 2, 3], true));
        assert_eq!(
            log.lock().unwrap().len(),
            1,
            "burst of two joins sounds once"
        );
        hub.observe(&party(&[1], true));
        assert_eq!(
            log.lock().unwrap().len(),
            1,
            "leaves inside the gap are skipped"
        );
        open_gap(&hub);
        hub.observe(&party(&[1, 2], true));
        assert_eq!(
            log.lock().unwrap().len(),
            2,
            "after the gap it sounds again"
        );
    }

    #[test]
    fn changing_channel_ends_activity_silently() {
        let (hub, log) = activity_hub(SoundSettings::default());
        hub.observe(&shares(&[(2, &[])]));
        log.lock().unwrap().clear();
        hub.observe(&EngineEvent::UserUpserted(user(1, 11, false, false)));
        hub.observe(&shares(&[]));
        assert!(played(&log).is_empty());
    }
}
