//! Lock-free settings and read-outs shared between UI, hotkey handling and
//! the processing thread.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

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
    auto_gain: AtomicBool,
    push_to_talk: AtomicBool,
    ptt_pressed: AtomicBool,
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
            auto_gain: AtomicBool::new(true),
            push_to_talk: AtomicBool::new(false),
            ptt_pressed: AtomicBool::new(false),
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

    pub fn set_auto_gain(&self, on: bool) {
        self.auto_gain.store(on, Ordering::Relaxed);
    }

    pub fn auto_gain(&self) -> bool {
        self.auto_gain.load(Ordering::Relaxed)
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

    /// Push-to-talk key state, set by the global-shortcut handler.
    pub fn set_ptt_pressed(&self, pressed: bool) {
        self.ptt_pressed.store(pressed, Ordering::Relaxed);
    }

    pub fn ptt_pressed(&self) -> bool {
        self.ptt_pressed.load(Ordering::Relaxed)
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
