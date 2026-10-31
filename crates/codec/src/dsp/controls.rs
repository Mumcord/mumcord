//! Lock-free settings and read-outs shared between UI, hotkey handling and
//! the processing thread.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use super::noise_suppression::{
    Backend, BackendBuilder, SuppressionBackend, SuppressionLevel, build_backend,
};

/// Lowest and highest manual voice-activation threshold, in dBFS.
pub const THRESHOLD_MIN_DBFS: f32 = -60.0;
pub const THRESHOLD_MAX_DBFS: f32 = 0.0;
/// Threshold used until a setting is applied.
pub const THRESHOLD_DEFAULT_DBFS: f32 = -40.0;

/// How the send gate decides when to transmit (FR-1.21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TalkMode {
    /// Transmit while voice is detected (default).
    #[default]
    VoiceActivation,
    /// Transmit while the push-to-talk key is held.
    PushToTalk,
}

/// Microphone level of the processed signal (before the send gate), in dBFS.
/// Silence reads as [`Level::FLOOR_DBFS`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

impl Level {
    /// Lowest value reported; anything quieter is clamped to it.
    pub const FLOOR_DBFS: f32 = -100.0;
}

/// Shared state of one [`DspChain`](super::DspChain).
///
/// Every setter and getter is a single atomic operation, so any thread may
/// call them at any rate (the hotkey thread for push-to-talk, the UI for
/// settings and the level meter). Changes take effect on the next frame.
#[derive(Debug)]
pub struct DspControls {
    echo_cancellation: AtomicBool,
    noise_suppression: AtomicBool,
    noise_backend: AtomicU32,
    suppression_level: AtomicU32,
    /// Backend built by the settings thread and waiting for the processing thread; after a switch
    /// it holds the backend the chain handed back, so that one is freed off the frame path. The
    /// frame path reads `backend_pending` and only ever tries the mutex, it never waits for it.
    backend_pending: AtomicBool,
    backend_slot: Mutex<Option<Box<dyn SuppressionBackend>>>,
    /// Implementations for backends `mumcord-codec` cannot carry, keyed by `Backend::code`. Entered
    /// on the settings thread, never in the frame path.
    builders: Mutex<HashMap<u32, BackendBuilder>>,
    auto_gain: AtomicBool,
    input_gain: AtomicF32,
    music_quality: AtomicBool,
    low_latency: AtomicBool,
    bitrate_cap_kbps: AtomicU32,
    push_to_talk: AtomicBool,
    manual_threshold: AtomicBool,
    threshold_dbfs: AtomicF32,
    ptt_pressed: AtomicBool,
    voice_target: AtomicU8,
    voice_active: AtomicBool,
    transmitting: AtomicBool,
    peak: AtomicF32,
    rms: AtomicF32,
    processing_errors: AtomicU32,
}

impl Default for DspControls {
    /// Echo cancellation, noise suppression and automatic gain on, voice
    /// activation mode (FR-1.21, FR-1.23).
    fn default() -> Self {
        Self {
            echo_cancellation: AtomicBool::new(true),
            noise_suppression: AtomicBool::new(true),
            noise_backend: AtomicU32::new(Backend::default().code()),
            suppression_level: AtomicU32::new(SuppressionLevel::default().code()),
            backend_pending: AtomicBool::new(false),
            backend_slot: Mutex::new(None),
            builders: Mutex::new(HashMap::new()),
            auto_gain: AtomicBool::new(true),
            input_gain: AtomicF32::new(1.0),
            music_quality: AtomicBool::new(false),
            low_latency: AtomicBool::new(false),
            bitrate_cap_kbps: AtomicU32::new(0),
            push_to_talk: AtomicBool::new(false),
            manual_threshold: AtomicBool::new(false),
            threshold_dbfs: AtomicF32::new(THRESHOLD_DEFAULT_DBFS),
            ptt_pressed: AtomicBool::new(false),
            voice_target: AtomicU8::new(0),
            voice_active: AtomicBool::new(false),
            transmitting: AtomicBool::new(false),
            peak: AtomicF32::new(0.0),
            rms: AtomicF32::new(0.0),
            processing_errors: AtomicU32::new(0),
        }
    }
}

impl DspControls {
    pub fn set_echo_cancellation(&self, on: bool) {
        self.echo_cancellation.store(on, Ordering::Relaxed);
    }

    pub fn echo_cancellation(&self) -> bool {
        self.echo_cancellation.load(Ordering::Relaxed)
    }

    pub fn set_noise_suppression(&self, on: bool) {
        self.noise_suppression.store(on, Ordering::Relaxed);
    }

    pub fn noise_suppression(&self) -> bool {
        self.noise_suppression.load(Ordering::Relaxed)
    }

    /// Selects the backend and its level, and prepares the implementation for the processing
    /// thread. Nothing is built when nothing changed, so saving unchanged settings costs nothing.
    pub fn set_noise_backend(&self, kind: Backend, level: SuppressionLevel) {
        let changed = kind != self.noise_backend()
            || (kind.supports_level() && level != self.suppression_level());
        self.noise_backend.store(kind.code(), Ordering::Relaxed);
        self.suppression_level
            .store(level.code(), Ordering::Relaxed);
        if changed {
            // Built before the slot is entered, so the processing thread never finds it held for
            // the length of a model allocation.
            let backend = self.build(kind, level);
            let previous = {
                let mut slot = self.backend_slot.lock().expect("backend slot");
                self.backend_pending.store(true, Ordering::Release);
                slot.replace(backend)
            };
            // A backend the chain handed back (or one it never took) is freed here, on the
            // settings thread.
            drop(previous);
        }
    }

    /// Registers the implementation used for a backend this crate does not carry.
    pub fn register_backend(&self, kind: Backend, builder: BackendBuilder) {
        self.builders
            .lock()
            .expect("backend registry")
            .insert(kind.code(), builder);
    }

    /// Builds `kind` on the settings thread: the registered implementation when there is one, else
    /// the backend `mumcord-codec` carries itself.
    pub fn build(&self, kind: Backend, level: SuppressionLevel) -> Box<dyn SuppressionBackend> {
        let registered = self
            .builders
            .lock()
            .expect("backend registry")
            .get(&kind.code())
            .copied();
        match registered {
            Some(builder) => builder(level),
            None => build_backend(kind, level),
        }
    }

    pub fn noise_backend(&self) -> Backend {
        Backend::from_code(self.noise_backend.load(Ordering::Relaxed))
    }

    /// Puts a backend in the slot without building one (test only, so a failing implementation can
    /// be handed to the chain).
    #[cfg(test)]
    pub(super) fn hand_over_backend(&self, backend: Box<dyn SuppressionBackend>) {
        *self.backend_slot.lock().expect("backend slot") = Some(backend);
        self.backend_pending.store(true, Ordering::Release);
    }

    pub fn suppression_level(&self) -> SuppressionLevel {
        SuppressionLevel::from_code(self.suppression_level.load(Ordering::Relaxed))
    }

    /// Swaps the backend prepared by `set_noise_backend` into `current`, if one is waiting, and
    /// tells whether it did. Called by the processing thread once per frame: it never waits for
    /// the lock (a busy slot means the switch happens on a later frame) and never frees memory,
    /// because the replaced backend stays in the slot for the settings thread to drop.
    pub fn exchange_backend(&self, current: &mut Box<dyn SuppressionBackend>) -> bool {
        if !self.backend_pending.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut slot) = self.backend_slot.try_lock() else {
            return false;
        };
        let Some(next) = slot.as_mut() else {
            return false;
        };
        std::mem::swap(current, next);
        self.backend_pending.store(false, Ordering::Release);
        true
    }

    pub fn set_auto_gain(&self, on: bool) {
        self.auto_gain.store(on, Ordering::Relaxed);
    }

    pub fn auto_gain(&self) -> bool {
        self.auto_gain.load(Ordering::Relaxed)
    }

    /// Manual microphone gain in percent (100 = unchanged), applied before noise suppression.
    pub fn set_input_gain_percent(&self, percent: u16) {
        self.input_gain.store(f32::from(percent) / 100.0);
    }

    /// The manual microphone gain as a factor (1.0 = unchanged).
    pub fn input_gain(&self) -> f32 {
        self.input_gain.load()
    }

    /// "Music quality" (QA.37): the chain skips noise suppression and the speech gain control,
    /// which damage music; with automatic gain on, a slow leveler keeps music as loud as speech.
    /// The engine tunes the encoder for music and lifts the bitrate cap.
    pub fn set_music_quality(&self, on: bool) {
        self.music_quality.store(on, Ordering::Relaxed);
    }

    pub fn music_quality(&self) -> bool {
        self.music_quality.load(Ordering::Relaxed)
    }

    /// "Low-latency mode" (issue #156): the chain bypasses echo cancellation, noise suppression
    /// and automatic gain (music-quality signal path; the gate still decides when to send). The
    /// engine also switches to 10 ms low-delay packets, a minimal jitter buffer and small device
    /// buffers.
    pub fn set_low_latency(&self, on: bool) {
        self.low_latency.store(on, Ordering::Relaxed);
    }

    pub fn low_latency(&self) -> bool {
        self.low_latency.load(Ordering::Relaxed)
    }

    /// Own bitrate cap in kbit/s (FR-2.64); `None` means "Max". The engine's bitrate controller
    /// never goes above it.
    pub fn set_bitrate_cap_kbps(&self, kbps: Option<u32>) {
        self.bitrate_cap_kbps
            .store(kbps.unwrap_or(0), Ordering::Relaxed);
    }

    pub fn bitrate_cap_kbps(&self) -> Option<u32> {
        Some(self.bitrate_cap_kbps.load(Ordering::Relaxed)).filter(|&k| k > 0)
    }

    pub fn set_mode(&self, mode: TalkMode) {
        self.push_to_talk
            .store(mode == TalkMode::PushToTalk, Ordering::Relaxed);
    }

    pub fn mode(&self) -> TalkMode {
        if self.push_to_talk.load(Ordering::Relaxed) {
            TalkMode::PushToTalk
        } else {
            TalkMode::VoiceActivation
        }
    }

    /// Manual voice-activation threshold (FR-1.57): on, the voice-activation gate opens when the
    /// processed input level reaches `threshold_dbfs` instead of following the speech model.
    pub fn set_manual_threshold(&self, on: bool) {
        self.manual_threshold.store(on, Ordering::Relaxed);
    }

    pub fn manual_threshold(&self) -> bool {
        self.manual_threshold.load(Ordering::Relaxed)
    }

    /// Sets the manual threshold, clamped to the slider range.
    pub fn set_threshold_dbfs(&self, dbfs: f32) {
        self.threshold_dbfs
            .store(dbfs.clamp(THRESHOLD_MIN_DBFS, THRESHOLD_MAX_DBFS));
    }

    pub fn threshold_dbfs(&self) -> f32 {
        self.threshold_dbfs.load()
    }

    /// Push-to-talk key state, set by the global-shortcut handler.
    pub fn set_ptt_pressed(&self, pressed: bool) {
        self.ptt_pressed.store(pressed, Ordering::Relaxed);
    }

    pub fn ptt_pressed(&self) -> bool {
        self.ptt_pressed.load(Ordering::Relaxed)
    }

    /// Voice target being held for whisper or shout (issue #67); 0 = none, i.e. talk to the
    /// own channel. A held target transmits like a pressed push-to-talk key. One atomic, so the
    /// capture thread never sees a target without the matching transmit state.
    pub fn set_voice_target(&self, target: u8) {
        self.voice_target.store(target, Ordering::Relaxed);
    }

    pub fn voice_target(&self) -> u8 {
        self.voice_target.load(Ordering::Relaxed)
    }

    /// Whether voice activity detection currently hears speech (including
    /// hangover), independent of the talk mode.
    pub fn voice_active(&self) -> bool {
        self.voice_active.load(Ordering::Relaxed)
    }

    /// Whether the send gate is open, i.e. frames are being transmitted.
    pub fn transmitting(&self) -> bool {
        self.transmitting.load(Ordering::Relaxed)
    }

    /// Level of the most recent processed frame. The UI polls this at its
    /// own rate (e.g. 30 Hz); the chain updates it every 10 ms.
    pub fn level(&self) -> Level {
        Level {
            peak_dbfs: to_dbfs(self.peak.load()),
            rms_dbfs: to_dbfs(self.rms.load()),
        }
    }

    /// Frames a processing library rejected (passed through unprocessed).
    /// Counted instead of logged because the processing path must not log.
    pub fn processing_errors(&self) -> u32 {
        self.processing_errors.load(Ordering::Relaxed)
    }

    pub(super) fn publish_frame(&self, peak: f32, rms: f32, voice: bool, transmitting: bool) {
        self.peak.store(peak);
        self.rms.store(rms);
        self.voice_active.store(voice, Ordering::Relaxed);
        self.transmitting.store(transmitting, Ordering::Relaxed);
    }

    pub(super) fn count_error(&self) {
        self.processing_errors.fetch_add(1, Ordering::Relaxed);
    }
}

fn to_dbfs(linear: f32) -> f32 {
    if linear <= 0.0 {
        Level::FLOOR_DBFS
    } else {
        (20.0 * linear.log10()).max(Level::FLOOR_DBFS)
    }
}

#[derive(Debug)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_clamped_to_the_slider_range() {
        let controls = DspControls::default();
        assert_eq!(controls.threshold_dbfs(), THRESHOLD_DEFAULT_DBFS);
        controls.set_threshold_dbfs(-120.0);
        assert_eq!(controls.threshold_dbfs(), THRESHOLD_MIN_DBFS);
        controls.set_threshold_dbfs(6.0);
        assert_eq!(controls.threshold_dbfs(), THRESHOLD_MAX_DBFS);
    }
}
