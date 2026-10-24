//! Send gate: voice activity detection with hysteresis and hangover, or
//! push-to-talk with a short release tail.

/// Speech probability (from the RNNoise model) that opens a closed gate.
const VAD_OPEN: f32 = 0.6;
/// Probability that keeps an open gate open; lower than [`VAD_OPEN`] so
/// the gate does not flutter around a single threshold.
const VAD_HOLD: f32 = 0.3;
/// Frames that are too quiet to be speech no matter what the model says
/// (RMS, linear). Keeps faint background voices from opening the gate.
const VAD_MIN_RMS: f32 = 0.002; // ≈ -54 dBFS
/// Frames the gate stays open after speech ends, so word endings and short
/// pauses between words are not cut off (300 ms).
pub(super) const VAD_HANGOVER_FRAMES: u32 = 30;
/// Frames transmitted after the push-to-talk key is released; people tend
/// to let go a moment before the last syllable is out (50 ms).
pub(super) const PTT_TAIL_FRAMES: u32 = 5;

#[derive(Debug, Default)]
pub(super) struct VoiceDetector {
    open: bool,
    hangover: u32,
}

impl VoiceDetector {
    /// Feeds one frame's speech probability and RMS; returns whether voice
    /// is active (including hangover).
    pub(super) fn update(&mut self, probability: f32, rms: f32) -> bool {
        let threshold = if self.open { VAD_HOLD } else { VAD_OPEN };
        if probability >= threshold && rms >= VAD_MIN_RMS {
            self.open = true;
            self.hangover = VAD_HANGOVER_FRAMES;
        } else if self.hangover > 0 {
            self.hangover -= 1;
        } else {
            self.open = false;
        }
        self.open
    }
}

#[derive(Debug, Default)]
pub(super) struct PttGate {
    tail: u32,
}

impl PttGate {
    pub(super) fn update(&mut self, pressed: bool) -> bool {
        if pressed {
            self.tail = PTT_TAIL_FRAMES;
            true
        } else if self.tail > 0 {
            self.tail -= 1;
            true
        } else {
            false
        }
    }
}
