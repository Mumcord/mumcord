//! Global hotkeys beyond push-to-talk (issue #64, FR-3.72, FR-3.73).
//!
//! Same backends as push-to-talk ([`crate::ptt`]): `tauri-plugin-global-shortcut` on X11,
//! Windows, macOS and in the Wayland compatibility mode; on native Wayland one session of the
//! XDG `GlobalShortcuts` portal for all hotkeys, so the desktop asks for consent once per set.
//! Without the portal, or with "Work while Mumcord is in the background" off, the UI handles
//! the keys itself while its window has the focus.
//!
//! Nothing is bound by default (Q1.7). A key press runs [`trigger`]: "Open Mumcord" brings the
//! window to the front here, every other action goes to the UI as [`HotkeyPressed`], which
//! already knows the voice server, the channels and the layout.
//!
//! Other features bind their keys through [`bind_group`] as well (whisper targets, #67): each
//! [`Group`] is replaced as a whole and never touches the keys of another group.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mumcord_engine::store::{self, Hotkey, HotkeyAction};
use tauri::AppHandle;
use tauri_specta::Event;

use crate::commands::hotkeys::{HotkeyConflict, HotkeyPressed, HotkeySlot, InAppShortcut};
use crate::ptt::{Backend, KeyError, current_backend, normalize_key};

/// Receives `true` when a key goes down and `false` when it comes up again.
pub type Sink = Arc<dyn Fn(bool) + Send + Sync>;

/// One key of a [`Group`].
pub struct Binding {
    /// Stable within the group; the portal files the shortcut under it.
    pub id: String,
    /// What the desktop's shortcut settings show (Wayland).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub description: String,
    /// Canonical key, see [`normalize_key`].
    pub key: String,
    pub sink: Sink,
}

/// Owners of global keys besides push-to-talk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Actions,
}

impl Group {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn prefix(self) -> &'static str {
        match self {
            Self::Actions => "hotkey",
        }
    }
}

#[derive(Default)]
struct GroupState {
    /// Keys registered with the global-shortcut plugin.
    registered: Vec<String>,
    #[cfg(target_os = "linux")]
    portal: Option<crate::ptt::portal::Binding>,
}

#[derive(Default)]
struct State {
    groups: HashMap<Group, GroupState>,
    /// Ids of action hotkeys that could not be bound, for the settings.
    failed: Vec<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Serialises rebinding so two settings changes cannot interleave.
static OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard: MutexGuard<'_, Option<State>> =
        STATE.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(State::default))
}

// ---------------------------------------------------------------------------------------------
// Conflicts
// ---------------------------------------------------------------------------------------------

/// Window-wide shortcuts of Mumcord itself (Ctrl, or Cmd on macOS). A global hotkey on one of
/// them would swallow it, so they are refused.
const IN_APP: [(&str, InAppShortcut); 10] = [
    ("Ctrl+Shift+M", InAppShortcut::MemberColumn),
    ("Shift+Super+M", InAppShortcut::MemberColumn),
    ("Ctrl+Shift+N", InAppShortcut::NotificationCentre),
    ("Shift+Super+N", InAppShortcut::NotificationCentre),
    ("Ctrl+K", InAppShortcut::Search),
    ("Super+K", InAppShortcut::Search),
    ("Ctrl+F", InAppShortcut::Search),
    ("Super+F", InAppShortcut::Search),
    ("Ctrl+W", InAppShortcut::CloseWindow),
    ("Super+W", InAppShortcut::CloseWindow),
];

/// Validates `key` for `slot` and returns its canonical form with the first thing already
/// using it: Mumcord's own shortcuts, push-to-talk, or another hotkey.
pub fn check(
    key: &str,
    slot: HotkeySlot,
    hotkeys: &[Hotkey],
    ptt_key: Option<&str>,
) -> Result<(String, Option<HotkeyConflict>), KeyError> {
    let key = normalize_key(key)?;
    let same = |other: &str| normalize_key(other).is_ok_and(|o| o == key);
    let conflict = if let Some((_, shortcut)) = IN_APP.iter().find(|(k, _)| *k == key) {
        Some(HotkeyConflict::InApp {
            shortcut: *shortcut,
        })
    } else if slot != HotkeySlot::PushToTalk && ptt_key.is_some_and(same) {
        Some(HotkeyConflict::PushToTalk)
    } else {
        hotkeys
            .iter()
            .enumerate()
            .find(|(i, h)| {
                slot != HotkeySlot::Hotkey {
                    index: index_of(*i),
                } && same(&h.key)
            })
            .map(|(i, _)| HotkeyConflict::Hotkey { index: index_of(i) })
    };
    Ok((key, conflict))
}

fn index_of(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

// ---------------------------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------------------------

/// Binds the action hotkeys of `settings`: all of them while "Work while Mumcord is in the
/// background" is on, none while it is off.
pub async fn configure(app: &AppHandle, settings: &store::Settings) {
    let bindings = if settings.global_hotkeys {
        action_bindings(app, &settings.hotkeys)
    } else {
        Vec::new()
    };
    let failed = bind_group(app, Group::Actions, bindings).await;
    with_state(|state| state.failed = failed);
}

/// Indices into `hotkeys` whose key could not be bound in the last [`configure`].
pub fn failed(hotkeys: &[Hotkey]) -> Vec<u32> {
    let failed = with_state(|state| state.failed.clone());
    hotkeys
        .iter()
        .enumerate()
        .filter(|(_, h)| failed.contains(&action_id(&h.action)))
        .map(|(i, _)| index_of(i))
        .collect()
}

fn action_bindings(app: &AppHandle, hotkeys: &[Hotkey]) -> Vec<Binding> {
    hotkeys
        .iter()
        .map(|hotkey| {
            let (app, action) = (app.clone(), hotkey.action.clone());
            Binding {
                id: action_id(&hotkey.action),
                description: action_description(&hotkey.action),
                key: hotkey.key.clone(),
                sink: Arc::new(move |pressed| {
                    if pressed {
                        trigger(&app, &action);
                    }
                }),
            }
        })
        .collect()
}

/// Runs a pressed action hotkey.
fn trigger(app: &AppHandle, action: &HotkeyAction) {
    match action {
        HotkeyAction::OpenMumcord => {
            crate::window::show(app);
            return;
        }
        // The menu opens in the window, which must be in front to be seen.
        HotkeyAction::OpenQuickSettings => crate::window::show(app),
        _ => {}
    }
    let event = HotkeyPressed {
        action: action.clone().into(),
    };
    if let Err(e) = event.emit(app) {
        tracing::warn!(error = %e, "telling the UI about a hotkey failed");
    }
}

/// A stable id per action; "switch to channel" also names its channel, so the desktop keeps
/// one entry per channel.
fn action_id(action: &HotkeyAction) -> String {
    let name = match action {
        HotkeyAction::ToggleMute => "mute",
        HotkeyAction::ToggleDeafen => "deafen",
        HotkeyAction::ToggleDnd => "dnd",
        HotkeyAction::JoinLeaveVoice => "join-leave-voice",
        HotkeyAction::SwitchVoiceServer => "switch-voice-server",
        HotkeyAction::SwitchChannel { server_id, path } => {
            return format!("channel-{server_id}-{}", path.join("/"));
        }
        HotkeyAction::PreviousChannel => "previous-channel",
        HotkeyAction::ToggleMemberColumn => "member-column",
        HotkeyAction::OpenQuickSettings => "quick-settings",
        HotkeyAction::OpenMumcord => "open",
    };
    name.to_owned()
}

/// English description for the desktop's shortcut list; the desktop shows it as it is.
fn action_description(action: &HotkeyAction) -> String {
    match action {
        HotkeyAction::ToggleMute => "Mute / unmute microphone".to_owned(),
        HotkeyAction::ToggleDeafen => "Deafen / undeafen".to_owned(),
        HotkeyAction::ToggleDnd => "Toggle Do Not Disturb".to_owned(),
        HotkeyAction::JoinLeaveVoice => "Join / leave voice".to_owned(),
        HotkeyAction::SwitchVoiceServer => "Switch voice server".to_owned(),
        HotkeyAction::SwitchChannel { path, .. } => {
            format!("Switch to channel {}", path.join(" / "))
        }
        HotkeyAction::PreviousChannel => "Back to previous channel".to_owned(),
        HotkeyAction::ToggleMemberColumn => "Show / hide member column".to_owned(),
        HotkeyAction::OpenQuickSettings => "Open quick settings".to_owned(),
        HotkeyAction::OpenMumcord => "Open Mumcord".to_owned(),
    }
}

// ---------------------------------------------------------------------------------------------
// Groups
// ---------------------------------------------------------------------------------------------

/// Replaces the keys of `group` with `bindings`. Returns the ids that could not be bound,
/// e.g. because another app holds the key or the desktop assigned none.
pub async fn bind_group(app: &AppHandle, group: Group, bindings: Vec<Binding>) -> Vec<String> {
    let _guard = OPERATION.lock().await;
    unbind(app, group);
    if bindings.is_empty() {
        return Vec::new();
    }
    match current_backend() {
        Backend::GlobalShortcut => register_all(app, group, bindings),
        Backend::Portal => bind_portal(group, bindings).await,
    }
}

fn unbind(app: &AppHandle, group: Group) {
    let old = with_state(|state| state.groups.remove(&group)).unwrap_or_default();
    #[cfg(target_os = "linux")]
    if let Some(binding) = old.portal {
        binding.close();
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        for key in old.registered {
            if let Err(err) = app.global_shortcut().unregister(key.as_str()) {
                tracing::warn!("unregistering a hotkey failed: {err}");
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let _ = (app, old);
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn register_all(app: &AppHandle, group: Group, bindings: Vec<Binding>) -> Vec<String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let mut registered = Vec::new();
    let mut failed = Vec::new();
    for binding in bindings {
        let key = match normalize_key(&binding.key) {
            Ok(key) => key,
            Err(err) => {
                tracing::warn!("hotkey rejected: {err}");
                failed.push(binding.id);
                continue;
            }
        };
        // Key repeat sends further presses while the key is held; only the first one counts.
        let held = AtomicBool::new(false);
        let sink = binding.sink;
        let result =
            app.global_shortcut()
                .on_shortcut(key.as_str(), move |_app, _shortcut, event| {
                    let pressed = event.state == ShortcutState::Pressed;
                    if held.swap(pressed, Ordering::Relaxed) != pressed {
                        sink(pressed);
                    }
                });
        match result {
            Ok(()) => registered.push(key),
            Err(err) => {
                tracing::warn!("registering a hotkey failed: {err}");
                failed.push(binding.id);
            }
        }
    }
    with_state(|state| state.groups.entry(group).or_default().registered = registered);
    failed
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn register_all(_app: &AppHandle, _group: Group, bindings: Vec<Binding>) -> Vec<String> {
    bindings.into_iter().map(|b| b.id).collect()
}

#[cfg(target_os = "linux")]
async fn bind_portal(group: Group, bindings: Vec<Binding>) -> Vec<String> {
    use crate::ptt::portal;

    // Portal ids are shared by all sessions of the app, so each group gets its own prefix.
    let portal_id = |id: &str| format!("{}-{id}", group.prefix());
    let triggers: Vec<Option<String>> = bindings
        .iter()
        .map(|b| {
            normalize_key(&b.key)
                .ok()
                .and_then(|k| crate::ptt::portal_trigger(&k))
        })
        .collect();
    let ids: Vec<String> = bindings.iter().map(|b| portal_id(&b.id)).collect();
    let requests: Vec<portal::Request<'_>> = bindings
        .iter()
        .zip(&ids)
        .zip(&triggers)
        .map(|((b, id), trigger)| portal::Request {
            id,
            description: &b.description,
            preferred_trigger: trigger.as_deref(),
        })
        .collect();
    let sinks: HashMap<String, Sink> = ids
        .iter()
        .cloned()
        .zip(bindings.iter().map(|b| b.sink.clone()))
        .collect();
    let sink: portal::Sink = Arc::new(move |id, pressed| {
        if let Some(sink) = sinks.get(id) {
            sink(pressed);
        }
    });
    match portal::bind(&requests, sink).await {
        Ok((binding, shortcuts)) => {
            with_state(|state| state.groups.entry(group).or_default().portal = Some(binding));
            bindings
                .iter()
                .zip(&ids)
                .filter(|(_, id)| portal::trigger_of(&shortcuts, id).is_none())
                .map(|(b, _)| b.id.clone())
                .collect()
        }
        Err(err) => {
            tracing::warn!("binding hotkeys via the portal failed: {err}");
            bindings.into_iter().map(|b| b.id).collect()
        }
    }
}

#[cfg(not(target_os = "linux"))]
async fn bind_portal(_group: Group, bindings: Vec<Binding>) -> Vec<String> {
    bindings.into_iter().map(|b| b.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(key: &str, action: HotkeyAction) -> Hotkey {
        Hotkey {
            key: key.into(),
            action,
        }
    }

    #[test]
    fn check_normalises_and_finds_no_conflict() {
        let hotkeys = [hotkey("Ctrl+Shift+F7", HotkeyAction::ToggleMute)];
        let (key, conflict) =
            check("control+shift+KeyD", HotkeySlot::New, &hotkeys, Some("F13")).expect("valid");
        assert_eq!(key, "Ctrl+Shift+D");
        assert_eq!(conflict, None);
    }

    #[test]
    fn check_reports_another_hotkey() {
        let hotkeys = [
            hotkey("Ctrl+Shift+F7", HotkeyAction::ToggleMute),
            hotkey("Alt+F8", HotkeyAction::ToggleDeafen),
        ];
        let (_, conflict) =
            check("shift+control+F7", HotkeySlot::New, &hotkeys, None).expect("valid");
        assert_eq!(conflict, Some(HotkeyConflict::Hotkey { index: 0 }));
        // Assigning a key again to the same action is no conflict.
        let (_, conflict) =
            check("Alt+F8", HotkeySlot::Hotkey { index: 1 }, &hotkeys, None).expect("valid");
        assert_eq!(conflict, None);
    }

    #[test]
    fn check_reports_push_to_talk_except_for_itself() {
        let hotkeys = [hotkey("F9", HotkeyAction::ToggleDnd)];
        let (_, conflict) = check("F13", HotkeySlot::New, &[], Some("F13")).expect("valid");
        assert_eq!(conflict, Some(HotkeyConflict::PushToTalk));
        let (_, conflict) = check("F13", HotkeySlot::PushToTalk, &[], Some("F13")).expect("valid");
        assert_eq!(conflict, None);
        let (_, conflict) = check("F9", HotkeySlot::PushToTalk, &hotkeys, None).expect("valid");
        assert_eq!(conflict, Some(HotkeyConflict::Hotkey { index: 0 }));
        // A mouse button for push-to-talk never clashes with a keyboard key.
        let (_, conflict) = check("F9", HotkeySlot::New, &[], Some("Mouse4")).expect("valid");
        assert_eq!(conflict, None);
    }

    #[test]
    fn check_refuses_mumcords_own_shortcuts() {
        for (key, shortcut) in [
            ("control+shift+KeyM", InAppShortcut::MemberColumn),
            ("Super+Shift+KeyM", InAppShortcut::MemberColumn),
            ("Ctrl+Shift+N", InAppShortcut::NotificationCentre),
            ("Ctrl+KeyK", InAppShortcut::Search),
            ("Meta+KeyF", InAppShortcut::Search),
            ("Ctrl+W", InAppShortcut::CloseWindow),
        ] {
            let (_, conflict) = check(key, HotkeySlot::New, &[], None).expect("valid");
            assert_eq!(conflict, Some(HotkeyConflict::InApp { shortcut }), "{key}");
        }
        // Without Shift, M is free.
        let (_, conflict) = check("Ctrl+M", HotkeySlot::New, &[], None).expect("valid");
        assert_eq!(conflict, None);
    }

    #[test]
    fn in_app_shortcuts_are_canonical() {
        for (key, _) in IN_APP {
            assert_eq!(normalize_key(key).as_deref(), Ok(key));
        }
    }

    #[test]
    fn check_rejects_invalid_keys() {
        assert_eq!(
            check("Shift", HotkeySlot::New, &[], None),
            Err(KeyError::ModifierOnly)
        );
        assert!(check("Mouse4", HotkeySlot::New, &[], None).is_err());
        assert_eq!(check("", HotkeySlot::New, &[], None), Err(KeyError::Empty));
    }

    #[test]
    fn action_ids_are_unique_per_channel() {
        let a = HotkeyAction::SwitchChannel {
            server_id: 1,
            path: vec!["Lobby".into()],
        };
        let b = HotkeyAction::SwitchChannel {
            server_id: 2,
            path: vec!["Lobby".into()],
        };
        assert_ne!(action_id(&a), action_id(&b));
        assert_eq!(action_id(&HotkeyAction::ToggleMute), "mute");
        assert_eq!(action_description(&a), "Switch to channel Lobby");
    }
}
