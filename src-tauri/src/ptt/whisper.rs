//! Hold-to-talk keys of whisper targets (issue #67), bound through the global hotkey layer
//! ([`crate::hotkeys`], issue #64) as their own [`Group`]: X11, Windows, macOS and the Wayland
//! compatibility mode use `tauri-plugin-global-shortcut`, native Wayland the desktop portal.
//! Without the portal, or wherever a key cannot be registered, whisper still works from the UI
//! and the settings say why the key is unavailable.
//!
//! Engine hook: like [`super::set_handler`], this module knows nothing about audio. The app
//! installs a sink with [`set_handler`] that receives `(target id, pressed)`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mumcord_engine::store::Hotkey;
use tauri::AppHandle;

use super::{Backend, KeyError, current_backend, normalize_key};
use crate::commands::hotkeys::HotkeySlot;
use crate::commands::whisper::WhisperKeysUnavailable;
use crate::hotkeys::{Binding, Group};

/// Sink for whisper key changes: target id and whether the key is held.
pub type WhisperKeyHandler = dyn Fn(&str, bool) + Send + Sync;

struct State {
    handler: Option<Arc<WhisperKeyHandler>>,
    /// Targets whose key is bound.
    bound: Vec<String>,
}

static STATE: Mutex<State> = Mutex::new(State {
    handler: None,
    bound: Vec::new(),
});

/// Native Wayland without the global-shortcuts portal, as found by the last [`configure`].
static PORTAL_MISSING: AtomicBool = AtomicBool::new(false);

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Installs the sink that receives whisper key changes; replaces any previous one.
pub fn set_handler(handler: impl Fn(&str, bool) + Send + Sync + 'static) {
    state().handler = Some(Arc::new(handler));
}

/// Why whisper keys cannot work in this session at all, if they cannot.
pub fn unavailable() -> Option<WhisperKeysUnavailable> {
    PORTAL_MISSING
        .load(Ordering::Relaxed)
        .then_some(WhisperKeysUnavailable::WaylandPortal)
}

/// Checks a key for a whisper target: a keyboard shortcut in the canonical form that neither
/// Mumcord's own shortcuts, push-to-talk, an action hotkey nor another target uses.
pub fn validate(
    key: &str,
    ptt_key: Option<&str>,
    hotkeys: &[Hotkey],
    others: &[&str],
) -> Result<String, KeyError> {
    let (key, conflict) = crate::hotkeys::check(key, HotkeySlot::New, hotkeys, ptt_key)?;
    let taken = |other: &&str| normalize_key(other).is_ok_and(|o| o == key);
    if conflict.is_some() || others.iter().any(taken) {
        return Err(KeyError::InvalidFormat);
    }
    Ok(key)
}

/// Binds exactly `keys` (target id, key) as whisper keys. Returns the targets whose key could
/// not be bound, e.g. because another app holds it.
pub async fn configure(app: &AppHandle, keys: &[(String, String)]) -> Vec<String> {
    // Never leave a whisper running because its key was held while it went away.
    let (previous, handler) = {
        let mut state = state();
        (std::mem::take(&mut state.bound), state.handler.clone())
    };
    if let Some(handler) = &handler {
        for target in &previous {
            handler(target, false);
        }
    }
    let missing = current_backend() == Backend::Portal && !crate::ptt::portal_reachable().await;
    PORTAL_MISSING.store(missing, Ordering::Relaxed);
    let bindings = keys
        .iter()
        .map(|(target, key)| {
            let id = target.clone();
            Binding {
                id: target.clone(),
                description: "Whisper".to_owned(),
                key: key.clone(),
                sink: Arc::new(move |pressed| {
                    let handler = state().handler.clone();
                    if let Some(handler) = handler {
                        handler(&id, pressed);
                    }
                }),
            }
        })
        .collect();
    let failed = crate::hotkeys::bind_group(app, Group::Whisper, bindings).await;
    state().bound = keys
        .iter()
        .filter(|(id, _)| !failed.contains(id))
        .map(|(id, _)| id.clone())
        .collect();
    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_engine::store::HotkeyAction;

    #[test]
    fn whisper_keys_must_differ_from_push_to_talk_hotkeys_and_each_other() {
        let hotkeys = [Hotkey {
            key: "F11".into(),
            action: HotkeyAction::ToggleMute,
        }];
        assert_eq!(
            validate("F9", Some("F13"), &hotkeys, &["F10"]).as_deref(),
            Ok("F9")
        );
        assert_eq!(
            validate("control+KeyQ", None, &[], &[]).as_deref(),
            Ok("Ctrl+Q")
        );
        assert!(
            validate("control+KeyW", None, &[], &[]).is_err(),
            "Ctrl+W closes Mumcord's windows"
        );
        assert!(validate("F13", Some("F13"), &[], &[]).is_err());
        assert!(validate("F11", None, &hotkeys, &[]).is_err());
        assert!(validate("Ctrl+Q", None, &[], &["control+KeyQ"]).is_err());
        assert!(validate("Shift", None, &[], &[]).is_err());
    }
}
