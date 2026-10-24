//! Linux Wayland: mouse buttons read directly from evdev (FR-1.21, QA.4, §9).
//!
//! Only used when the user switched on "Allow mouse buttons (requires input group)".
//! Devices are filtered by their sysfs capabilities *before* anything is opened: a device must
//! report `BTN_LEFT` and relative motion and must have no letter or digit keys, so keyboards
//! (including keyboard interfaces of gaming mice) are never opened. Nodes are opened read-only
//! and never grabbed, only the configured button's codes are evaluated, nothing is recorded
//! and no input is ever logged. New mice are picked up by a periodic rescan; unplugged ones
//! end their reader.

use std::collections::HashSet;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::ops::RangeInclusive;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tauri::async_runtime::{self, JoinHandle};
use tokio::io::unix::AsyncFd;
use tokio::task::JoinSet;

use super::mouse::{MouseButton, OnChange, StartError};
use crate::commands::ptt::PttMouseState;

const SYS_INPUT: &str = "/sys/class/input";
const DEV_INPUT: &str = "/dev/input";
/// How often new mice are looked for.
const RESCAN: Duration = Duration::from_secs(2);

const EV_KEY: u16 = 0x01;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const KEY_BACK: u16 = 158;
const KEY_FORWARD: u16 = 159;
const BTN_LEFT: u16 = 0x110;
const BTN_MIDDLE: u16 = 0x112;
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;
const BTN_FORWARD: u16 = 0x115;
const BTN_BACK: u16 = 0x116;

/// Digit and letter key codes (`KEY_1`–`KEY_0`, `KEY_Q`–`KEY_P`, `KEY_A`–`KEY_L`,
/// `KEY_Z`–`KEY_M`): a device reporting any of them is a keyboard and is never opened.
const KEYBOARD_KEYS: [RangeInclusive<u16>; 4] = [2..=11, 16..=25, 30..=38, 44..=50];

/// Size of `struct input_event`; type, code and value are its last eight bytes.
const EVENT_SIZE: usize = std::mem::size_of::<libc::input_event>();
/// Bits per word in sysfs capability bitmaps (`unsigned long`).
const WORD_BITS: usize = usize::BITS as usize;

/// Event codes that mean `button`. Some mice report the side buttons as `BTN_BACK` /
/// `BTN_FORWARD` or `KEY_BACK` / `KEY_FORWARD`; only devices that passed the mouse filter are
/// read, so the `KEY_*` codes never come from a keyboard.
fn button_codes(button: MouseButton) -> &'static [u16] {
    match button {
        MouseButton::Middle => &[BTN_MIDDLE],
        MouseButton::Back => &[BTN_SIDE, BTN_BACK, KEY_BACK],
        MouseButton::Forward => &[BTN_EXTRA, BTN_FORWARD, KEY_FORWARD],
    }
}

/// Parses a sysfs capability bitmap such as `"1f0000 0 0 0 0"` (hex words, most significant
/// first) into words, least significant first.
fn parse_bitmap(text: &str) -> Option<Vec<u64>> {
    text.split_whitespace()
        .rev()
        .map(|word| u64::from_str_radix(word, 16).ok())
        .collect()
}

fn has_bit(words: &[u64], bit: u16) -> bool {
    let bit = usize::from(bit);
    words
        .get(bit / WORD_BITS)
        .is_some_and(|word| (word >> (bit % WORD_BITS)) & 1 == 1)
}

/// Whether a device with these sysfs `capabilities/key` and `capabilities/rel` bitmaps is a
/// mouse that may be opened; with `button`, it must also report one of that button's codes.
fn is_mouse(key_caps: &str, rel_caps: &str, button: Option<MouseButton>) -> bool {
    let (Some(keys), Some(rel)) = (parse_bitmap(key_caps), parse_bitmap(rel_caps)) else {
        return false;
    };
    let keyboard = KEYBOARD_KEYS
        .iter()
        .flat_map(Clone::clone)
        .any(|code| has_bit(&keys, code));
    let reports_button = button.is_none_or(|button| {
        button_codes(button)
            .iter()
            .any(|&code| has_bit(&keys, code))
    });
    has_bit(&keys, BTN_LEFT)
        && (has_bit(&rel, REL_X) || has_bit(&rel, REL_Y))
        && !keyboard
        && reports_button
}

/// `/dev/input/event*` nodes of the mice under `sys_root` (see [`is_mouse`]). Reads sysfs only.
fn mice(sys_root: &Path, button: Option<MouseButton>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(sys_root) else {
        return Vec::new();
    };
    let mut nodes: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("event"))
        .filter(|entry| {
            let caps = entry.path().join("device/capabilities");
            let read = |name: &str| std::fs::read_to_string(caps.join(name)).ok();
            matches!((read("key"), read("rel")), (Some(key), Some(rel)) if is_mouse(&key, &rel, button))
        })
        .map(|entry| Path::new(DEV_INPUT).join(entry.file_name()))
        .collect();
    nodes.sort();
    nodes
}

/// Whether one mouse node may be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Readable,
    Denied,
    /// Unplugged meanwhile, or another error unrelated to permissions.
    Unavailable,
}

impl Access {
    fn of<T>(result: &io::Result<T>) -> Self {
        match result {
            Ok(_) => Self::Readable,
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => Self::Denied,
            Err(_) => Self::Unavailable,
        }
    }

    /// Checks read permission with `access(2)`, without opening the device.
    fn check(node: &Path) -> Self {
        let Ok(path) = CString::new(node.as_os_str().as_bytes()) else {
            return Self::Unavailable;
        };
        // SAFETY: `path` is a valid NUL-terminated string for the duration of the call.
        let result = if unsafe { libc::access(path.as_ptr(), libc::R_OK) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        };
        Self::of(&result)
    }
}

/// Overall state from the open attempts on all mice: one readable mouse is enough; mice that
/// exist but are all refused mean the `input` group is missing.
fn classify(outcomes: impl IntoIterator<Item = Access>) -> PttMouseState {
    let mut denied = false;
    for outcome in outcomes {
        match outcome {
            Access::Readable => return PttMouseState::Ready,
            Access::Denied => denied = true,
            Access::Unavailable => {}
        }
    }
    if denied {
        PttMouseState::InputGroupMissing
    } else {
        PttMouseState::NoDevice
    }
}

fn open(node: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(node)
}

/// Checks whether mice could be read, without opening any device (works with the setting off).
pub fn probe() -> PttMouseState {
    classify(
        mice(Path::new(SYS_INPUT), None)
            .iter()
            .map(|node| Access::check(node)),
    )
}

/// Merges the press state of several mice into one push-to-talk state.
#[derive(Default)]
struct PressTracker {
    held: HashSet<PathBuf>,
}

impl PressTracker {
    /// Records a press or release on `device`; returns the combined state if it changed.
    fn update(&mut self, device: &Path, pressed: bool) -> Option<bool> {
        let before = !self.held.is_empty();
        if pressed {
            self.held.insert(device.to_owned());
        } else {
            self.held.remove(device);
        }
        let after = !self.held.is_empty();
        (before != after).then_some(after)
    }
}

/// Decodes one raw `input_event`: `Some(pressed)` for a press or release of one of `codes`.
/// Auto-repeat (value 2) and every other event are ignored.
fn decode(raw: &[u8; EVENT_SIZE], codes: &[u16]) -> Option<bool> {
    let (_time, tail) = raw.split_last_chunk::<8>()?;
    let kind = u16::from_ne_bytes([tail[0], tail[1]]);
    let code = u16::from_ne_bytes([tail[2], tail[3]]);
    let value = i32::from_ne_bytes([tail[4], tail[5], tail[6], tail[7]]);
    if kind != EV_KEY || !codes.contains(&code) {
        return None;
    }
    match value {
        1 => Some(true),
        0 => Some(false),
        _ => None,
    }
}

/// Listens while alive; dropping it closes all devices.
pub struct Listener {
    task: JoinHandle<()>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Opens the mice that report `button` and starts listening. Fails with
/// [`StartError::NeedsInputGroup`] when mice exist but none may be read. Without any mouse it
/// still starts, so a mouse plugged in later works.
pub fn start(button: MouseButton, on_change: OnChange) -> Result<Listener, StartError> {
    let mut opened = Vec::new();
    let mut outcomes = Vec::new();
    for node in mice(Path::new(SYS_INPUT), Some(button)) {
        let result = open(&node);
        outcomes.push(Access::of(&result));
        if let Ok(file) = result {
            opened.push((node, file));
        }
    }
    if classify(outcomes) == PttMouseState::InputGroupMissing {
        return Err(StartError::NeedsInputGroup);
    }
    tracing::info!(
        "mouse push-to-talk: reading {} mouse device(s) via evdev",
        opened.len()
    );
    let task = async_runtime::spawn(supervise(button, on_change, opened));
    Ok(Listener { task })
}

type Tracker = Arc<Mutex<PressTracker>>;

fn track(tracker: &Tracker, device: &Path, pressed: bool) -> Option<bool> {
    tracker
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .update(device, pressed)
}

/// Runs one reader per mouse and rescans for new mice; aborting it drops all readers.
async fn supervise(button: MouseButton, on_change: OnChange, initial: Vec<(PathBuf, File)>) {
    let codes = button_codes(button);
    let tracker = Tracker::default();
    let mut readers = JoinSet::new();
    let mut watched = HashSet::new();
    for (node, file) in initial {
        watched.insert(node.clone());
        readers.spawn(read_device(
            node,
            file,
            codes,
            tracker.clone(),
            on_change.clone(),
        ));
    }
    let mut rescan = tokio::time::interval(RESCAN);
    rescan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            Some(done) = readers.join_next() => {
                if let Ok(node) = done {
                    watched.remove(&node);
                    if let Some(pressed) = track(&tracker, &node, false) {
                        on_change(pressed);
                    }
                    tracing::info!("mouse push-to-talk: a mouse was disconnected");
                }
            }
            _ = rescan.tick() => {
                for node in mice(Path::new(SYS_INPUT), Some(button)) {
                    if watched.contains(&node) {
                        continue;
                    }
                    // Permissions of a fresh node may lag behind; the next rescan retries.
                    if let Ok(file) = open(&node) {
                        tracing::info!("mouse push-to-talk: a mouse was connected");
                        watched.insert(node.clone());
                        readers.spawn(read_device(
                            node,
                            file,
                            codes,
                            tracker.clone(),
                            on_change.clone(),
                        ));
                    }
                }
            }
        }
    }
}

/// Reads one mouse until it disappears; returns its node.
async fn read_device(
    node: PathBuf,
    file: File,
    codes: &'static [u16],
    tracker: Tracker,
    on_change: OnChange,
) -> PathBuf {
    // SAFETY: the `File` is moved into the `AsyncFd` and owns its descriptor, which therefore
    // stays open and unchanged for the `AsyncFd`'s whole lifetime.
    let Ok(fd) = (unsafe { AsyncFd::register(file) }) else {
        return node;
    };
    let mut buf = [0u8; EVENT_SIZE * 32];
    loop {
        let Ok(mut guard) = fd.readable().await else {
            break;
        };
        let read = guard.try_io(|inner| {
            let mut file: &File = inner.get_ref();
            file.read(&mut buf)
        });
        let n = match read {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => n,
            Ok(Err(err)) if err.kind() == io::ErrorKind::Interrupted => continue,
            // ENODEV once the mouse is unplugged.
            Ok(Err(_)) => break,
            Err(_would_block) => continue,
        };
        for raw in buf[..n].as_chunks::<EVENT_SIZE>().0 {
            if let Some(pressed) = decode(raw, codes)
                && let Some(changed) = track(&tracker, &node, pressed)
            {
                on_change(changed);
            }
        }
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real sysfs bitmaps from a Razer Basilisk V3 Pro and a NuPhy Halo65 keyboard.
    const RAZER_MOUSE_KEY: &str = "1f0000 0 0 0 0";
    const RAZER_MOUSE_REL: &str = "1943";
    const RAZER_KEYBOARD_KEY: &str = "733eff 0 0 483ffff17aff32d bfd4444600000000 1 130c730b17c007 ffbf7bfad941dfff febeffdfffefffff fffffffffffffffe";
    const RAZER_KEYBOARD_REL: &str = "1040";
    const NUPHY_COMBINED_KEY: &str = "3f00733fff 0 0 483ffff17aff32d bfd4444600000000 1f0001 130ff38b17d000 ffe77bfad941dfed 81beffcd01cfffff febffbffdffffffe";
    const NUPHY_COMBINED_REL: &str = "1943";
    const POWER_BUTTON_KEY: &str = "8000 10000000000000 0";

    #[test]
    fn parses_bitmaps_least_significant_word_first() {
        assert_eq!(
            parse_bitmap("1f0000 0 0 0 0"),
            Some(vec![0, 0, 0, 0, 0x1f_0000])
        );
        assert_eq!(parse_bitmap("0"), Some(vec![0]));
        assert_eq!(parse_bitmap("zz"), None);
        let words = parse_bitmap(RAZER_MOUSE_KEY).unwrap_or_default();
        for bit in [BTN_LEFT, BTN_MIDDLE, BTN_SIDE, BTN_EXTRA] {
            assert!(has_bit(&words, bit), "{bit:#x}");
        }
        assert!(!has_bit(&words, BTN_BACK));
        assert!(!has_bit(&words, 0xffff), "out of range");
    }

    #[test]
    fn opens_only_mice() {
        assert!(is_mouse(RAZER_MOUSE_KEY, RAZER_MOUSE_REL, None));
        assert!(is_mouse(
            RAZER_MOUSE_KEY,
            RAZER_MOUSE_REL,
            Some(MouseButton::Back)
        ));
        // Keyboard interfaces report BTN_LEFT too, but have letter keys.
        assert!(!is_mouse(RAZER_KEYBOARD_KEY, RAZER_KEYBOARD_REL, None));
        assert!(!is_mouse(NUPHY_COMBINED_KEY, NUPHY_COMBINED_REL, None));
        assert!(!is_mouse(POWER_BUTTON_KEY, "0", None));
        // BTN_LEFT without relative motion (e.g. a touchscreen) is no mouse.
        assert!(!is_mouse(RAZER_MOUSE_KEY, "0", None));
        assert!(!is_mouse("", "", None));
        assert!(!is_mouse("garbage", "3", None));
    }

    #[test]
    fn needs_the_configured_button() {
        // BTN_LEFT, BTN_RIGHT, BTN_MIDDLE only.
        let three_buttons = "70000 0 0 0 0";
        assert!(is_mouse(three_buttons, "3", Some(MouseButton::Middle)));
        assert!(!is_mouse(three_buttons, "3", Some(MouseButton::Back)));
        // KEY_BACK / KEY_FORWARD (bits 158/159) on a mouse count as side buttons.
        let key_back_forward = "30000 0 c0000000 0 0";
        assert!(is_mouse(key_back_forward, "3", Some(MouseButton::Back)));
        assert!(is_mouse(key_back_forward, "3", Some(MouseButton::Forward)));
    }

    #[test]
    fn classifies_permission_state() {
        assert_eq!(classify([]), PttMouseState::NoDevice);
        assert_eq!(classify([Access::Unavailable]), PttMouseState::NoDevice);
        assert_eq!(
            classify([Access::Denied, Access::Denied]),
            PttMouseState::InputGroupMissing
        );
        assert_eq!(
            classify([Access::Denied, Access::Readable]),
            PttMouseState::Ready
        );
    }

    #[test]
    fn access_distinguishes_permission_errors() {
        let denied: io::Result<()> = Err(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(Access::of(&denied), Access::Denied);
        let gone: io::Result<()> = Err(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(Access::of(&gone), Access::Unavailable);
        assert_eq!(Access::of(&Ok(())), Access::Readable);
        assert_eq!(
            Access::check(Path::new("/nonexistent/event0")),
            Access::Unavailable
        );
    }

    fn event(kind: u16, code: u16, value: i32) -> [u8; EVENT_SIZE] {
        let mut raw = [0u8; EVENT_SIZE];
        raw[EVENT_SIZE - 8..EVENT_SIZE - 6].copy_from_slice(&kind.to_ne_bytes());
        raw[EVENT_SIZE - 6..EVENT_SIZE - 4].copy_from_slice(&code.to_ne_bytes());
        raw[EVENT_SIZE - 4..].copy_from_slice(&value.to_ne_bytes());
        raw
    }

    #[test]
    fn decodes_only_the_configured_button() {
        let back = button_codes(MouseButton::Back);
        assert_eq!(decode(&event(EV_KEY, BTN_SIDE, 1), back), Some(true));
        assert_eq!(decode(&event(EV_KEY, BTN_SIDE, 0), back), Some(false));
        assert_eq!(decode(&event(EV_KEY, KEY_BACK, 1), back), Some(true));
        assert_eq!(decode(&event(EV_KEY, BTN_SIDE, 2), back), None, "repeat");
        assert_eq!(decode(&event(EV_KEY, BTN_LEFT, 1), back), None);
        assert_eq!(decode(&event(EV_KEY, BTN_EXTRA, 1), back), None);
        assert_eq!(decode(&event(0x02, BTN_SIDE, 1), back), None, "EV_REL");
        let forward = button_codes(MouseButton::Forward);
        assert_eq!(decode(&event(EV_KEY, BTN_EXTRA, 1), forward), Some(true));
        assert_eq!(decode(&event(EV_KEY, KEY_FORWARD, 0), forward), Some(false));
        let middle = button_codes(MouseButton::Middle);
        assert_eq!(decode(&event(EV_KEY, BTN_MIDDLE, 1), middle), Some(true));
    }

    #[test]
    fn merges_presses_of_several_mice() {
        let (a, b) = (
            Path::new("/dev/input/event2"),
            Path::new("/dev/input/event4"),
        );
        let mut tracker = PressTracker::default();
        assert_eq!(tracker.update(a, true), Some(true));
        assert_eq!(tracker.update(b, true), None);
        assert_eq!(tracker.update(a, false), None);
        assert_eq!(tracker.update(b, false), Some(false));
        assert_eq!(tracker.update(b, false), None);
    }

    /// Local check on a real machine: without `input` group membership the evdev path must
    /// report the missing permission instead of failing silently.
    #[test]
    #[ignore = "depends on the local mice and group membership"]
    fn local_probe() {
        let state = probe();
        println!("evdev probe: {state:?}");
        assert_ne!(state, PttMouseState::Unsupported);
    }
}
