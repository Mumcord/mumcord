//! Where the pictures and the sound of the own share come from.
//!
//! The encode thread pulls one picture per frame at the current target frame rate, so a source
//! only has to keep its latest picture at hand. The OS capture backends plug in here; until then
//! [`TestPattern`] is shared.
//!
//! Sound is only the shared app's own sound (FR-2.42), from per-app capture where the OS has it
//! (Windows process loopback, macOS ScreenCaptureKit); [`TestTone`] stands in for tests.

use std::time::{Duration, Instant};

use mumcord_audio::{Frame, SAMPLE_RATE};
use mumcord_screenshare::I420Frame;

use super::signal::ShareKind;

/// A source of pictures for the own share.
pub trait CaptureSource: Send {
    /// The latest picture, scaled to fit into `max_width` × `max_height` with its aspect ratio
    /// kept (even sizes), or `None` when there is nothing new to show. Called on the encode
    /// thread, never on the audio thread; it may block briefly but not for a whole frame.
    fn frame(&mut self, max_width: u32, max_height: u32) -> Option<I420Frame>;

    /// Whether the capture ended on its own (the desktop's stop button, the window closed);
    /// no more pictures come and the share should stop.
    fn ended(&self) -> bool {
        false
    }

    /// What the user picked in the system picker (issue #163); `None` where the platform cannot
    /// tell a window from the whole screen.
    fn kind(&self) -> Option<ShareKind> {
        None
    }

    /// The picked app's own sound, taken once when the share starts; `None` where the OS cannot
    /// isolate it (FR-2.42).
    fn take_sound(&mut self) -> Option<Box<dyn SoundSource>> {
        None
    }
}

/// A moving picture for tests and for sharing before OS capture exists: a gradient with a bar
/// that sweeps across, so every frame differs and motion is visible.
#[derive(Default)]
pub struct TestPattern {
    tick: u32,
}

/// Pixels the bar moves per frame.
const BAR_STEP: u32 = 8;

impl CaptureSource for TestPattern {
    fn frame(&mut self, max_width: u32, max_height: u32) -> Option<I420Frame> {
        let (width, height) = (max_width & !1, max_height & !1);
        if width == 0 || height == 0 {
            return None;
        }
        let tick = self.tick;
        self.tick = self.tick.wrapping_add(1);
        let luma = (width * height) as usize;
        let chroma = I420Frame::buffer_len(width, height) - luma;
        let mut data = Vec::with_capacity(luma + chroma);
        let bar_width = (width / 16).max(2);
        let bar = tick.wrapping_mul(BAR_STEP) % width;
        for y in 0..height {
            for x in 0..width {
                let in_bar = x.wrapping_sub(bar) < bar_width;
                let value = if in_bar {
                    235
                } else {
                    16 + (x + y) * 200 / (width + height)
                };
                data.push(value as u8);
            }
        }
        // Chroma drifts slowly so colour changes over time too.
        let half = chroma / 2;
        data.extend(std::iter::repeat_n(64 + (tick % 128) as u8, half));
        data.extend(std::iter::repeat_n(192 - (tick % 128) as u8, chroma - half));
        I420Frame::new(width, height, data).ok()
    }
}

/// A source of the shared app's sound for the own share.
pub trait SoundSource: Send {
    /// Waits up to `timeout` for the next 10 ms of sound (48 kHz mono, as the audio crate's
    /// frames) and writes it to `frame`; `false` when none came in time, e.g. while the app is
    /// silent. Called on the share's sound thread, never on the audio thread; the source paces
    /// the frames, at the rate the OS delivers them.
    fn next_frame(&mut self, frame: &mut Frame, timeout: Duration) -> bool;
}

/// A steady sine for tests: what the viewer hears has to match its level.
pub struct TestTone {
    hz: f32,
    amplitude: f32,
    sample: u64,
    next: Option<Instant>,
}

impl TestTone {
    pub fn new(hz: f32, amplitude: f32) -> Self {
        Self {
            hz,
            amplitude,
            sample: 0,
            next: None,
        }
    }
}

const FRAME_TIME: Duration = Duration::from_millis(10);

impl SoundSource for TestTone {
    fn next_frame(&mut self, frame: &mut Frame, timeout: Duration) -> bool {
        let now = Instant::now();
        let due = *self.next.get_or_insert(now);
        if due > now + timeout {
            std::thread::sleep(timeout);
            return false;
        }
        if due > now {
            std::thread::sleep(due - now);
        }
        // Far behind (the thread was stalled): carry on from now instead of rushing frames out.
        let behind = Instant::now().saturating_duration_since(due) > FRAME_TIME * 10;
        self.next = Some(if behind { Instant::now() } else { due } + FRAME_TIME);
        let step = std::f64::consts::TAU * f64::from(self.hz) / f64::from(SAMPLE_RATE);
        for s in frame.iter_mut() {
            // In f64 and wrapped per period, so the phase stays exact over long tests.
            let phase = (self.sample as f64 * step) % std::f64::consts::TAU;
            *s = self.amplitude * phase.sin() as f32;
            self.sample += 1;
        }
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use mumcord_audio::FRAME_SAMPLES;

    #[test]
    fn the_tone_comes_in_real_time_at_its_level() {
        let mut tone = TestTone::new(440.0, 0.5);
        let mut frame = [0.0; FRAME_SAMPLES];
        let started = Instant::now();
        let mut energy = 0.0;
        for _ in 0..10 {
            assert!(tone.next_frame(&mut frame, Duration::from_millis(50)));
            energy += frame.iter().map(|s| s * s).sum::<f32>();
        }
        assert!(started.elapsed() >= Duration::from_millis(90));
        let rms = (energy / (10 * FRAME_SAMPLES) as f32).sqrt();
        assert!(
            (rms - 0.5 / std::f32::consts::SQRT_2).abs() < 0.01,
            "rms {rms}"
        );
    }

    #[test]
    fn the_pattern_fits_the_box_and_moves() {
        let mut pattern = TestPattern::default();
        let first = pattern.frame(1281, 720).unwrap();
        assert_eq!((first.width(), first.height()), (1280, 720));
        let second = pattern.frame(1280, 720).unwrap();
        assert_ne!(first.y(), second.y(), "the bar moved");
        let small = pattern.frame(854, 480).unwrap();
        assert_eq!((small.width(), small.height()), (854, 480));
        assert!(pattern.frame(1, 720).is_none());
    }
}
