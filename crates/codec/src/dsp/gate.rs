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

/// How far below the threshold (dB) an open manual gate still counts as speech, so a voice
/// hovering around the threshold does not flutter the gate.
pub(super) const MANUAL_HYSTERESIS_DB: f32 = 3.0;

/// Manual voice-activation threshold (FR-1.57): opens on the first 10 ms frame whose peak
/// reaches the threshold (attack of one frame) and closes after [`VAD_HANGOVER_FRAMES`] frames
/// below it.
#[derive(Debug, Default)]
pub(super) struct ThresholdGate {
    open: bool,
    hangover: u32,
}

impl ThresholdGate {
    /// Feeds one frame's peak level (dBFS); returns whether the gate is open (including hold).
    pub(super) fn update(&mut self, peak_dbfs: f32, threshold_dbfs: f32) -> bool {
        let limit = if self.open {
            threshold_dbfs - MANUAL_HYSTERESIS_DB
        } else {
            threshold_dbfs
        };
        if peak_dbfs >= limit {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_gate_stays_closed_below_and_opens_above_threshold() {
        let mut gate = ThresholdGate::default();
        for _ in 0..50 {
            assert!(!gate.update(-45.0, -40.0));
        }
        assert!(gate.update(-39.0, -40.0));
    }

    #[test]
    fn manual_gate_holds_for_300_ms_then_closes() {
        let mut gate = ThresholdGate::default();
        assert!(gate.update(-10.0, -40.0));
        for _ in 0..VAD_HANGOVER_FRAMES {
            assert!(gate.update(-90.0, -40.0));
        }
        assert!(!gate.update(-90.0, -40.0));
    }

    #[test]
    fn manual_gate_hysteresis_keeps_open_just_below_threshold() {
        let mut gate = ThresholdGate::default();
        assert!(gate.update(-30.0, -40.0));
        // Within the hysteresis band the hold is refreshed every frame.
        for _ in 0..(VAD_HANGOVER_FRAMES * 3) {
            assert!(gate.update(-41.0, -40.0));
        }
        // The same level does not open a closed gate.
        let mut closed = ThresholdGate::default();
        assert!(!closed.update(-41.0, -40.0));
    }
}
