//! Connection quality, client and OS of people from Mumble UserStats (FR-2.25 – FR-2.27).

use mumcord_engine::{Engine, UserStatsView};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::session::Transport;
use super::types::{ApiResult, ServerId, SessionId, to_engine_server_id};

/// One person's connection figures; a field is `null` when the server did not return it
/// (loss, late and bandwidth only for the own channel, client and OS only for the own user or
/// to admins).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct UserStats {
    /// Average ping of the transport the person's voice uses.
    pub ping_ms: Option<f32>,
    pub transport: Option<Transport>,
    /// Lost voice packets in both directions since the person connected, 0–100.
    pub loss_percent: Option<f32>,
    /// Late voice packets in both directions since the person connected, 0–100.
    pub late_percent: Option<f32>,
    pub bandwidth_kbps: Option<u32>,
    /// Client release, e.g. "Mumble 1.5.735" or "Mumcord 1.2.0". Untrusted: render as plain
    /// text (control characters already removed, at most 64 characters).
    pub client_release: Option<String>,
    /// Protocol version of the client, e.g. "1.5.735".
    pub client_version: Option<String>,
    /// Operating system and its version; untrusted plain text like `client_release`.
    pub os: Option<String>,
    pub os_version: Option<String>,
}

impl From<UserStatsView> for UserStats {
    fn from(v: UserStatsView) -> Self {
        Self {
            ping_ms: v.ping_ms,
            transport: v.transport.map(Into::into),
            loss_percent: v.loss_percent,
            late_percent: v.late_percent,
            bandwidth_kbps: v.bandwidth_kbps,
            client_release: v.client_release,
            client_version: v.client_version,
            os: v.os,
            os_version: v.os_version,
        }
    }
}

/// New UserStats arrived for a person on the server.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UserStatsChanged {
    pub server_id: ServerId,
    pub session: SessionId,
    pub stats: UserStats,
}

/// Tells the engine which people of the server are visible right now; it requests their
/// UserStats at a paced rate (at most one request every 2 s per server, people in the user's
/// channel first, each person at most every 30 s) and emits `userStatsChanged`. An empty list
/// stops the requests. Fails with `notConnected` if the server is not connected.
#[tauri::command]
#[specta::specta]
pub fn user_stats_watch(
    engine: State<'_, Engine>,
    server_id: ServerId,
    sessions: Vec<SessionId>,
) -> ApiResult<()> {
    Ok(engine.user_stats_watch(to_engine_server_id(server_id), sessions)?)
}
