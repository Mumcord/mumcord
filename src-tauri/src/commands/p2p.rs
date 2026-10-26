//! Direct connections between Mumcord users (FR-2.35, FR-2.36).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiResult, ServerId};

/// A direct connection would be used for the first time while `allowDirectConnections` is
/// still `unset`; the UI asks once and answers with `p2p_consent_decide`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DirectConsentRequired {
    pub server_id: ServerId,
}

/// Stores the answer to `directConsentRequired` as `allowDirectConnections` and continues or
/// drops the waiting direct connection.
#[tauri::command]
#[specta::specta]
pub async fn p2p_consent_decide(engine: State<'_, Engine>, allow: bool) -> ApiResult<()> {
    Ok(engine.p2p_consent_decide(allow).await?)
}
