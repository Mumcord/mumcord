//! Do Not Disturb (FR-3.111, FR-3.112): the Mumcord switch, "Follow system Do Not Disturb" and
//! the system state Mumcord reads.

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::types::ApiResult;
use crate::dnd;

/// The desktop's own Do Not Disturb or Focus mode as far as Mumcord can read it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemDnd {
    /// The desktop whose state is read, e.g. "KDE Plasma"; `null` when none can be read.
    pub desktop: Option<String>,
    /// Whether the system's Do Not Disturb is on; `null` when it can't be read.
    pub on: Option<bool>,
}

/// Everything the UI shows about Do Not Disturb.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct DndState {
    /// The Mumcord switch (quick settings, settings, tray).
    pub manual: bool,
    /// "Follow system Do Not Disturb".
    pub follow_system: bool,
    pub system: SystemDnd,
}

impl DndState {
    /// Whether Do Not Disturb is in effect: the switch, or the followed system state.
    pub fn active(&self) -> bool {
        self.manual || (self.follow_system && self.system.on == Some(true))
    }
}

/// The switch, the follow setting or the system state changed (also from the tray).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DndChanged {
    pub state: DndState,
}

#[tauri::command]
#[specta::specta]
pub fn dnd_state(app: AppHandle, engine: State<'_, Engine>) -> ApiResult<DndState> {
    let settings = engine.settings()?;
    Ok(dnd::state_from(&settings, dnd::system(&app)))
}

/// Turns the Mumcord switch on or off; it is saved and applies to all servers at once.
#[tauri::command]
#[specta::specta]
pub async fn dnd_set(app: AppHandle, on: bool) -> ApiResult<DndState> {
    dnd::set(&app, |s| s.do_not_disturb = on).await
}

/// "Follow system Do Not Disturb".
#[tauri::command]
#[specta::specta]
pub async fn dnd_set_follow_system(app: AppHandle, on: bool) -> ApiResult<DndState> {
    dnd::set(&app, |s| s.follow_system_dnd = on).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(manual: bool, follow_system: bool, on: Option<bool>) -> DndState {
        DndState {
            manual,
            follow_system,
            system: SystemDnd {
                desktop: on.map(|_| "KDE Plasma".into()),
                on,
            },
        }
    }

    #[test]
    fn the_switch_or_the_followed_system_state_turns_it_on() {
        assert!(!state(false, true, None).active());
        assert!(!state(false, true, Some(false)).active());
        assert!(state(false, true, Some(true)).active());
        assert!(!state(false, false, Some(true)).active(), "not followed");
        assert!(state(true, false, None).active());
        assert!(state(true, true, Some(false)).active());
    }
}
