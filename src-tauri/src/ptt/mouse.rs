//! Mouse buttons as push-to-talk (FR-1.21, QA.4).
//!
//! Stored keys: `MouseMiddle`, `Mouse4` (back side button), `Mouse5` (forward side button).
//! Left and right click are never accepted, and mouse buttons take no modifiers.
//!
//! Backends:
//! - Windows: low-level mouse hook (`mouse_windows`).
//! - macOS: listen-only event tap; asks for the Accessibility permission if needed
//!   (`mouse_macos`).
//! - X11: XInput2 raw button events on the root window, without a grab (`mouse_x11`).
//! - Linux Wayland: evdev, only when the user switched on "Allow mouse buttons (requires
//!   input group)" (`mouse_evdev`). The portal cannot deliver mouse buttons.
//!
//! Button events are passed to [`super::emit`] and never logged.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::KeyError;
use crate::commands::ptt::{PttMouseState, PttMouseStatus, PttMouseSupport, PttUnavailableReason};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Middle,
    /// Side button "back" (`Mouse4`).
    Back,
    /// Side button "forward" (`Mouse5`).
    Forward,
}

impl MouseButton {
    /// The `pttKey` spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Middle => "MouseMiddle",
            Self::Back => "Mouse4",
            Self::Forward => "Mouse5",
        }
    }
}

/// Parses a mouse key such as `"Mouse4"` (case-insensitive). `None` if `raw` does not name a
/// mouse button at all, so it is a keyboard key.
pub fn parse(raw: &str) -> Option<Result<MouseButton, KeyError>> {
    let tokens: Vec<&str> = raw.split('+').map(str::trim).collect();
    let last = tokens.last()?;
    let lower = last.to_ascii_lowercase();
    if !lower.starts_with("mouse") {
        return None;
    }
    if tokens.len() > 1 {
        return Some(Err(KeyError::InvalidFormat));
    }
    Some(match lower.as_str() {
        "mousemiddle" => Ok(MouseButton::Middle),
        "mouse4" => Ok(MouseButton::Back),
        "mouse5" => Ok(MouseButton::Forward),
        "mouseleft" | "mouseright" | "mouse1" | "mouse2" => Err(KeyError::PrimaryMouseButton),
        _ => Err(KeyError::UnknownKey((*last).to_owned())),
    })
}

/// Receives button state changes (`true` = held) from a backend.
pub type OnChange = Arc<dyn Fn(bool) + Send + Sync>;

/// Why a backend could not start.
#[derive(Debug)]
pub enum StartError {
    /// Linux Wayland: mice exist, but none can be opened (not in the `input` group).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NeedsInputGroup,
    /// macOS: the event tap was refused and the process is not trusted for Accessibility.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NeedsAccessibility,
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedsInputGroup => f.write_str("mice cannot be read (not in the input group)"),
            Self::NeedsAccessibility => f.write_str("the Accessibility permission is missing"),
            Self::Failed(detail) => f.write_str(detail),
        }
    }
}

impl StartError {
    fn reason(&self) -> PttUnavailableReason {
        match self {
            Self::NeedsInputGroup => PttUnavailableReason::MouseNeedsInputGroup,
            Self::NeedsAccessibility => PttUnavailableReason::MouseNeedsAccessibility,
            Self::Failed(_) => PttUnavailableReason::RegistrationFailed,
        }
    }
}

/// How mouse buttons are read in this session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    #[cfg_attr(
        not(any(target_os = "linux", target_os = "windows", target_os = "macos")),
        allow(dead_code)
    )]
    Native,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Evdev,
    #[cfg_attr(
        any(target_os = "linux", target_os = "windows", target_os = "macos"),
        allow(dead_code)
    )]
    Unsupported,
}

fn kind() -> Kind {
    #[cfg(target_os = "linux")]
    {
        match super::current_backend() {
            super::Backend::Portal => Kind::Evdev,
            super::Backend::GlobalShortcut => Kind::Native,
        }
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        Kind::Native
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Kind::Unsupported
    }
}

struct Active {
    button: MouseButton,
    /// The backend's listener; dropping it stops listening.
    _listener: Box<dyn Send>,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static LAST_FAILURE: Mutex<Option<PttUnavailableReason>> = Mutex::new(None);
/// "Allow mouse buttons (requires input group)"; only consulted on Linux Wayland.
static EVDEV_ALLOWED: AtomicBool = AtomicBool::new(false);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Applies the user's opt-in for reading input devices on Wayland (FR-1.39).
pub fn set_evdev_allowed(allowed: bool) {
    EVDEV_ALLOWED.store(allowed, Ordering::SeqCst);
}

/// Whether a mouse button may be captured as the push-to-talk key right now.
pub fn capturable() -> bool {
    match kind() {
        Kind::Native => true,
        Kind::Evdev => EVDEV_ALLOWED.load(Ordering::SeqCst),
        Kind::Unsupported => false,
    }
}

/// Starts listening for `button` (keeps an already running listener for the same button).
pub fn start(button: MouseButton) -> Result<(), PttUnavailableReason> {
    let kind = kind();
    if kind == Kind::Evdev && !EVDEV_ALLOWED.load(Ordering::SeqCst) {
        stop();
        return Err(PttUnavailableReason::MouseDisabled);
    }
    let mut active = lock(&ACTIVE);
    if active.as_ref().is_some_and(|a| a.button == button) {
        return Ok(());
    }
    if active.take().is_some() {
        super::emit(false);
    }
    let on_change: OnChange = Arc::new(super::emit);
    let result = match kind {
        Kind::Native => start_native(button, on_change),
        Kind::Evdev => start_evdev(button, on_change),
        Kind::Unsupported => Err(StartError::Failed(
            "mouse buttons are not supported on this platform".to_owned(),
        )),
    };
    match result {
        Ok(listener) => {
            *active = Some(Active {
                button,
                _listener: listener,
            });
            *lock(&LAST_FAILURE) = None;
            Ok(())
        }
        Err(err) => {
            tracing::warn!("mouse push-to-talk unavailable: {err}");
            let reason = err.reason();
            *lock(&LAST_FAILURE) = Some(reason);
            Err(reason)
        }
    }
}

/// Stops listening and releases push-to-talk if the button was held.
pub fn stop() {
    let stopped = lock(&ACTIVE).take().is_some();
    *lock(&LAST_FAILURE) = None;
    if stopped {
        super::emit(false);
    }
}

/// Mouse-button support and its current state for the UI (AC-1.37). On Wayland the UI
/// combines it with the `allowMouseButtons` setting.
pub fn status() -> PttMouseStatus {
    let last_failure = *lock(&LAST_FAILURE);
    match kind() {
        Kind::Native => PttMouseStatus {
            support: PttMouseSupport::Native,
            state: native_state(last_failure),
        },
        // Independent of the opt-in, so the UI can explain the permission before and after
        // the user switches it on; checking never opens a device.
        Kind::Evdev => PttMouseStatus {
            support: PttMouseSupport::Evdev,
            state: evdev_state(),
        },
        Kind::Unsupported => PttMouseStatus {
            support: PttMouseSupport::None,
            state: PttMouseState::Unsupported,
        },
    }
}

fn native_state(last_failure: Option<PttUnavailableReason>) -> PttMouseState {
    match last_failure {
        Some(PttUnavailableReason::MouseNeedsAccessibility) => PttMouseState::AccessibilityMissing,
        _ => PttMouseState::Ready,
    }
}

#[cfg(target_os = "linux")]
fn start_native(button: MouseButton, on_change: OnChange) -> Result<Box<dyn Send>, StartError> {
    super::mouse_x11::start(button, on_change).map(|l| Box::new(l) as Box<dyn Send>)
}

#[cfg(target_os = "windows")]
fn start_native(button: MouseButton, on_change: OnChange) -> Result<Box<dyn Send>, StartError> {
    super::mouse_windows::start(button, on_change).map(|l| Box::new(l) as Box<dyn Send>)
}

#[cfg(target_os = "macos")]
fn start_native(button: MouseButton, on_change: OnChange) -> Result<Box<dyn Send>, StartError> {
    super::mouse_macos::start(button, on_change).map(|l| Box::new(l) as Box<dyn Send>)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn start_native(_button: MouseButton, _on_change: OnChange) -> Result<Box<dyn Send>, StartError> {
    Err(StartError::Failed("no mouse backend".to_owned()))
}

#[cfg(target_os = "linux")]
fn start_evdev(button: MouseButton, on_change: OnChange) -> Result<Box<dyn Send>, StartError> {
    super::mouse_evdev::start(button, on_change).map(|l| Box::new(l) as Box<dyn Send>)
}

#[cfg(not(target_os = "linux"))]
fn start_evdev(_button: MouseButton, _on_change: OnChange) -> Result<Box<dyn Send>, StartError> {
    Err(StartError::Failed("evdev is Linux only".to_owned()))
}

#[cfg(target_os = "linux")]
fn evdev_state() -> PttMouseState {
    super::mouse_evdev::probe()
}

#[cfg(not(target_os = "linux"))]
fn evdev_state() -> PttMouseState {
    PttMouseState::Unsupported
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mouse_keys() {
        assert_eq!(
            parse("Mouse4").map(Result::ok),
            Some(Some(MouseButton::Back))
        );
        assert_eq!(
            parse("mouse5").map(Result::ok),
            Some(Some(MouseButton::Forward))
        );
        assert_eq!(
            parse(" MouseMiddle ").map(Result::ok),
            Some(Some(MouseButton::Middle))
        );
        for button in [MouseButton::Middle, MouseButton::Back, MouseButton::Forward] {
            assert_eq!(parse(button.name()).map(Result::ok), Some(Some(button)));
        }
    }

    #[test]
    fn rejects_primary_buttons_and_modifiers() {
        for primary in ["MouseLeft", "MouseRight", "Mouse1", "mouse2"] {
            assert_eq!(
                parse(primary).map(Result::err),
                Some(Some(KeyError::PrimaryMouseButton)),
                "{primary}"
            );
        }
        assert_eq!(
            parse("Ctrl+Mouse4").map(Result::err),
            Some(Some(KeyError::InvalidFormat))
        );
        assert_eq!(
            parse("Mouse9").map(Result::err),
            Some(Some(KeyError::UnknownKey("Mouse9".into())))
        );
    }

    #[test]
    fn keyboard_keys_are_not_mouse_keys() {
        assert!(parse("KeyM").is_none());
        assert!(parse("Ctrl+F13").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn native_state_reports_missing_accessibility() {
        assert_eq!(native_state(None), PttMouseState::Ready);
        assert_eq!(
            native_state(Some(PttUnavailableReason::MouseNeedsAccessibility)),
            PttMouseState::AccessibilityMissing
        );
        assert_eq!(
            native_state(Some(PttUnavailableReason::RegistrationFailed)),
            PttMouseState::Ready
        );
    }
}
