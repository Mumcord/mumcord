//! The shared app's sound on macOS (FR-2.42). ScreenCaptureKit delivers it in the same stream
//! as the pictures ([`super::capture_macos`]), already limited to the shared content and without
//! Mumcord's own output (`excludesCurrentProcessAudio`). Its callback cuts the samples into
//! 10 ms frames and hands them over here; the share's sound thread takes them as a
//! [`SoundSource`].
//!
//! Free of macOS calls, so it is tested on every system.

use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::time::Duration;

use mumcord_audio::{FRAME_SAMPLES, Frame};

use super::capture::SoundSource;

/// Frames that may wait for the sound thread (0.5 s); older sound is dropped instead of piling
/// up latency behind a stalled thread.
const QUEUE_FRAMES: usize = 50;

/// The capture side: fed with whatever sample counts the OS delivers.
pub struct SoundSender {
    frame: Frame,
    filled: usize,
    tx: SyncSender<Frame>,
    dropped: u64,
}

/// The sound thread's side.
pub struct MacSound {
    rx: Receiver<Frame>,
}

/// A connected pair; dropping the sender ends the sound.
pub fn sound_channel() -> (SoundSender, MacSound) {
    let (tx, rx) = sync_channel(QUEUE_FRAMES);
    (
        SoundSender {
            frame: [0.0; FRAME_SAMPLES],
            filled: 0,
            tx,
            dropped: 0,
        },
        MacSound { rx },
    )
}

impl SoundSender {
    /// Adds 48 kHz mono samples; every full 10 ms frame is passed on. Never blocks, since it runs
    /// on ScreenCaptureKit's delivery queue.
    pub fn push(&mut self, mut samples: &[f32]) {
        while !samples.is_empty() {
            let take = (FRAME_SAMPLES - self.filled).min(samples.len());
            self.frame[self.filled..self.filled + take].copy_from_slice(&samples[..take]);
            self.filled += take;
            samples = &samples[take..];
            if self.filled < FRAME_SAMPLES {
                return;
            }
            self.filled = 0;
            match self.tx.try_send(self.frame) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                Err(TrySendError::Full(_)) => {
                    self.dropped += 1;
                    if self.dropped.is_power_of_two() {
                        tracing::debug!(
                            dropped = self.dropped,
                            "screen share sound frames dropped"
                        );
                    }
                }
            }
        }
    }
}

impl SoundSource for MacSound {
    fn next_frame(&mut self, frame: &mut Frame, timeout: Duration) -> bool {
        match self.rx.recv_timeout(timeout) {
            Ok(next) => {
                *frame = next;
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => {
                // The capture is gone; wait like a silent app instead of spinning until the share
                // stops the sound thread.
                std::thread::sleep(timeout);
                false
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const WAIT: Duration = Duration::from_millis(20);

    #[test]
    fn odd_deliveries_come_out_as_whole_frames_in_order() {
        let (mut tx, mut rx) = sound_channel();
        let samples: Vec<f32> = (0..FRAME_SAMPLES * 2 + 7).map(|i| i as f32).collect();
        for chunk in samples.chunks(333) {
            tx.push(chunk);
        }
        let mut frame = [0.0; FRAME_SAMPLES];
        assert!(rx.next_frame(&mut frame, WAIT));
        assert_eq!(frame[0], 0.0);
        assert_eq!(frame[FRAME_SAMPLES - 1], (FRAME_SAMPLES - 1) as f32);
        assert!(rx.next_frame(&mut frame, WAIT));
        assert_eq!(frame[0], FRAME_SAMPLES as f32);
        assert!(
            !rx.next_frame(&mut frame, WAIT),
            "7 samples are not a frame yet"
        );
    }

    #[test]
    fn a_full_queue_drops_new_frames_without_blocking() {
        let (mut tx, mut rx) = sound_channel();
        let frame_of = |v: f32| [v; FRAME_SAMPLES];
        for i in 0..QUEUE_FRAMES + 5 {
            tx.push(&frame_of(i as f32));
        }
        assert_eq!(tx.dropped, 5);
        let mut frame = [0.0; FRAME_SAMPLES];
        for i in 0..QUEUE_FRAMES {
            assert!(rx.next_frame(&mut frame, WAIT));
            assert_eq!(frame[0], i as f32);
        }
    }

    #[test]
    fn a_closed_capture_is_silence() {
        let (tx, mut rx) = sound_channel();
        drop(tx);
        let mut frame = [0.0; FRAME_SAMPLES];
        let started = std::time::Instant::now();
        assert!(!rx.next_frame(&mut frame, WAIT));
        assert!(started.elapsed() >= WAIT, "waits instead of spinning");
    }
}
