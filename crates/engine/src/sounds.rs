//! Notification sounds (FR-1.27): short, soft tones for people joining or leaving your channel,
//! your own mute/deafen changes and new messages. No speech.
//!
//! The [`SoundHub`] observes the engine's own event flow ([`crate::session::Events::emit`]),
//! tracks who is in your channel, maps events to a [`Sound`] and, if the per-event toggle in
//! the settings allows it, writes the embedded 48 kHz mono samples into the mixer's mix-in
//! port. Sounds only play while audio is open (a voice pipeline attached its port) and not
//! while you are deafened, except the undeafen sound itself.
//!
//! Self mute/deafen sounds follow the server's echo of your own user state, so they match
//! what the server (and everyone else) sees. New-message sounds are requested by the chat
//! code through [`SoundHub::play`] with [`Sound::Message`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

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
}

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
        }
    }

    /// 48 kHz mono samples of the sound.
    pub fn samples(self) -> Vec<f32> {
        decode_wav(self.wav())
    }
}

/// Decodes the embedded 16-bit mono WAV files (canonical RIFF layout written by
/// `assets/sounds/generate.py`); anything unexpected yields silence-free empty output.
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

/// Follows users and the own state to turn events into sounds.
#[derive(Default)]
struct Tracker {
    own: Option<u32>,
    users: HashMap<u32, Known>,
}

impl Tracker {
    fn own_channel(&self) -> Option<u32> {
        self.own.and_then(|s| self.users.get(&s)).map(|k| k.channel)
    }

    fn own_deaf(&self) -> bool {
        self.own
            .and_then(|s| self.users.get(&s))
            .is_some_and(|k| k.deaf)
    }

    /// Updates the state from `event`; returns the sound it calls for and whether the own
    /// user was deafened before it.
    fn observe(&mut self, event: &EngineEvent) -> Option<(Sound, bool)> {
        match event {
            EngineEvent::ServerSynced(snapshot) => {
                self.own = Some(snapshot.self_session);
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
}

/// Maps engine events to sounds and plays them. Cheap to share.
pub struct SoundHub {
    settings: Box<dyn Fn() -> SoundSettings + Send + Sync>,
    state: Mutex<State>,
}

impl SoundHub {
    /// `settings` returns the current per-event toggles.
    pub fn new(settings: impl Fn() -> SoundSettings + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            settings: Box::new(settings),
            state: Mutex::new(State {
                tracker: Tracker::default(),
                out: None,
            }),
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

    /// Audio is open: sounds go to `out` from now on.
    pub fn attach(&self, out: SampleSink) {
        self.state().out = Some(out);
    }

    /// Audio closed: no sounds until the next [`attach`](Self::attach).
    pub fn detach(&self) {
        self.state().out = None;
    }

    /// Feeds one engine event (called for every emitted event).
    pub fn observe(&self, event: &EngineEvent) {
        let mut state = self.state();
        if let Some((sound, was_deaf)) = state.tracker.observe(event) {
            self.play_locked(&mut state, sound, was_deaf);
        }
    }

    /// Plays `sound` now (for events the hub cannot see, such as [`Sound::Message`]),
    /// subject to the toggles and to deafness.
    pub fn play(&self, sound: Sound) {
        let mut state = self.state();
        let deaf = state.tracker.own_deaf();
        self.play_locked(&mut state, sound, deaf);
    }

    fn play_locked(&self, state: &mut State, sound: Sound, was_deaf: bool) {
        if was_deaf && sound != Sound::Undeafen {
            return;
        }
        if !sound.enabled(&(self.settings)()) {
            return;
        }
        if let Some(out) = state.out.as_mut() {
            let samples: Vec<f32> = sound
                .samples()
                .into_iter()
                .map(|s| s * SOUND_GAIN)
                .collect();
            if !out(&samples) {
                tracing::debug!("notification sound skipped: mixer input is full");
            }
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

    const ALL: [Sound; 7] = [
        Sound::UserJoined,
        Sound::UserLeft,
        Sound::Mute,
        Sound::Unmute,
        Sound::Deafen,
        Sound::Undeafen,
        Sound::Message,
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
            listening_channel_ids: Vec::new(),
            has_avatar: false,
            has_comment: false,
            volume: 100,
            local_mute: false,
        }
    }

    fn synced(users: Vec<UserView>) -> EngineEvent {
        EngineEvent::ServerSynced(Box::new(Snapshot {
            server_id: 0,
            self_session: 1,
            info: crate::session::ServerInfoView {
                version: String::new(),
                follow_supported: true,
                welcome_text: None,
                max_message_length: 5000,
                max_image_message_length: 0,
                allow_html: false,
            },
            channels: Vec::new(),
            users,
            followed_channel_ids: Vec::new(),
            unread: Vec::new(),
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
            assert!(
                samples.len() <= 48_000 * 400 / 1000,
                "{s:?} longer than 400 ms"
            );
            assert!(peak_dbfs(&samples) <= -12.0, "{s:?} too loud");
            assert!(peak_dbfs(&samples) > -40.0, "{s:?} inaudible");
        }
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
}
