//! Converts the official client's push-to-talk buttons into Mumcord's key format
//! (`"Ctrl+Space"`, `"F13"`, `"Mouse4"`; see `ptt::normalize_key` in the app shell).
//!
//! Mumble stores raw, platform-specific codes: X11 keycodes on Linux, scan codes on Windows
//! (as `InputKeyboard` since 1.5, as a DirectInput pair before) and virtual key codes plus
//! modifier and mouse offsets on macOS.

use super::Platform;
use super::qt::Variant;

/// Why a Mumble shortcut cannot become a Mumcord push-to-talk key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyIssue {
    /// Only modifiers (e.g. Right Ctrl alone), which Mumcord does not allow as a key.
    ModifierOnly,
    /// More than one ordinary key, or a mouse button combined with keys.
    TooManyKeys,
    /// A key, game controller or device Mumcord does not know.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

enum Button {
    Modifier(Modifier),
    Key(&'static str),
    Mouse(&'static str),
}

/// The Mumcord key for a list of Mumble buttons pressed together.
pub fn ptt_key(platform: Platform, buttons: &[Variant]) -> Result<String, KeyIssue> {
    let mut modifiers = Vec::new();
    let mut keys = Vec::new();
    let mut mouse = Vec::new();
    for button in buttons {
        match button_of(platform, button).ok_or(KeyIssue::Unknown)? {
            Button::Modifier(m) => modifiers.push(m),
            Button::Key(k) => keys.push(k),
            Button::Mouse(m) => mouse.push(m),
        }
    }
    match (keys.as_slice(), mouse.as_slice()) {
        ([], [m]) if modifiers.is_empty() => Ok((*m).to_owned()),
        ([], []) if !modifiers.is_empty() => Err(KeyIssue::ModifierOnly),
        ([key], []) => {
            let mut parts: Vec<&str> = [
                (Modifier::Ctrl, "Ctrl"),
                (Modifier::Alt, "Alt"),
                (Modifier::Shift, "Shift"),
                (Modifier::Super, "Super"),
            ]
            .iter()
            .filter(|(m, _)| modifiers.contains(m))
            .map(|(_, name)| *name)
            .collect();
            parts.push(key);
            Ok(parts.join("+"))
        }
        ([], []) => Err(KeyIssue::Unknown),
        _ => Err(KeyIssue::TooManyKeys),
    }
}

fn button_of(platform: Platform, v: &Variant) -> Option<Button> {
    match platform {
        Platform::Linux => x11(u32::try_from(unsigned(v)?).ok()?),
        Platform::Macos => macos(u32::try_from(unsigned(v)?).ok()?),
        Platform::Windows => windows(v),
    }
}

fn unsigned(v: &Variant) -> Option<u64> {
    match v {
        Variant::UInt(u) => Some(*u),
        Variant::Int(i) => u64::try_from(*i).ok(),
        // The INI file stores plain numbers as text.
        Variant::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// X11 keycodes are Linux evdev codes plus 8; mouse buttons are `0x117 + X button`.
fn x11(code: u32) -> Option<Button> {
    let mouse = match code {
        0x119 => Some("MouseMiddle"),
        0x11F => Some("Mouse4"),
        0x120 => Some("Mouse5"),
        0x118..0x128 => return None,
        _ => None,
    };
    if let Some(m) = mouse {
        return Some(Button::Mouse(m));
    }
    let evdev = code.checked_sub(8)?;
    Some(match evdev {
        29 | 97 => Button::Modifier(Modifier::Ctrl),
        42 | 54 => Button::Modifier(Modifier::Shift),
        56 | 100 => Button::Modifier(Modifier::Alt),
        125 | 126 => Button::Modifier(Modifier::Super),
        _ => Button::Key(evdev_key(evdev)?),
    })
}

const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];
const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const F_KEYS: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

fn letter(c: u8) -> &'static str {
    LETTERS[usize::from(c - b'A')]
}

fn f_key(n: u32) -> Option<&'static str> {
    F_KEYS
        .get(usize::try_from(n.checked_sub(1)?).ok()?)
        .copied()
}

/// Linux input event codes (`linux/input-event-codes.h`).
fn evdev_key(code: u32) -> Option<&'static str> {
    const ROW_Q: &[u8] = b"QWERTYUIOP";
    const ROW_A: &[u8] = b"ASDFGHJKL";
    const ROW_Z: &[u8] = b"ZXCVBNM";
    let index = |start: u32| usize::try_from(code - start).unwrap_or(usize::MAX);
    Some(match code {
        1 => "Escape",
        2..=10 => DIGITS[index(1)],
        11 => "0",
        12 => "Minus",
        13 => "Equal",
        14 => "Backspace",
        15 => "Tab",
        16..=25 => letter(ROW_Q[index(16)]),
        26 => "BracketLeft",
        27 => "BracketRight",
        28 => "Enter",
        30..=38 => letter(ROW_A[index(30)]),
        39 => "Semicolon",
        40 => "Quote",
        41 => "Backquote",
        43 => "Backslash",
        44..=50 => letter(ROW_Z[index(44)]),
        51 => "Comma",
        52 => "Period",
        53 => "Slash",
        55 => "NumpadMultiply",
        57 => "Space",
        58 => "CapsLock",
        59..=68 => f_key(code - 58)?,
        69 => "NumLock",
        70 => "ScrollLock",
        71 => "Numpad7",
        72 => "Numpad8",
        73 => "Numpad9",
        74 => "NumpadSubtract",
        75 => "Numpad4",
        76 => "Numpad5",
        77 => "Numpad6",
        78 => "NumpadAdd",
        79 => "Numpad1",
        80 => "Numpad2",
        81 => "Numpad3",
        82 => "Numpad0",
        83 => "NumpadDecimal",
        87 => "F11",
        88 => "F12",
        96 => "NumpadEnter",
        98 => "NumpadDivide",
        99 => "PrintScreen",
        102 => "Home",
        103 => "ArrowUp",
        104 => "PageUp",
        105 => "ArrowLeft",
        106 => "ArrowRight",
        107 => "End",
        108 => "ArrowDown",
        109 => "PageDown",
        110 => "Insert",
        111 => "Delete",
        113 => "AudioVolumeMute",
        114 => "AudioVolumeDown",
        115 => "AudioVolumeUp",
        117 => "NumpadEqual",
        119 => "Pause",
        164 => "MediaPlayPause",
        183..=194 => f_key(code - 170)?,
        _ => return None,
    })
}

/// Mumble's Windows `InputMouse`: 3 = middle, 4 = side 1 (back), 5 = side 2 (forward).
fn windows_mouse(button: i64) -> Option<Button> {
    match button {
        3 => Some(Button::Mouse("MouseMiddle")),
        4 => Some(Button::Mouse("Mouse4")),
        5 => Some(Button::Mouse("Mouse5")),
        _ => None,
    }
}

/// `{6F1D2B61-D5A0-11CF-BFC7-444553540000}` (`GUID_SysKeyboard`) as Qt streams a `QUuid`.
const KEYBOARD_UUID: [u8; 16] = [
    0x6F, 0x1D, 0x2B, 0x61, 0xD5, 0xA0, 0x11, 0xCF, 0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00,
];
/// `GUID_SysMouse`.
const MOUSE_UUID: [u8; 16] = [
    0x6F, 0x1D, 0x2B, 0x60, 0xD5, 0xA0, 0x11, 0xCF, 0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00,
];

fn windows(v: &Variant) -> Option<Button> {
    match v {
        Variant::WinKeyboard { e0, code } => scan_code(*e0, *code),
        Variant::WinMouse(m) => windows_mouse(i64::from(*m)),
        // Before 1.5: a DirectInput pair of value and device GUID; converted like Mumble's
        // own migration (`GlobalShortcutWin`).
        Variant::List(pair) => match pair.as_slice() {
            [value, Variant::Uuid(uuid)] => {
                let value = u32::try_from(unsigned(value)?).ok()?;
                if *uuid == KEYBOARD_UUID {
                    let code = u16::try_from((value & !0x8000) >> 8).ok()?;
                    let mut e0 = value & 0x8000 != 0;
                    // DirectInput and raw input disagree on the extended bit of this code.
                    if code == 0x45 {
                        e0 = !e0;
                    }
                    scan_code(e0, code)
                } else if *uuid == MOUSE_UUID {
                    windows_mouse(i64::from(value >> 8) - 2)
                } else {
                    None
                }
            }
            _ => None,
        },
        _ => None,
    }
}

/// PC scan codes (set 1) as Windows raw input reports them.
fn scan_code(e0: bool, code: u16) -> Option<Button> {
    const ROW_Q: &[u8] = b"QWERTYUIOP";
    const ROW_A: &[u8] = b"ASDFGHJKL";
    const ROW_Z: &[u8] = b"ZXCVBNM";
    let index = |start: u16| usize::from(code - start);
    let key = match (e0, code) {
        (_, 0x1D) => return Some(Button::Modifier(Modifier::Ctrl)),
        (false, 0x2A | 0x36) => return Some(Button::Modifier(Modifier::Shift)),
        (_, 0x38) => return Some(Button::Modifier(Modifier::Alt)),
        (true, 0x5B | 0x5C) => return Some(Button::Modifier(Modifier::Super)),
        (false, 0x01) => "Escape",
        (false, 0x02..=0x0A) => DIGITS[index(0x01)],
        (false, 0x0B) => "0",
        (false, 0x0C) => "Minus",
        (false, 0x0D) => "Equal",
        (false, 0x0E) => "Backspace",
        (false, 0x0F) => "Tab",
        (false, 0x10..=0x19) => letter(ROW_Q[index(0x10)]),
        (false, 0x1A) => "BracketLeft",
        (false, 0x1B) => "BracketRight",
        (false, 0x1C) => "Enter",
        (true, 0x1C) => "NumpadEnter",
        (false, 0x1E..=0x26) => letter(ROW_A[index(0x1E)]),
        (false, 0x27) => "Semicolon",
        (false, 0x28) => "Quote",
        (false, 0x29) => "Backquote",
        (false, 0x2B) => "Backslash",
        (false, 0x2C..=0x32) => letter(ROW_Z[index(0x2C)]),
        (false, 0x33) => "Comma",
        (false, 0x34) => "Period",
        (false, 0x35) => "Slash",
        (true, 0x35) => "NumpadDivide",
        (false, 0x37) => "NumpadMultiply",
        (true, 0x37) => "PrintScreen",
        (false, 0x39) => "Space",
        (false, 0x3A) => "CapsLock",
        (false, 0x3B..=0x44) => f_key(u32::from(code - 0x3A))?,
        (true, 0x45) => "NumLock",
        (false, 0x45) => "Pause",
        (false, 0x46) => "ScrollLock",
        (false, 0x47) => "Numpad7",
        (true, 0x47) => "Home",
        (false, 0x48) => "Numpad8",
        (true, 0x48) => "ArrowUp",
        (false, 0x49) => "Numpad9",
        (true, 0x49) => "PageUp",
        (false, 0x4A) => "NumpadSubtract",
        (false, 0x4B) => "Numpad4",
        (true, 0x4B) => "ArrowLeft",
        (false, 0x4C) => "Numpad5",
        (false, 0x4D) => "Numpad6",
        (true, 0x4D) => "ArrowRight",
        (false, 0x4E) => "NumpadAdd",
        (false, 0x4F) => "Numpad1",
        (true, 0x4F) => "End",
        (false, 0x50) => "Numpad2",
        (true, 0x50) => "ArrowDown",
        (false, 0x51) => "Numpad3",
        (true, 0x51) => "PageDown",
        (false, 0x52) => "Numpad0",
        (true, 0x52) => "Insert",
        (false, 0x53) => "NumpadDecimal",
        (true, 0x53) => "Delete",
        (false, 0x57) => "F11",
        (false, 0x58) => "F12",
        (false, 0x64..=0x6E) => f_key(u32::from(code - 0x57))?,
        (false, 0x76) => "F24",
        (true, 0x20) => "AudioVolumeMute",
        (true, 0x2E) => "AudioVolumeDown",
        (true, 0x30) => "AudioVolumeUp",
        (true, 0x22) => "MediaPlayPause",
        _ => return None,
    };
    Some(Button::Key(key))
}

/// Mumble's macOS offsets for modifier flags and mouse buttons (`GlobalShortcut_macx.mm`).
const MAC_MOD_OFFSET: u32 = 0x10000;
const MAC_MOUSE_OFFSET: u32 = 0x20000;

/// macOS virtual key codes (`kVK_*` in `HIToolbox/Events.h`).
fn macos(code: u32) -> Option<Button> {
    if let Some(m) = code.checked_sub(MAC_MOUSE_OFFSET) {
        return match m {
            2 => Some(Button::Mouse("MouseMiddle")),
            3 => Some(Button::Mouse("Mouse4")),
            4 => Some(Button::Mouse("Mouse5")),
            _ => None,
        };
    }
    if let Some(m) = code.checked_sub(MAC_MOD_OFFSET) {
        return match m {
            1 => Some(Button::Modifier(Modifier::Shift)),
            2 => Some(Button::Modifier(Modifier::Ctrl)),
            3 => Some(Button::Modifier(Modifier::Alt)),
            4 => Some(Button::Modifier(Modifier::Super)),
            _ => None,
        };
    }
    let key = match code {
        0x00 => "A",
        0x01 => "S",
        0x02 => "D",
        0x03 => "F",
        0x04 => "H",
        0x05 => "G",
        0x06 => "Z",
        0x07 => "X",
        0x08 => "C",
        0x09 => "V",
        0x0B => "B",
        0x0C => "Q",
        0x0D => "W",
        0x0E => "E",
        0x0F => "R",
        0x10 => "Y",
        0x11 => "T",
        0x12 => "1",
        0x13 => "2",
        0x14 => "3",
        0x15 => "4",
        0x16 => "6",
        0x17 => "5",
        0x18 => "Equal",
        0x19 => "9",
        0x1A => "7",
        0x1B => "Minus",
        0x1C => "8",
        0x1D => "0",
        0x1E => "BracketRight",
        0x1F => "O",
        0x20 => "U",
        0x21 => "BracketLeft",
        0x22 => "I",
        0x23 => "P",
        0x24 => "Enter",
        0x25 => "L",
        0x26 => "J",
        0x27 => "Quote",
        0x28 => "K",
        0x29 => "Semicolon",
        0x2A => "Backslash",
        0x2B => "Comma",
        0x2C => "Slash",
        0x2D => "N",
        0x2E => "M",
        0x2F => "Period",
        0x30 => "Tab",
        0x31 => "Space",
        0x32 => "Backquote",
        0x33 => "Backspace",
        0x35 => "Escape",
        0x41 => "NumpadDecimal",
        0x43 => "NumpadMultiply",
        0x45 => "NumpadAdd",
        0x48 => "AudioVolumeUp",
        0x49 => "AudioVolumeDown",
        0x4A => "AudioVolumeMute",
        0x4B => "NumpadDivide",
        0x4C => "NumpadEnter",
        0x4E => "NumpadSubtract",
        0x4F => "F18",
        0x50 => "F19",
        0x51 => "NumpadEqual",
        0x52..=0x59 => NUMPAD[usize::try_from(code - 0x52).ok()?],
        0x5A => "F20",
        0x5B => "Numpad8",
        0x5C => "Numpad9",
        0x60 => "F5",
        0x61 => "F6",
        0x62 => "F7",
        0x63 => "F3",
        0x64 => "F8",
        0x65 => "F9",
        0x67 => "F11",
        0x69 => "F13",
        0x6A => "F16",
        0x6B => "F14",
        0x6D => "F10",
        0x6F => "F12",
        0x71 => "F15",
        0x72 => "Insert",
        0x73 => "Home",
        0x74 => "PageUp",
        0x75 => "Delete",
        0x76 => "F4",
        0x77 => "End",
        0x78 => "F2",
        0x79 => "PageDown",
        0x7A => "F1",
        0x7B => "ArrowLeft",
        0x7C => "ArrowRight",
        0x7D => "ArrowDown",
        0x7E => "ArrowUp",
        _ => return None,
    };
    Some(Button::Key(key))
}

/// `kVK_ANSI_Keypad0` … `kVK_ANSI_Keypad7` (0x52–0x59).
const NUMPAD: [&str; 8] = [
    "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4", "Numpad5", "Numpad6", "Numpad7",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn uints(codes: &[u64]) -> Vec<Variant> {
        codes.iter().map(|c| Variant::UInt(*c)).collect()
    }

    #[test]
    fn x11_keycodes_become_mumcord_keys() {
        // Left Ctrl (37) + Space (65).
        assert_eq!(
            ptt_key(Platform::Linux, &uints(&[37, 65])),
            Ok("Ctrl+Space".into())
        );
        // F13 = evdev 183 + 8; letters and digits.
        assert_eq!(ptt_key(Platform::Linux, &uints(&[191])), Ok("F13".into()));
        assert_eq!(ptt_key(Platform::Linux, &uints(&[38])), Ok("A".into()));
        assert_eq!(ptt_key(Platform::Linux, &uints(&[10])), Ok("1".into()));
        assert_eq!(
            ptt_key(Platform::Linux, &uints(&[0x11F])),
            Ok("Mouse4".into())
        );
        // Text, as in the 1.4 INI file.
        assert_eq!(
            ptt_key(
                Platform::Linux,
                &[Variant::String("50".into()), Variant::String("38".into())]
            ),
            Ok("Shift+A".into())
        );
    }

    #[test]
    fn unsupported_combinations_are_explained() {
        assert_eq!(
            ptt_key(Platform::Linux, &uints(&[105])),
            Err(KeyIssue::ModifierOnly)
        );
        assert_eq!(
            ptt_key(Platform::Linux, &uints(&[38, 39])),
            Err(KeyIssue::TooManyKeys)
        );
        assert_eq!(
            ptt_key(Platform::Linux, &uints(&[37, 0x11F])),
            Err(KeyIssue::TooManyKeys)
        );
        assert_eq!(
            ptt_key(Platform::Linux, &uints(&[0x118])),
            Err(KeyIssue::Unknown)
        );
        assert_eq!(
            ptt_key(Platform::Windows, &[Variant::User("InputXinput".into())]),
            Err(KeyIssue::Unknown)
        );
        assert_eq!(ptt_key(Platform::Linux, &[]), Err(KeyIssue::Unknown));
    }

    #[test]
    fn windows_scan_codes_in_both_formats() {
        let ctrl_right = Variant::WinKeyboard {
            e0: true,
            code: 0x1D,
        };
        let space = Variant::WinKeyboard {
            e0: false,
            code: 0x39,
        };
        assert_eq!(
            ptt_key(Platform::Windows, &[ctrl_right, space]),
            Ok("Ctrl+Space".into())
        );
        assert_eq!(
            ptt_key(Platform::Windows, &[Variant::WinMouse(4)]),
            Ok("Mouse4".into())
        );
        // 1.4: DirectInput value (scan code << 8, 0x8000 = extended) with the keyboard GUID.
        let legacy =
            |value: u64, uuid| Variant::List(vec![Variant::UInt(value), Variant::Uuid(uuid)]);
        assert_eq!(
            ptt_key(Platform::Windows, &[legacy(0x1E00, KEYBOARD_UUID)]),
            Ok("A".into())
        );
        assert_eq!(
            ptt_key(Platform::Windows, &[legacy(0xC800, KEYBOARD_UUID)]),
            Ok("ArrowUp".into())
        );
        // Mouse: (button + 2) << 8; 6 = side 2.
        assert_eq!(
            ptt_key(Platform::Windows, &[legacy(0x0700, MOUSE_UUID)]),
            Ok("Mouse5".into())
        );
    }

    #[test]
    fn macos_key_codes_and_offsets() {
        assert_eq!(
            ptt_key(Platform::Macos, &uints(&[0x10002, 0x31])),
            Ok("Ctrl+Space".into())
        );
        assert_eq!(
            ptt_key(Platform::Macos, &uints(&[0x10004, 0x10001, 0x00])),
            Ok("Shift+Super+A".into())
        );
        assert_eq!(
            ptt_key(Platform::Macos, &uints(&[0x20003])),
            Ok("Mouse4".into())
        );
        assert_eq!(
            ptt_key(Platform::Macos, &uints(&[0x55])),
            Ok("Numpad3".into())
        );
        assert_eq!(ptt_key(Platform::Macos, &uints(&[0x69])), Ok("F13".into()));
    }
}
