//! Positional audio status (FR-1.41–1.43, FR-1.72). The switch and the distances live in the
//! settings record (`settings_update`).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::ApiResult;

/// What the settings show (FR-1.43) and who gets the "3D" icon (FR-1.72).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PositionalStatus {
    pub enabled: bool,
    /// The source delivering positions right now ("Mumble Link" or a plugin name); `null` while
    /// no game delivers any. Plain text.
    pub source: Option<String>,
    /// The game's own name, if it reports one. Plain text.
    pub game: Option<String>,
    /// Sessions currently sending positions, the own one included.
    pub senders: Vec<u32>,
}

/// Positional audio was switched or the game delivering positions changed. `senders` is a
/// snapshot; follow `PositionalSendingChanged` afterwards.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PositionalStatusChanged {
    pub status: PositionalStatus,
}

/// A person (or the user, with the own session) started or stopped sending positions: show or
/// hide the "3D" icon. Forget all on a new `serverSynced` and when a person leaves.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PositionalSendingChanged {
    pub session: u32,
    pub sending: bool,
}

impl From<mumcord_engine::PositionalStatus> for PositionalStatus {
    fn from(s: mumcord_engine::PositionalStatus) -> Self {
        Self {
            enabled: s.enabled,
            source: s.source,
            game: s.game,
            senders: s.senders,
        }
    }
}

/// The current positional audio status.
#[tauri::command]
#[specta::specta]
pub fn positional_status(engine: State<'_, Engine>) -> ApiResult<PositionalStatus> {
    Ok(engine.positional_status().into())
}
