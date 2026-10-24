//! Global push-to-talk shortcut, including the Wayland portal (FR-1.5, FR-1.21, Q4.11).
//!
//! Backends:
//! - X11, Windows, macOS: `tauri-plugin-global-shortcut`, which reports key press and release.
//! - Wayland sessions: the XDG desktop portal `GlobalShortcuts` interface (via `ashpd`). X11
//!   key grabs through XWayland only fire while one of our own windows is focused, so a
//!   Wayland session always uses the portal.
//! - Mouse buttons (`MouseMiddle`, `Mouse4`, `Mouse5`): [`mouse`] with a native backend per
//!   platform, or evdev on Wayland behind the opt-in setting (QA.4). Keyboard keys never
//!   reach the mouse backend and mouse keys never reach the plugin or the portal.
//!
//! Engine hook: this module knows nothing about audio. The engine installs a sink with
//! [`set_handler`] (typically `move |pressed| controls.set_ptt_pressed(pressed)`) and turns
//! push-to-talk on or off with [`configure`]. The handler only sees real state changes and
//! always gets a final `false` when push-to-talk is switched off while the key is held.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Runtime};

use crate::commands::ptt::{PttAvailability, PttMouseStatus, PttUnavailableReason};
use crate::commands::types::{ApiError, ApiResult, ErrorCode};

mod mouse;
#[cfg(target_os = "linux")]
mod mouse_evdev;
#[cfg(target_os = "macos")]
mod mouse_macos;
#[cfg(target_os = "windows")]
mod mouse_windows;
#[cfg(target_os = "linux")]
mod mouse_x11;

use mouse::MouseButton;

/// Sink for push-to-talk state changes (`true` = key held).
pub type PttHandler = dyn Fn(bool) + Send + Sync;

/// Which mechanism delivers global key events in this session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    GlobalShortcut,
    Portal,
}

#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    Empty,
    /// Only modifiers were pressed; global shortcuts need a main key.
    ModifierOnly,
    UnknownKey(String),
    /// More than one main key, or a modifier after the main key (or with a mouse button).
    InvalidFormat,
    /// Left or right click: never usable as push-to-talk.
    PrimaryMouseButton,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("empty key"),
            Self::ModifierOnly => f.write_str("a key other than a modifier is required"),
            Self::UnknownKey(key) => write!(f, "unsupported key: {key}"),
            Self::InvalidFormat => f.write_str("expected modifiers followed by one key"),
            Self::PrimaryMouseButton => {
                f.write_str("left and right click cannot be used for push-to-talk")
            }
        }
    }
}

struct State {
    handler: Option<Arc<PttHandler>>,
    /// Whether key events are forwarded to the handler (push-to-talk switched on).
    forwarding: bool,
    pressed: bool,
    last_failure: Option<PttUnavailableReason>,
    /// The configured key is a mouse button.
    mouse_key: bool,
    /// Shortcut currently registered with the global-shortcut plugin.
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    registered: Option<String>,
    #[cfg(target_os = "linux")]
    portal: Option<portal::Binding>,
    #[cfg(target_os = "linux")]
    capture_cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

static STATE: Mutex<State> = Mutex::new(State {
    handler: None,
    forwarding: false,
    pressed: false,
    last_failure: None,
    mouse_key: false,
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    registered: None,
    #[cfg(target_os = "linux")]
    portal: None,
    #[cfg(target_os = "linux")]
    capture_cancel: None,
});

/// Serialises (re)binding so concurrent settings changes and captures cannot interleave.
static OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn state() -> MutexGuard<'static, State> {
    // The state stays consistent even if a handler panicked while it was locked.
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Installs the sink that receives push-to-talk state changes; replaces any previous one.
pub fn set_handler(handler: impl Fn(bool) + Send + Sync + 'static) {
    state().handler = Some(Arc::new(handler));
}

/// Forwards a key state change to the handler, dropping repeats and presses while switched off.
fn emit(pressed: bool) {
    let handler = {
        let mut state = state();
        if pressed == state.pressed || (pressed && !state.forwarding) {
            return;
        }
        state.pressed = pressed;
        state.handler.clone()
    };
    if let Some(handler) = handler {
        handler(pressed);
    }
}

/// Switches push-to-talk on with `key` (the stored `pttKey`) or off with `None`.
///
/// On Wayland this may show the portal's consent dialog if the binding is new or changed.
pub async fn configure<R: Runtime>(
    app: &AppHandle<R>,
    key: Option<&str>,
) -> Result<(), PttUnavailableReason> {
    let _guard = OPERATION.lock().await;
    let Some(key) = key else {
        deactivate(app);
        return Ok(());
    };
    let mouse_button = match parse_key(key) {
        Ok(PttKey::Mouse(button)) => Some(button),
        _ => None,
    };
    let result = if let Some(button) = mouse_button {
        // Only one trigger at a time: drop the keyboard shortcut first.
        unregister_global(app);
        close_portal();
        mouse::start(button)
    } else {
        mouse::stop();
        match current_backend() {
            Backend::GlobalShortcut => register_global(app, key),
            Backend::Portal => activate_portal(key).await,
        }
    };
    let mut state = state();
    state.mouse_key = mouse_button.is_some();
    state.last_failure = result.err();
    state.forwarding = result.is_ok();
    result
}

fn deactivate<R: Runtime>(app: &AppHandle<R>) {
    state().forwarding = false;
    unregister_global(app);
    mouse::stop();
    // Never leave the engine transmitting because the key was held while switching off.
    emit(false);
}

/// Applies "Allow mouse buttons (requires input group)" (Linux Wayland only, FR-1.39). Call
/// [`configure`] afterwards so a configured mouse button starts or stops.
pub fn set_evdev_mouse_allowed(allowed: bool) {
    mouse::set_evdev_allowed(allowed);
}

/// Whether and how mouse buttons can be used for push-to-talk (AC-1.37).
pub fn mouse_status() -> PttMouseStatus {
    mouse::status()
}

/// Current availability for the UI (FR-1.5).
pub async fn availability() -> PttAvailability {
    let backend = current_backend();
    let (mouse_key, last_failure) = {
        let state = state();
        (state.mouse_key, state.last_failure)
    };
    // A mouse button does not need the portal.
    let uses_portal = backend == Backend::Portal && !mouse_key;
    let portal_reachable = uses_portal && portal_reachable().await;
    availability_from(uses_portal, portal_reachable, last_failure)
}

fn availability_from(
    uses_portal: bool,
    portal_reachable: bool,
    last_failure: Option<PttUnavailableReason>,
) -> PttAvailability {
    let reason = if uses_portal && !portal_reachable {
        Some(PttUnavailableReason::WaylandNoPortal)
    } else {
        last_failure
    };
    match reason {
        Some(reason) => PttAvailability::Unavailable { reason },
        None => PttAvailability::Available,
    }
}

/// Validates a key captured by the UI and returns the string to store as `pttKey`.
///
/// On Wayland the portal binds the shortcut with `key` as the preferred trigger; the user
/// confirms or changes it in the desktop's dialog and the chosen trigger is returned. If the
/// desktop already knows the shortcut but has no key for it, its shortcut settings open
/// instead. Desktop failures are `shortcutNotConfirmed`, never `invalidInput`.
pub async fn capture(key: &str) -> ApiResult<String> {
    let invalid = |err: KeyError| ApiError::new(ErrorCode::InvalidInput, err.to_string());
    match parse_key(key).map_err(invalid)? {
        PttKey::Mouse(button) if mouse::capturable() => Ok(button.name().to_owned()),
        PttKey::Mouse(_) => Err(ApiError::new(
            ErrorCode::InvalidInput,
            "mouse buttons are not enabled for push-to-talk",
        )),
        PttKey::Keyboard(normalized) => match current_backend() {
            Backend::GlobalShortcut => Ok(normalized),
            Backend::Portal => capture_portal(&normalized).await,
        },
    }
}

/// Aborts a pending portal capture, which then fails with `invalidInput`.
pub fn cancel_capture() {
    #[cfg(target_os = "linux")]
    if let Some(cancel) = state().capture_cancel.take() {
        // The capture may have finished in the meantime; then there is nothing to cancel.
        let _ = cancel.send(());
    }
}

fn current_backend() -> Backend {
    #[cfg(target_os = "linux")]
    {
        let session_type = std::env::var("XDG_SESSION_TYPE").ok();
        let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
        detect_backend(session_type.as_deref(), wayland_display.as_deref())
    }
    #[cfg(not(target_os = "linux"))]
    Backend::GlobalShortcut
}

/// Picks the backend from the session environment; `XDG_SESSION_TYPE` wins when set.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn detect_backend(session_type: Option<&str>, wayland_display: Option<&str>) -> Backend {
    match session_type.map(str::to_ascii_lowercase).as_deref() {
        Some("wayland") => Backend::Portal,
        Some("x11") => Backend::GlobalShortcut,
        _ if wayland_display.is_some_and(|display| !display.is_empty()) => Backend::Portal,
        _ => Backend::GlobalShortcut,
    }
}

// ---------------------------------------------------------------------------------------------
// Key strings
// ---------------------------------------------------------------------------------------------

/// Modifiers in canonical order with their spellings (UI `KeyboardEvent`, plugin, portal).
const MODIFIERS: [(&str, &[&str]); 4] = [
    ("Ctrl", &["ctrl", "control"]),
    ("Alt", &["alt", "option"]),
    ("Shift", &["shift"]),
    ("Super", &["super", "meta", "cmd", "command", "logo", "win"]),
];

/// `KeyboardEvent.code` values of modifier keys pressed on their own.
const MODIFIER_CODES: [&str; 8] = [
    "ControlLeft",
    "ControlRight",
    "AltLeft",
    "AltRight",
    "ShiftLeft",
    "ShiftRight",
    "MetaLeft",
    "MetaRight",
];

/// Named keys (canonical = `KeyboardEvent.code`) with their XKB keysym for the portal.
/// Letters, digits and F1–F24 are handled separately.
const NAMED_KEYS: [(&str, &str); 52] = [
    ("Space", "space"),
    ("Enter", "Return"),
    ("Tab", "Tab"),
    ("Escape", "Escape"),
    ("Backspace", "BackSpace"),
    ("Delete", "Delete"),
    ("Insert", "Insert"),
    ("Home", "Home"),
    ("End", "End"),
    ("PageUp", "Prior"),
    ("PageDown", "Next"),
    ("ArrowUp", "Up"),
    ("ArrowDown", "Down"),
    ("ArrowLeft", "Left"),
    ("ArrowRight", "Right"),
    ("CapsLock", "Caps_Lock"),
    ("NumLock", "Num_Lock"),
    ("ScrollLock", "Scroll_Lock"),
    ("PrintScreen", "Print"),
    ("Pause", "Pause"),
    ("Backquote", "grave"),
    ("Minus", "minus"),
    ("Equal", "equal"),
    ("BracketLeft", "bracketleft"),
    ("BracketRight", "bracketright"),
    ("Backslash", "backslash"),
    ("Semicolon", "semicolon"),
    ("Quote", "apostrophe"),
    ("Comma", "comma"),
    ("Period", "period"),
    ("Slash", "slash"),
    ("Numpad0", "KP_0"),
    ("Numpad1", "KP_1"),
    ("Numpad2", "KP_2"),
    ("Numpad3", "KP_3"),
    ("Numpad4", "KP_4"),
    ("Numpad5", "KP_5"),
    ("Numpad6", "KP_6"),
    ("Numpad7", "KP_7"),
    ("Numpad8", "KP_8"),
    ("Numpad9", "KP_9"),
    ("NumpadAdd", "KP_Add"),
    ("NumpadSubtract", "KP_Subtract"),
    ("NumpadMultiply", "KP_Multiply"),
    ("NumpadDivide", "KP_Divide"),
    ("NumpadDecimal", "KP_Decimal"),
    ("NumpadEnter", "KP_Enter"),
    ("NumpadEqual", "KP_Equal"),
    ("AudioVolumeUp", "XF86AudioRaiseVolume"),
    ("AudioVolumeDown", "XF86AudioLowerVolume"),
    ("AudioVolumeMute", "XF86AudioMute"),
    ("MediaPlayPause", "XF86AudioPlay"),
];

/// A validated push-to-talk key.
#[derive(Debug, PartialEq, Eq)]
enum PttKey {
    /// Canonical keyboard shortcut, e.g. `"Ctrl+A"`.
    Keyboard(String),
    Mouse(MouseButton),
}

/// Parses a stored or captured key: mouse buttons (`"Mouse4"`) or keyboard shortcuts.
fn parse_key(raw: &str) -> Result<PttKey, KeyError> {
    match mouse::parse(raw) {
        Some(result) => result.map(PttKey::Mouse),
        None => normalize_key(raw).map(PttKey::Keyboard),
    }
}

/// Normalises a shortcut such as `"control+KeyA"` or `"Ctrl+F13"` to the canonical form
/// `"Ctrl+A"` (modifiers in the order Ctrl, Alt, Shift, Super; then exactly one key).
pub fn normalize_key(raw: &str) -> Result<String, KeyError> {
    let tokens: Vec<&str> = raw.split('+').map(str::trim).collect();
    if raw.trim().is_empty() {
        return Err(KeyError::Empty);
    }
    let (key, modifiers) = tokens.split_last().ok_or(KeyError::Empty)?;
    let mut held = [false; MODIFIERS.len()];
    for token in modifiers {
        let index = modifier_index(token).ok_or(KeyError::InvalidFormat)?;
        held[index] = true;
    }
    if key.is_empty() {
        return Err(KeyError::InvalidFormat);
    }
    if modifier_index(key).is_some() || MODIFIER_CODES.iter().any(|c| c.eq_ignore_ascii_case(key)) {
        return Err(KeyError::ModifierOnly);
    }
    let key = canonical_key(key).ok_or_else(|| KeyError::UnknownKey((*key).to_owned()))?;
    let mut parts: Vec<&str> = MODIFIERS
        .iter()
        .zip(held)
        .filter_map(|((name, _), on)| on.then_some(*name))
        .collect();
    parts.push(&key);
    Ok(parts.join("+"))
}

fn modifier_index(token: &str) -> Option<usize> {
    MODIFIERS
        .iter()
        .position(|(_, spellings)| spellings.iter().any(|s| s.eq_ignore_ascii_case(token)))
}

fn canonical_key(key: &str) -> Option<String> {
    let upper = key.to_ascii_uppercase();
    let single = upper
        .strip_prefix("KEY")
        .or_else(|| upper.strip_prefix("DIGIT"))
        .unwrap_or(&upper);
    if single.len() == 1 && single.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Some(single.to_owned());
    }
    if let Some(number) = upper.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&number).then(|| format!("F{number}"));
    }
    NAMED_KEYS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(name, _)| (*name).to_owned())
}

/// Converts a canonical key to the XDG "shortcuts" format used as the portal's preferred
/// trigger, e.g. `"Ctrl+A"` → `"CTRL+a"`. `None` if the key has no known keysym.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn portal_trigger(canonical: &str) -> Option<String> {
    let (key, modifiers) = canonical
        .rsplit_once('+')
        .map_or((canonical, ""), |(m, k)| (k, m));
    let keysym = if key.len() == 1 {
        key.to_ascii_lowercase()
    } else if key.starts_with('F') && key[1..].parse::<u8>().is_ok() {
        key.to_owned()
    } else {
        let (_, keysym) = NAMED_KEYS.iter().find(|(name, _)| *name == key)?;
        (*keysym).to_owned()
    };
    let mut parts: Vec<&str> = modifiers
        .split('+')
        .filter(|m| !m.is_empty())
        .map(|m| match m {
            "Ctrl" => "CTRL",
            "Alt" => "ALT",
            "Shift" => "SHIFT",
            _ => "LOGO",
        })
        .collect();
    parts.push(&keysym);
    Some(parts.join("+"))
}

// ---------------------------------------------------------------------------------------------
// Global-shortcut plugin (X11, Windows, macOS)
// ---------------------------------------------------------------------------------------------

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn register_global<R: Runtime>(app: &AppHandle<R>, key: &str) -> Result<(), PttUnavailableReason> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let key = normalize_key(key).map_err(|err| {
        tracing::warn!("push-to-talk key rejected: {err}");
        PttUnavailableReason::RegistrationFailed
    })?;
    if state().registered.as_deref() == Some(key.as_str()) {
        return Ok(());
    }
    unregister_global(app);
    app.global_shortcut()
        .on_shortcut(key.as_str(), |_app, _shortcut, event| {
            emit(event.state == ShortcutState::Pressed);
        })
        .map_err(|err| {
            tracing::warn!("registering the push-to-talk shortcut failed: {err}");
            PttUnavailableReason::RegistrationFailed
        })?;
    state().registered = Some(key);
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn register_global<R: Runtime>(
    _app: &AppHandle<R>,
    _key: &str,
) -> Result<(), PttUnavailableReason> {
    Err(PttUnavailableReason::RegistrationFailed)
}

fn unregister_global<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        let Some(key) = state().registered.take() else {
            return;
        };
        if let Err(err) = app.global_shortcut().unregister(key.as_str()) {
            tracing::warn!("unregistering the push-to-talk shortcut failed: {err}");
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let _ = app;
}

// ---------------------------------------------------------------------------------------------
// XDG desktop portal (Wayland)
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
async fn portal_reachable() -> bool {
    match ashpd::desktop::global_shortcuts::GlobalShortcuts::new().await {
        Ok(_) => true,
        Err(err) => {
            tracing::info!("global shortcuts portal unavailable: {err}");
            false
        }
    }
}

#[cfg(target_os = "linux")]
async fn activate_portal(key: &str) -> Result<(), PttUnavailableReason> {
    let bound = state().portal.as_ref().map(|binding| binding.key.clone());
    if bound.as_deref() == Some(key) {
        return Ok(());
    }
    let preferred = normalize_key(key).ok().and_then(|k| portal_trigger(&k));
    match portal::bind(preferred.as_deref()).await {
        // Keep the user's stored key: it is what `configure` is called with next time.
        Ok((binding, trigger)) => {
            if trigger.is_none() {
                // The session is live; a key assigned later in the desktop settings works.
                tracing::warn!("the desktop has no key assigned to the push-to-talk shortcut");
            }
            replace_portal_binding(binding.with_key(key.to_owned()));
            Ok(())
        }
        Err(portal::BindError::Unavailable(err)) => {
            tracing::warn!("global shortcuts portal unavailable: {err}");
            Err(PttUnavailableReason::WaylandNoPortal)
        }
        Err(err) => {
            tracing::warn!("binding the push-to-talk shortcut via the portal failed: {err}");
            Err(PttUnavailableReason::RegistrationFailed)
        }
    }
}

/// How long the desktop's shortcut settings stay open for the user to assign a key.
#[cfg(target_os = "linux")]
const ASSIGN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

#[cfg(target_os = "linux")]
async fn capture_portal(normalized: &str) -> ApiResult<String> {
    let _guard = OPERATION.lock().await;
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    state().capture_cancel = Some(cancel_tx);
    let preferred = portal_trigger(normalized);
    tracing::info!(
        "binding the push-to-talk shortcut via the portal (preferred trigger {preferred:?})"
    );
    let result = tokio::select! {
        result = bind_and_assign(preferred.as_deref()) => Some(result),
        _ = cancel_rx => None,
    };
    state().capture_cancel = None;
    let not_confirmed = |detail: String| {
        tracing::warn!("the desktop did not confirm the push-to-talk shortcut: {detail}");
        Err(ApiError::new(ErrorCode::ShortcutNotConfirmed, detail))
    };
    match result {
        None => Err(ApiError::new(
            ErrorCode::InvalidInput,
            "key capture cancelled",
        )),
        Some(Ok((binding, trigger))) => {
            tracing::info!("push-to-talk shortcut bound to {trigger:?}");
            replace_portal_binding(binding.with_key(trigger.clone()));
            Ok(trigger)
        }
        Some(Err(portal::BindError::Cancelled)) => {
            not_confirmed("the shortcut dialog was dismissed".to_owned())
        }
        Some(Err(portal::BindError::NoTrigger)) => {
            not_confirmed("no key was assigned to the shortcut".to_owned())
        }
        Some(Err(err)) => not_confirmed(err.to_string()),
    }
}

/// Binds the shortcut; if the desktop returns it without a key (KDE does this, without any
/// dialog, for a shortcut it already knows), opens the desktop's shortcut settings and waits
/// until the user assigns one.
#[cfg(target_os = "linux")]
async fn bind_and_assign(
    preferred: Option<&str>,
) -> Result<(portal::Binding, String), portal::BindError> {
    let (binding, trigger) = portal::bind(preferred).await?;
    if let Some(trigger) = trigger {
        return Ok((binding, trigger));
    }
    tracing::info!(
        "the desktop knows the push-to-talk shortcut but has no key assigned; \
         opening its shortcut settings"
    );
    match tokio::time::timeout(ASSIGN_TIMEOUT, portal::assign(&binding)).await {
        Ok(Ok(trigger)) => Ok((binding, trigger)),
        Ok(Err(err)) => {
            binding.close();
            Err(err)
        }
        Err(_elapsed) => {
            binding.close();
            Err(portal::BindError::NoTrigger)
        }
    }
}

/// Closes the live portal session, if any (a mouse button replaces the keyboard shortcut).
#[cfg(target_os = "linux")]
fn close_portal() {
    if let Some(old) = state().portal.take() {
        old.close();
    }
}

#[cfg(not(target_os = "linux"))]
fn close_portal() {}

/// Makes `binding` the live portal session and closes the previous one.
#[cfg(target_os = "linux")]
fn replace_portal_binding(binding: portal::Binding) {
    if let Some(old) = state().portal.replace(binding) {
        old.close();
    }
}

#[cfg(not(target_os = "linux"))]
async fn portal_reachable() -> bool {
    false
}

#[cfg(not(target_os = "linux"))]
async fn activate_portal(_key: &str) -> Result<(), PttUnavailableReason> {
    Err(PttUnavailableReason::WaylandNoPortal)
}

#[cfg(not(target_os = "linux"))]
async fn capture_portal(normalized: &str) -> ApiResult<String> {
    Ok(normalized.to_owned())
}

#[cfg(target_os = "linux")]
mod portal {
    use std::pin::Pin;
    use std::sync::Arc;

    use ashpd::desktop::global_shortcuts::{
        BindShortcutsOptions, ConfigureShortcutsOptions, GlobalShortcuts, NewShortcut, Shortcut,
    };
    use ashpd::desktop::{CreateSessionOptions, ResponseError, Session};
    use ashpd::zbus::export::futures_core::Stream;
    use tauri::async_runtime::{self, JoinHandle};

    const SHORTCUT_ID: &str = "push-to-talk";
    const SHORTCUT_DESCRIPTION: &str = "Push to talk";

    /// A live portal session with its push-to-talk shortcut and the task listening to it.
    pub struct Binding {
        /// The `pttKey` this binding was made for.
        pub key: String,
        session: Arc<Session<GlobalShortcuts>>,
        listener: JoinHandle<()>,
    }

    impl Binding {
        pub fn with_key(self, key: String) -> Self {
            Self { key, ..self }
        }

        pub fn close(self) {
            self.listener.abort();
            async_runtime::spawn(async move {
                if let Err(err) = self.session.close().await {
                    tracing::debug!("closing the old portal session failed: {err}");
                }
            });
        }
    }

    #[derive(Debug)]
    pub enum BindError {
        /// The portal or its GlobalShortcuts interface is missing.
        Unavailable(ashpd::Error),
        /// The user dismissed the consent dialog.
        Cancelled,
        /// No key was assigned to the shortcut.
        NoTrigger,
        Failed(ashpd::Error),
    }

    impl std::fmt::Display for BindError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Unavailable(err) => write!(f, "portal unavailable: {err}"),
                Self::Cancelled => f.write_str("dialog cancelled"),
                Self::NoTrigger => f.write_str("no trigger assigned"),
                Self::Failed(err) => write!(f, "{err}"),
            }
        }
    }

    /// Opens a new session and binds the push-to-talk shortcut. The desktop shows its consent
    /// dialog unless it remembers an earlier binding. Returns the binding (with an empty
    /// `key`) and the trigger the user chose, or `None` if the desktop has no key assigned
    /// (the session stays live, so a key assigned later still works).
    ///
    /// KDE only shows its dialog for shortcuts it has never seen for this app; for a known
    /// one it answers at once with the stored key and ignores `preferred_trigger`.
    pub async fn bind(
        preferred_trigger: Option<&str>,
    ) -> Result<(Binding, Option<String>), BindError> {
        let portal = GlobalShortcuts::new()
            .await
            .map_err(BindError::Unavailable)?;
        // Subscribe before binding so no early press is lost.
        let activated = portal
            .receive_activated()
            .await
            .map_err(BindError::Failed)?;
        let deactivated = portal
            .receive_deactivated()
            .await
            .map_err(BindError::Failed)?;
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(BindError::Failed)?;
        let shortcut = NewShortcut::new(SHORTCUT_ID, SHORTCUT_DESCRIPTION)
            .preferred_trigger(preferred_trigger);
        let response = portal
            .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
            .await
            .and_then(|request| request.response());
        let bound = match response {
            Ok(bound) => bound,
            Err(err) => {
                close_quietly(&session).await;
                return Err(match err {
                    ashpd::Error::Response(ResponseError::Cancelled) => BindError::Cancelled,
                    err => BindError::Failed(err),
                });
            }
        };
        let trigger = trigger_of(bound.shortcuts());
        let listener = async_runtime::spawn(async move {
            // The proxy must outlive the signal streams.
            let _portal = portal;
            listen(Box::pin(activated), Box::pin(deactivated)).await;
        });
        let binding = Binding {
            key: String::new(),
            session: Arc::new(session),
            listener,
        };
        Ok((binding, trigger))
    }

    /// Opens the desktop's settings for this session's shortcuts (portal version 2) and waits
    /// until the push-to-talk shortcut has a key; returns its trigger description.
    pub async fn assign(binding: &Binding) -> Result<String, BindError> {
        let portal = GlobalShortcuts::new()
            .await
            .map_err(BindError::Unavailable)?;
        // The portal only sends our own sessions' signals; any session of this app sees the
        // same shortcut, so the session handle need not be compared.
        let mut changed = Box::pin(
            portal
                .receive_shortcuts_changed()
                .await
                .map_err(BindError::Failed)?,
        );
        portal
            .configure_shortcuts(&binding.session, None, ConfigureShortcutsOptions::default())
            .await
            .map_err(BindError::Failed)?;
        while let Some(event) = next(&mut changed).await {
            if let Some(trigger) = trigger_of(event.shortcuts()) {
                return Ok(trigger);
            }
        }
        Err(BindError::NoTrigger)
    }

    /// The push-to-talk trigger description in `shortcuts`, if it has a key.
    fn trigger_of(shortcuts: &[Shortcut]) -> Option<String> {
        shortcuts
            .iter()
            .find(|s| s.id() == SHORTCUT_ID)
            .map(|s| s.trigger_description().trim().to_owned())
            .filter(|t| !t.is_empty())
    }

    async fn close_quietly(session: &Session<GlobalShortcuts>) {
        if let Err(err) = session.close().await {
            tracing::debug!("closing the portal session failed: {err}");
        }
    }

    async fn listen(
        mut activated: Pin<Box<impl Stream<Item = ashpd::desktop::global_shortcuts::Activated>>>,
        mut deactivated: Pin<
            Box<impl Stream<Item = ashpd::desktop::global_shortcuts::Deactivated>>,
        >,
    ) {
        loop {
            tokio::select! {
                Some(event) = next(&mut activated) => {
                    if event.shortcut_id() == SHORTCUT_ID {
                        super::emit(true);
                    }
                }
                Some(event) = next(&mut deactivated) => {
                    if event.shortcut_id() == SHORTCUT_ID {
                        super::emit(false);
                    }
                }
                else => break,
            }
        }
        tracing::warn!("global shortcuts portal signal streams ended");
    }

    async fn next<S: Stream>(stream: &mut Pin<Box<S>>) -> Option<S::Item> {
        std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_session_uses_portal() {
        assert_eq!(detect_backend(Some("wayland"), None), Backend::Portal);
        assert_eq!(detect_backend(Some("Wayland"), Some("")), Backend::Portal);
        assert_eq!(detect_backend(None, Some("wayland-0")), Backend::Portal);
        assert_eq!(
            detect_backend(Some("tty"), Some("wayland-0")),
            Backend::Portal
        );
    }

    #[test]
    fn x11_or_unknown_session_uses_global_shortcut() {
        assert_eq!(detect_backend(Some("x11"), None), Backend::GlobalShortcut);
        // An explicit X11 session wins over a nested Wayland display.
        assert_eq!(
            detect_backend(Some("x11"), Some("wayland-0")),
            Backend::GlobalShortcut
        );
        assert_eq!(detect_backend(None, None), Backend::GlobalShortcut);
        assert_eq!(detect_backend(None, Some("")), Backend::GlobalShortcut);
    }

    #[test]
    fn availability_reports_missing_portal() {
        assert_eq!(
            availability_from(true, false, None),
            PttAvailability::Unavailable {
                reason: PttUnavailableReason::WaylandNoPortal
            }
        );
        // A missing portal explains more than an earlier failed bind.
        assert_eq!(
            availability_from(true, false, Some(PttUnavailableReason::RegistrationFailed)),
            PttAvailability::Unavailable {
                reason: PttUnavailableReason::WaylandNoPortal
            }
        );
        assert_eq!(
            availability_from(true, true, None),
            PttAvailability::Available
        );
    }

    #[test]
    fn availability_reports_failed_registration() {
        assert_eq!(
            availability_from(false, false, None),
            PttAvailability::Available
        );
        assert_eq!(
            availability_from(false, false, Some(PttUnavailableReason::RegistrationFailed)),
            PttAvailability::Unavailable {
                reason: PttUnavailableReason::RegistrationFailed
            }
        );
        assert_eq!(
            availability_from(true, true, Some(PttUnavailableReason::RegistrationFailed)),
            PttAvailability::Unavailable {
                reason: PttUnavailableReason::RegistrationFailed
            }
        );
    }

    #[test]
    fn normalizes_ui_key_codes() {
        assert_eq!(normalize_key("KeyA").as_deref(), Ok("A"));
        assert_eq!(normalize_key("Digit5").as_deref(), Ok("5"));
        assert_eq!(normalize_key("F13").as_deref(), Ok("F13"));
        assert_eq!(
            normalize_key("control+shift+KeyV").as_deref(),
            Ok("Ctrl+Shift+V")
        );
        assert_eq!(
            normalize_key("Meta+Alt+Ctrl+Space").as_deref(),
            Ok("Ctrl+Alt+Super+Space")
        );
        assert_eq!(normalize_key(" ctrl + f13 ").as_deref(), Ok("Ctrl+F13"));
        assert_eq!(normalize_key("numpad0").as_deref(), Ok("Numpad0"));
        assert_eq!(
            normalize_key("Ctrl+Ctrl+Backquote").as_deref(),
            Ok("Ctrl+Backquote")
        );
    }

    #[test]
    fn rejects_invalid_keys() {
        assert_eq!(normalize_key(""), Err(KeyError::Empty));
        assert_eq!(normalize_key("  "), Err(KeyError::Empty));
        assert_eq!(normalize_key("ControlLeft"), Err(KeyError::ModifierOnly));
        assert_eq!(
            normalize_key("Ctrl+ShiftRight"),
            Err(KeyError::ModifierOnly)
        );
        assert_eq!(normalize_key("Ctrl"), Err(KeyError::ModifierOnly));
        assert_eq!(
            normalize_key("F25"),
            Err(KeyError::UnknownKey("F25".into()))
        );
        assert_eq!(
            normalize_key("IntlRo"),
            Err(KeyError::UnknownKey("IntlRo".into()))
        );
        assert_eq!(normalize_key("A+B"), Err(KeyError::InvalidFormat));
        assert_eq!(normalize_key("Ctrl+"), Err(KeyError::InvalidFormat));
    }

    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    #[test]
    fn canonical_keys_are_accepted_by_the_plugin() {
        use tauri_plugin_global_shortcut::Shortcut;

        let letters = ('A'..='Z').chain('0'..='9').map(String::from);
        let functions = (1..=24).map(|n| format!("F{n}"));
        let named = NAMED_KEYS.iter().map(|(name, _)| (*name).to_owned());
        for key in letters.chain(functions).chain(named) {
            let shortcut = format!("Ctrl+Alt+Shift+Super+{key}");
            assert_eq!(normalize_key(&shortcut).as_deref(), Ok(shortcut.as_str()));
            assert!(
                shortcut.parse::<Shortcut>().is_ok(),
                "plugin rejects {shortcut}"
            );
        }
    }

    #[test]
    fn converts_to_portal_triggers() {
        assert_eq!(portal_trigger("A").as_deref(), Some("a"));
        assert_eq!(portal_trigger("Ctrl+F13").as_deref(), Some("CTRL+F13"));
        assert_eq!(
            portal_trigger("Ctrl+Alt+Shift+Super+5").as_deref(),
            Some("CTRL+ALT+SHIFT+LOGO+5")
        );
        assert_eq!(
            portal_trigger("Shift+PageUp").as_deref(),
            Some("SHIFT+Prior")
        );
        assert_eq!(portal_trigger("Backquote").as_deref(), Some("grave"));
        assert_eq!(portal_trigger("Unknown"), None);
    }

    /// Strings exactly as the wizard and settings build them from `KeyboardEvent`, through
    /// normalisation to the portal trigger (these reached KDE correctly in the field).
    #[test]
    fn ui_key_strings_reach_the_portal() {
        let cases = [
            ("KeyA", "a"),
            ("Ctrl+KeyZ", "CTRL+z"),
            ("Ctrl+Shift+KeyZ", "CTRL+SHIFT+z"),
            ("Alt+KeyE", "ALT+e"),
            ("Ctrl+Digit1", "CTRL+1"),
            ("Digit1", "1"),
            ("End", "End"),
            ("Ctrl+End", "CTRL+End"),
            ("F8", "F8"),
            ("Shift+F9", "SHIFT+F9"),
            ("Space", "space"),
            ("Meta+KeyQ", "LOGO+q"),
            ("Super+KeyQ", "LOGO+q"),
        ];
        for (ui, trigger) in cases {
            let canonical = normalize_key(ui).unwrap_or_else(|err| panic!("{ui}: {err}"));
            assert_eq!(portal_trigger(&canonical).as_deref(), Some(trigger), "{ui}");
        }
    }

    #[test]
    fn mouse_key_needs_no_portal() {
        // `availability` passes `uses_portal = false` for a mouse key, even on Wayland.
        assert_eq!(
            availability_from(false, false, None),
            PttAvailability::Available
        );
        assert_eq!(
            availability_from(
                false,
                false,
                Some(PttUnavailableReason::MouseNeedsInputGroup)
            ),
            PttAvailability::Unavailable {
                reason: PttUnavailableReason::MouseNeedsInputGroup
            }
        );
    }

    #[test]
    fn routes_mouse_and_keyboard_keys() {
        assert_eq!(parse_key("Mouse4"), Ok(PttKey::Mouse(MouseButton::Back)));
        assert_eq!(parse_key("Mouse5"), Ok(PttKey::Mouse(MouseButton::Forward)));
        assert_eq!(
            parse_key("MouseMiddle"),
            Ok(PttKey::Mouse(MouseButton::Middle))
        );
        assert_eq!(
            parse_key("Ctrl+KeyA"),
            Ok(PttKey::Keyboard("Ctrl+A".into()))
        );
        assert_eq!(parse_key("F13"), Ok(PttKey::Keyboard("F13".into())));
        assert_eq!(parse_key("MouseLeft"), Err(KeyError::PrimaryMouseButton));
        assert_eq!(parse_key("Shift+Mouse4"), Err(KeyError::InvalidFormat));
        // Mouse keys never reach the keyboard path (plugin or portal).
        assert!(normalize_key("Mouse4").is_err());
    }

    #[test]
    fn mouse_capture_returns_the_stored_name_without_the_portal() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        // Native backends (X11, Windows, macOS) always accept mouse buttons.
        if current_backend() == Backend::GlobalShortcut {
            assert_eq!(
                runtime.block_on(capture("Mouse4")).ok().as_deref(),
                Some("Mouse4")
            );
        }
        let err = runtime.block_on(capture("MouseRight")).err();
        assert_eq!(err.map(|e| e.code), Some(ErrorCode::InvalidInput));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wayland_mouse_capture_needs_the_opt_in() {
        if current_backend() != Backend::Portal {
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        set_evdev_mouse_allowed(false);
        let err = runtime.block_on(capture("Mouse5")).err();
        assert_eq!(err.map(|e| e.code), Some(ErrorCode::InvalidInput));
        assert_eq!(
            mouse_status().support,
            crate::commands::ptt::PttMouseSupport::Evdev
        );
        set_evdev_mouse_allowed(true);
        assert_eq!(
            runtime.block_on(capture("Mouse5")).ok().as_deref(),
            Some("Mouse5")
        );
        set_evdev_mouse_allowed(false);
    }

    #[test]
    fn emit_forwards_only_changes_while_active() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALLS: AtomicUsize = AtomicUsize::new(0);
        set_handler(|_| {
            CALLS.fetch_add(1, Ordering::SeqCst);
        });
        state().forwarding = false;
        emit(true);
        assert_eq!(CALLS.load(Ordering::SeqCst), 0, "press while switched off");
        state().forwarding = true;
        emit(true);
        emit(true);
        assert_eq!(CALLS.load(Ordering::SeqCst), 1, "repeat press is dropped");
        state().forwarding = false;
        emit(false);
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            2,
            "release always reaches the engine"
        );
    }

    /// X11 end-to-end: needs an X server on `DISPLAY` and `xdotool`. Use a private server
    /// (Xvfb, or `Xwayland :57` inside a Wayland session): the session's own XWayland forwards
    /// synthetic input to the compositor, so xdotool presses never reach the grab there. Run with
    /// `DISPLAY=:57 cargo test -p mumcord ptt::tests::x11_press_and_release -- --ignored`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs an X11 display and xdotool"]
    fn x11_press_and_release() {
        use std::sync::mpsc;
        use std::time::Duration;

        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_global_shortcut::Builder::new().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let (tx, rx) = mpsc::channel();
        set_handler(move |pressed| tx.send(pressed).expect("test receiver alive"));
        register_global(app.handle(), "F9").expect("register F9");
        state().forwarding = true;

        let xdotool = |action: &str| {
            let status = std::process::Command::new("xdotool")
                .args([action, "F9"])
                .status()
                .expect("run xdotool");
            assert!(status.success());
        };
        xdotool("keydown");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(true));
        xdotool("keyup");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(false));

        state().forwarding = false;
        unregister_global(app.handle());
    }
}
