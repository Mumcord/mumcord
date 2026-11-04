//! The Windows capture rules that need no Windows API, so they are tested on every system:
//! which Windows versions can capture and send sound, which process owns the picked window, and
//! turning the loopback's stereo PCM into the share's 10 ms mono frames.

#![cfg_attr(not(any(windows, test)), allow(dead_code))]

use std::collections::VecDeque;

use mumcord_audio::{FRAME_SAMPLES, Frame};

use super::signal::ShareKind;

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

/// What the user picked (FR-7.76). The picker hands back only a display name and a size, so a
/// pick counts as a whole screen only when no window has that name and its size is a monitor's.
/// Anything unclear counts as a window, which never shares more than one app's sound (FR-7.87).
pub(super) fn pick_kind(
    windows: &[TopWindow],
    picked: &str,
    size: (i32, i32),
    monitors: &[(i32, i32)],
) -> ShareKind {
    let titled = picked.is_empty() || windows.iter().any(|w| w.title == picked);
    if !titled && monitors.contains(&size) {
        ShareKind::Screen
    } else {
        ShareKind::Window
    }
}

/// A running process as the system's process snapshot lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProcessEntry {
    pub pid: u32,
    pub parent: u32,
    /// The program's file name, e.g. `chrome.exe`.
    pub exe: String,
}

/// The app whose sound goes with a window drawn by `pid` (FR-7.79): the root of its process
/// tree and the app's name for the sharer's live bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnerTree {
    pub root: u32,
    pub name: Option<String>,
}

/// The process tree that owns a window drawn by `pid`: the topmost ancestor running the same
/// program, so a window of one process of a multi-process app (browsers, Electron apps) brings
/// the sound its sibling processes play too. `None` when the window belongs to Mumcord's own
/// tree, e.g. a window of its web view: Mumcord is never part of the share sound (FR-7.77).
pub(super) fn owner_tree(processes: &[ProcessEntry], pid: u32, own_pid: u32) -> Option<OwnerTree> {
    if pid == own_pid {
        return None;
    }
    let find = |pid: u32| processes.iter().find(|p| p.pid == pid);
    let Some(start) = find(pid) else {
        return Some(OwnerTree {
            root: pid,
            name: None,
        });
    };
    let mut root = pid;
    let mut same_program = true;
    let mut current = start;
    // Bounded: a recycled parent id can make the chain loop.
    for _ in 0..processes.len() {
        if current.parent == 0 || current.parent == current.pid {
            break;
        }
        let Some(parent) = find(current.parent) else {
            break;
        };
        if parent.pid == own_pid {
            return None;
        }
        same_program &= parent.exe.eq_ignore_ascii_case(&start.exe);
        if same_program {
            root = parent.pid;
        }
        current = parent;
    }
    Some(OwnerTree {
        root,
        name: app_name(&start.exe),
    })
}

/// `chrome.exe` → `chrome`; nothing for an empty name.
fn app_name(exe: &str) -> Option<String> {
    let stem = match exe.len().checked_sub(4) {
        Some(cut) if exe.is_char_boundary(cut) && exe[cut..].eq_ignore_ascii_case(".exe") => {
            &exe[..cut]
        }
        _ => exe,
    };
    (!stem.is_empty()).then(|| stem.to_owned())
}

/// After this many failures in a row (opening the loopback or reading from it), the share sound
/// counts as failed (FR-7.84) instead of being retried forever.
pub(super) const MAX_FAILURES: u32 = 3;

/// Counts the loopback's failures in a row; sound that arrives starts the count anew.
#[derive(Debug, Default)]
pub(super) struct Reopen {
    failures: u32,
}

impl Reopen {
    /// Records a failure; `true` once the sound should be given up.
    pub fn failed(&mut self) -> bool {
        self.failures += 1;
        self.failures >= MAX_FAILURES
    }

    /// Sound arrived: the loopback works.
    pub fn worked(&mut self) {
        self.failures = 0;
    }
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

    const MONITORS: [(i32, i32); 2] = [(2560, 1440), (1920, 1080)];

    #[test]
    fn a_monitor_sized_pick_without_a_window_of_that_name_is_a_screen() {
        let windows = [window("Spotify Premium", 40)];
        assert_eq!(
            pick_kind(&windows, "Generic PnP Monitor", (1920, 1080), &MONITORS),
            ShareKind::Screen
        );
    }

    #[test]
    fn unclear_picks_count_as_windows() {
        let windows = [window("Doom", 50)];
        assert_eq!(
            pick_kind(&windows, "Doom", (1920, 1080), &MONITORS),
            ShareKind::Window,
            "a full-screen window"
        );
        assert_eq!(
            pick_kind(&windows, "Renamed tab", (1280, 720), &MONITORS),
            ShareKind::Window,
            "title changed since the pick"
        );
        assert_eq!(
            pick_kind(&windows, "", (1920, 1080), &MONITORS),
            ShareKind::Window,
            "no name"
        );
    }

    fn process(pid: u32, parent: u32, exe: &str) -> ProcessEntry {
        ProcessEntry {
            pid,
            parent,
            exe: exe.to_owned(),
        }
    }

    fn tree(root: u32, name: &str) -> Option<OwnerTree> {
        Some(OwnerTree {
            root,
            name: Some(name.to_owned()),
        })
    }

    #[test]
    fn a_window_brings_its_whole_app_tree() {
        let processes = [
            process(4, 0, "explorer.exe"),
            process(10, 4, "chrome.exe"),
            process(11, 10, "chrome.exe"),
            process(12, 11, "CHROME.EXE"),
            process(20, 4, "Spotify.exe"),
        ];
        assert_eq!(owner_tree(&processes, 12, 7), tree(10, "CHROME"));
        assert_eq!(owner_tree(&processes, 20, 7), tree(20, "Spotify"));
    }

    #[test]
    fn mumcord_s_own_tree_never_gives_sound() {
        let processes = [
            process(4, 0, "explorer.exe"),
            process(7, 4, "mumcord.exe"),
            process(8, 7, "msedgewebview2.exe"),
            process(9, 8, "msedgewebview2.exe"),
        ];
        assert_eq!(owner_tree(&processes, 7, 7), None, "Mumcord itself");
        assert_eq!(owner_tree(&processes, 9, 7), None, "its web view");
    }

    #[test]
    fn odd_snapshots_still_give_an_answer() {
        assert_eq!(
            owner_tree(&[], 30, 7),
            Some(OwnerTree {
                root: 30,
                name: None
            }),
            "process gone from the snapshot"
        );
        let looped = [process(30, 31, "a.exe"), process(31, 30, "a.exe")];
        assert!(owner_tree(&looped, 30, 7).is_some(), "recycled parent ids");
        assert_eq!(owner_tree(&[process(5, 5, "x")], 5, 7), tree(5, "x"));
    }

    #[test]
    fn reopening_gives_up_after_repeated_failures() {
        let mut reopen = Reopen::default();
        assert!(!reopen.failed());
        assert!(!reopen.failed());
        reopen.worked();
        assert!(!reopen.failed(), "sound that arrived starts anew");
        assert!(!reopen.failed());
        assert!(reopen.failed());
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
