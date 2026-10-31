//! Hold-to-talk keys of whisper targets (issue #67), on the push-to-talk shortcut machinery.
//!
//! This is only what whisper needs; general global hotkeys are issue #64. Whisper keys are
//! keyboard shortcuts registered with `tauri-plugin-global-shortcut`, i.e. the push-to-talk
//! backend of X11, Windows, macOS and the Wayland compatibility mode. Native Wayland sessions
//! bind shortcuts through the desktop portal, one consent dialog per binding set; adding more
//! keys to that binding belongs to #64. There, and wherever the key cannot be registered,
//! whisper still works from the UI and the settings say why the key is unavailable.
//!
//! Engine hook: like [`super::set_handler`], this module knows nothing about audio. The app
//! installs a sink with [`set_handler`] that receives `(target id, pressed)`.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Runtime};

use super::{Backend, KeyError, current_backend, normalize_key};
use crate::commands::whisper::WhisperKeysUnavailable;

/// Sink for whisper key changes: target id and whether the key is held.
pub type WhisperKeyHandler = dyn Fn(&str, bool) + Send + Sync;

struct State {
    handler: Option<Arc<WhisperKeyHandler>>,
    /// Registered shortcuts and the target each one holds.
    registered: Vec<(String, String)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    handler: None,
    registered: Vec::new(),
});

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Installs the sink that receives whisper key changes; replaces any previous one.
pub fn set_handler(handler: impl Fn(&str, bool) + Send + Sync + 'static) {
    state().handler = Some(Arc::new(handler));
}

/// Why whisper keys cannot work in this session at all, if they cannot.
pub fn unavailable() -> Option<WhisperKeysUnavailable> {
    match current_backend() {
        Backend::GlobalShortcut => None,
        Backend::Portal => Some(WhisperKeysUnavailable::WaylandPortal),
    }
}

/// Checks a key for a whisper target: a keyboard shortcut in the canonical form, not the
/// push-to-talk key and not taken by another target.
pub fn validate(key: &str, ptt_key: Option<&str>, others: &[&str]) -> Result<String, KeyError> {
    let key = normalize_key(key)?;
    let same = |other: &str| normalize_key(other).is_ok_and(|o| o == key);
    if ptt_key.is_some_and(same) || others.iter().any(|o| same(o)) {
        return Err(KeyError::InvalidFormat);
    }
    Ok(key)
}

/// Registers exactly `keys` (target id, key) as whisper keys. Returns the targets whose key
/// could not be registered, e.g. because another app holds it.
pub fn configure<R: Runtime>(app: &AppHandle<R>, keys: &[(String, String)]) -> Vec<String> {
    unregister_all(app);
    if unavailable().is_some() {
        return keys.iter().map(|(id, _)| id.clone()).collect();
    }
    let mut failed = Vec::new();
    for (target, key) in keys {
        match normalize_key(key).map(|key| register(app, target, &key)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                tracing::warn!("registering a whisper key failed: {err}");
                failed.push(target.clone());
            }
            Err(err) => {
                tracing::warn!("whisper key rejected: {err}");
                failed.push(target.clone());
            }
        }
    }
    failed
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn register<R: Runtime>(app: &AppHandle<R>, target: &str, key: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let id = target.to_owned();
    app.global_shortcut()
        .on_shortcut(key, move |_app, _shortcut, event| {
            let handler = state().handler.clone();
            if let Some(handler) = handler {
                handler(&id, event.state == ShortcutState::Pressed);
            }
        })
        .map_err(|e| e.to_string())?;
    state().registered.push((key.to_owned(), target.to_owned()));
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn register<R: Runtime>(_app: &AppHandle<R>, _target: &str, _key: &str) -> Result<(), String> {
    Err("global shortcuts are not supported on this platform".into())
}

fn unregister_all<R: Runtime>(app: &AppHandle<R>) {
    let registered = std::mem::take(&mut state().registered);
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        let handler = state().handler.clone();
        for (key, target) in registered {
            if let Err(err) = app.global_shortcut().unregister(key.as_str()) {
                tracing::warn!("unregistering a whisper key failed: {err}");
            }
            // Never leave a whisper running because its key was held while it went away.
            if let Some(handler) = &handler {
                handler(&target, false);
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let _ = (app, registered);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_keys_must_differ_from_push_to_talk_and_each_other() {
        assert_eq!(validate("F9", Some("F13"), &["F10"]).as_deref(), Ok("F9"));
        assert_eq!(validate("control+KeyW", None, &[]).as_deref(), Ok("Ctrl+W"));
        assert!(validate("F13", Some("F13"), &[]).is_err());
        assert!(validate("Ctrl+W", None, &["control+KeyW"]).is_err());
        assert!(validate("Shift", None, &[]).is_err());
    }
}
