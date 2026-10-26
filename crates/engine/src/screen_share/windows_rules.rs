//! The Windows capture rules that need no Windows API, so they are tested on every system:
//! which Windows versions can capture and send sound, which process owns the picked window, and
//! turning the loopback's stereo PCM into the share's 10 ms mono frames.

#![cfg_attr(not(any(windows, test)), allow(dead_code))]

use std::collections::VecDeque;

use mumcord_audio::{FRAME_SAMPLES, Frame};

/// Windows 10 1903: the free-threaded capture frame pool, so frames can be taken on the encode
/// thread without a UI thread.
pub(super) const CAPTURE_MIN_BUILD: u32 = 18362;

/// Windows 10 2004: process loopback, the only way to capture just one app's sound (FR-2.42).
pub(super) const SOUND_MIN_BUILD: u32 = 19041;

pub(super) fn capture_supported(build: u32) -> bool {
    build >= CAPTURE_MIN_BUILD
}

pub(super) fn sound_supported(build: u32) -> bool {
    build >= SOUND_MIN_BUILD
}

/// A visible top-level window as the desktop lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TopWindow {
    pub title: String,
    pub pid: u32,
}

/// The process whose sound goes with the picked item, `None` for no sound.
///
/// The system picker hands back only the item's display name, which for a window is its title.
/// The sound is sent only when that title belongs to exactly one process: a whole screen (no
/// window has the monitor's name) or two apps with the same title stay silent rather than send
/// the wrong app. Mumcord itself never: its sound is the voice chat (FR-2.42).
pub(super) fn sound_process(windows: &[TopWindow], picked: &str, own_pid: u32) -> Option<u32> {
    if picked.is_empty() {
        return None;
    }
    let mut owners = windows.iter().filter(|w| w.title == picked).map(|w| w.pid);
    let pid = owners.next()?;
    if owners.any(|other| other != pid) || pid == own_pid {
        return None;
    }
    Some(pid)
}

/// At most this much sound waits for the share's sound thread; older sound is dropped so a
/// stalled thread does not turn into a growing delay.
const MAX_PENDING: usize = FRAME_SAMPLES * 20;

/// Collects the loopback's packets (any length) and hands them out as 10 ms mono frames.
#[derive(Debug)]
pub(super) struct MonoFrames {
    pending: VecDeque<f32>,
}

impl Default for MonoFrames {
    fn default() -> Self {
        Self {
            pending: VecDeque::with_capacity(MAX_PENDING + FRAME_SAMPLES),
        }
    }
}

impl MonoFrames {
    /// Adds interleaved 16-bit stereo samples, mixed down to mono.
    pub fn push_stereo_i16(&mut self, samples: &[i16]) {
        self.pending.extend(
            samples
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[l, r]| (f32::from(l) + f32::from(r)) / (2.0 * 32768.0)),
        );
        self.trim();
    }

    /// Adds `samples` of silence (the loopback flags silent packets instead of sending zeros).
    pub fn push_silence(&mut self, samples: usize) {
        self.pending.extend(std::iter::repeat_n(0.0, samples));
        self.trim();
    }

    /// Takes the next whole frame; `false` while less than one is collected.
    pub fn pop(&mut self, frame: &mut Frame) -> bool {
        if self.pending.len() < FRAME_SAMPLES {
            return false;
        }
        for (out, sample) in frame.iter_mut().zip(self.pending.drain(..FRAME_SAMPLES)) {
            *out = sample;
        }
        true
    }

    fn trim(&mut self) {
        let over = self.pending.len().saturating_sub(MAX_PENDING);
        self.pending.drain(..over);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_gate_capture_and_sound() {
        assert!(!capture_supported(17763), "1809");
        assert!(capture_supported(18362), "1903");
        assert!(!sound_supported(18363), "1909");
        assert!(sound_supported(19041), "2004");
        assert!(sound_supported(22631), "Windows 11");
    }

    fn window(title: &str, pid: u32) -> TopWindow {
        TopWindow {
            title: title.to_owned(),
            pid,
        }
    }

    #[test]
    fn the_picked_window_s_process_gets_the_sound() {
        let windows = [
            window("Spotify Premium", 40),
            window("Doom", 50),
            window("Doom", 50),
        ];
        assert_eq!(sound_process(&windows, "Spotify Premium", 1), Some(40));
        assert_eq!(
            sound_process(&windows, "Doom", 1),
            Some(50),
            "one app, two windows"
        );
    }

    #[test]
    fn screens_unclear_titles_and_mumcord_itself_stay_silent() {
        let windows = [
            window("Untitled", 40),
            window("Untitled", 41),
            window("Mumcord", 7),
        ];
        assert_eq!(sound_process(&windows, "Generic PnP Monitor", 7), None);
        assert_eq!(sound_process(&windows, "Untitled", 7), None, "two apps");
        assert_eq!(sound_process(&windows, "Mumcord", 7), None, "own voice");
        assert_eq!(sound_process(&windows, "", 7), None);
    }

    #[test]
    fn stereo_packets_become_mono_frames() {
        let mut frames = MonoFrames::default();
        let mut frame = [1.0; FRAME_SAMPLES];
        let half: Vec<i16> = [16384, 16384].repeat(FRAME_SAMPLES / 2);
        frames.push_stereo_i16(&half);
        assert!(!frames.pop(&mut frame), "only half a frame yet");
        frames.push_stereo_i16(&[16384, -16384].repeat(FRAME_SAMPLES / 2));
        assert!(frames.pop(&mut frame));
        assert!((frame[0] - 0.5).abs() < 1e-6);
        assert!(
            frame[FRAME_SAMPLES - 1].abs() < 1e-6,
            "left and right cancel"
        );
        assert!(!frames.pop(&mut frame));
    }

    #[test]
    fn silence_counts_and_a_backlog_is_capped() {
        let mut frames = MonoFrames::default();
        let mut frame = [1.0; FRAME_SAMPLES];
        frames.push_silence(FRAME_SAMPLES);
        assert!(frames.pop(&mut frame));
        assert!(frame.iter().all(|&s| s == 0.0));

        frames.push_silence(MAX_PENDING * 3);
        let mut count = 0;
        while frames.pop(&mut frame) {
            count += 1;
        }
        assert_eq!(count, MAX_PENDING / FRAME_SAMPLES);
    }
}
