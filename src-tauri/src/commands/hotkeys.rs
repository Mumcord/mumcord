//! Global hotkeys beyond push-to-talk (issue #64, FR-3.72, FR-3.73); see [`crate::hotkeys`].

use mumcord_engine::{Engine, store};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{
    ApiError, ApiResult, ErrorCode, ServerId, to_api_server_id, to_engine_server_id,
};
use crate::ptt::{Backend, current_backend};

/// What a global hotkey does.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HotkeyAction {
    ToggleMute,
    ToggleDeafen,
    ToggleDnd,
    /// Leaves voice, or joins voice again on the shown server.
    JoinLeaveVoice,
    /// Moves voice to the next connected server.
    SwitchVoiceServer,
    /// Moves voice to the channel at `path` (names below the root) on `serverId`.
    SwitchChannel {
        #[serde(rename = "serverId")]
        server_id: ServerId,
        path: Vec<String>,
    },
    PreviousChannel,
    ToggleMemberColumn,
    OpenQuickSettings,
    OpenMumcord,
}

impl From<store::HotkeyAction> for HotkeyAction {
    fn from(action: store::HotkeyAction) -> Self {
        use store::HotkeyAction as S;
        match action {
            S::ToggleMute => Self::ToggleMute,
            S::ToggleDeafen => Self::ToggleDeafen,
            S::ToggleDnd => Self::ToggleDnd,
            S::JoinLeaveVoice => Self::JoinLeaveVoice,
            S::SwitchVoiceServer => Self::SwitchVoiceServer,
            S::SwitchChannel { server_id, path } => Self::SwitchChannel {
                server_id: to_api_server_id(server_id),
                path,
            },
            S::PreviousChannel => Self::PreviousChannel,
            S::ToggleMemberColumn => Self::ToggleMemberColumn,
            S::OpenQuickSettings => Self::OpenQuickSettings,
            S::OpenMumcord => Self::OpenMumcord,
        }
    }
}

impl From<HotkeyAction> for store::HotkeyAction {
    fn from(action: HotkeyAction) -> Self {
        use HotkeyAction as A;
        match action {
            A::ToggleMute => Self::ToggleMute,
            A::ToggleDeafen => Self::ToggleDeafen,
            A::ToggleDnd => Self::ToggleDnd,
            A::JoinLeaveVoice => Self::JoinLeaveVoice,
            A::SwitchVoiceServer => Self::SwitchVoiceServer,
            A::SwitchChannel { server_id, path } => Self::SwitchChannel {
                server_id: to_engine_server_id(server_id),
                path,
            },
            A::PreviousChannel => Self::PreviousChannel,
            A::ToggleMemberColumn => Self::ToggleMemberColumn,
            A::OpenQuickSettings => Self::OpenQuickSettings,
            A::OpenMumcord => Self::OpenMumcord,
        }
    }
}

/// A global hotkey: the key as returned by `hotkey_check` and its action.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    pub key: String,
    pub action: HotkeyAction,
}

impl From<store::Hotkey> for Hotkey {
    fn from(hotkey: store::Hotkey) -> Self {
        Self {
            key: hotkey.key,
            action: hotkey.action.into(),
        }
    }
}

impl From<Hotkey> for store::Hotkey {
    fn from(hotkey: Hotkey) -> Self {
        Self {
            key: hotkey.key,
            action: hotkey.action.into(),
        }
    }
}

/// The key being set: push-to-talk, an existing hotkey (index into `hotkeys`), or a new one.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HotkeySlot {
    PushToTalk,
    Hotkey { index: u32 },
    New,
}

/// Mumcord's own window shortcuts, which a global hotkey would swallow.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum InAppShortcut {
    /// Ctrl+Shift+M: show or hide the member column.
    MemberColumn,
    /// Ctrl+Shift+N: the notification centre.
    NotificationCentre,
    /// Ctrl+K, Ctrl+F: search.
    Search,
    /// Ctrl+W: close a separate window.
    CloseWindow,
}

/// What already uses a key.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HotkeyConflict {
    /// The hotkey at `index` in `hotkeys`.
    Hotkey {
        index: u32,
    },
    PushToTalk,
    /// The hold-to-talk key of a whisper target (#67), named by the user.
    #[serde(rename_all = "camelCase")]
    Whisper {
        target_id: String,
        name: String,
    },
    /// One of Mumcord's own shortcuts; cannot be cleared.
    InApp {
        shortcut: InAppShortcut,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyCheck {
    /// The key in the stored form, e.g. `"Ctrl+Shift+D"`.
    pub key: String,
    pub conflict: Option<HotkeyConflict>,
}

/// How hotkeys reach Mumcord in this session.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum HotkeysMode {
    /// X11, Windows, macOS, Wayland compatibility mode.
    Global,
    /// Native Wayland: the desktop asks once to allow the shortcuts.
    Portal,
    /// Native Wayland without the global-shortcuts portal: only while Mumcord has the focus.
    Unavailable,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct HotkeysStatus {
    pub mode: HotkeysMode,
    /// Indices into `hotkeys` whose key could not be registered (taken by another app, or the
    /// desktop assigned none).
    pub failed: Vec<u32>,
}

/// A global hotkey was pressed; the UI runs its action. "Open Mumcord" never arrives here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyPressed {
    pub action: HotkeyAction,
}

/// Checks a captured key for `slot`: returns it in the stored form and what already uses it.
/// `invalidInput` for keys that cannot be a global shortcut (modifiers only, mouse buttons).
#[tauri::command]
#[specta::specta]
pub fn hotkey_check(
    engine: State<'_, Engine>,
    key: String,
    slot: HotkeySlot,
) -> ApiResult<HotkeyCheck> {
    let settings = engine.settings()?;
    let (key, conflict) =
        crate::hotkeys::check(&key, slot, &settings.hotkeys, settings.ptt_key.as_deref())
            .map(|(key, conflict)| {
                let conflict =
                    conflict.or_else(|| crate::hotkeys::whisper_conflict(&key, &settings));
                (key, conflict)
            })
            .map_err(|err| ApiError::new(ErrorCode::InvalidInput, err.to_string()))?;
    Ok(HotkeyCheck { key, conflict })
}

/// How hotkeys work in this session and which of them could not be registered.
#[tauri::command]
#[specta::specta]
pub async fn hotkeys_status(engine: State<'_, Engine>) -> ApiResult<HotkeysStatus> {
    let hotkeys = engine.settings()?.hotkeys;
    let mode = match current_backend() {
        Backend::GlobalShortcut => HotkeysMode::Global,
        Backend::Portal if crate::ptt::portal_reachable().await => HotkeysMode::Portal,
        Backend::Portal => HotkeysMode::Unavailable,
    };
    Ok(HotkeysStatus {
        mode,
        failed: crate::hotkeys::failed(&hotkeys),
    })
}

/// (Re)binds the hotkeys in the background after a settings change or at start-up.
pub fn configure(app: &tauri::AppHandle, settings: &store::Settings) {
    let (app, settings) = (app.clone(), settings.clone());
    tauri::async_runtime::spawn(async move {
        crate::hotkeys::configure(&app, &settings).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_round_trip_through_the_store() {
        let action = HotkeyAction::SwitchChannel {
            server_id: 4,
            path: vec!["Night Owls".into()],
        };
        let stored: store::HotkeyAction = action.clone().into();
        assert_eq!(HotkeyAction::from(stored), action);
        let json = serde_json::to_value(&action).expect("serialise");
        assert_eq!(
            json,
            serde_json::json!({"kind": "switchChannel", "serverId": 4, "path": ["Night Owls"]})
        );
        let json = serde_json::to_value(HotkeyAction::ToggleDnd).expect("serialise");
        assert_eq!(json, serde_json::json!({"kind": "toggleDnd"}));
    }
}
