//! The public server browser (issue #68): the list the official Mumble client shows, and the
//! paced UDP ping of the rows on screen. Only the engine contacts the network.

use mumcord_engine::Engine;
use mumcord_engine::public_servers::PublicListError;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::types::{ApiError, ApiResult, ErrorCode};

/// One listed server. Untrusted list data, already cleaned by the engine; shown as plain text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PublicServer {
    pub name: String,
    pub host: String,
    pub port: u16,
    /// Country name from the list; may be empty.
    pub country: String,
    /// Upper-case ISO 3166 alpha-2 code, or empty.
    pub country_code: String,
}

/// What a listed server answered to the ping. Untrusted server data.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PublicServerPing {
    pub ping_ms: u32,
    pub users: u32,
    pub max_users: u32,
}

/// Downloads the public server list. The UI calls this only when the user opens the browser or
/// presses Refresh (FR-3.92); nothing is fetched in the background.
#[tauri::command]
#[specta::specta]
pub async fn public_servers_list(engine: State<'_, Engine>) -> ApiResult<Vec<PublicServer>> {
    let agent = format!("Mumcord/{}", crate::version::PACKAGE);
    match engine.public_servers(&agent).await {
        Ok(servers) => Ok(servers
            .into_iter()
            .map(|s| PublicServer {
                name: s.name,
                host: s.host,
                port: s.port,
                country: s.country,
                country_code: s.country_code,
            })
            .collect()),
        Err(e) => {
            tracing::warn!(error = %e, "loading the public server list failed");
            let code = match e {
                PublicListError::Fetch(_) => ErrorCode::ServerUnreachable,
                PublicListError::Malformed(_) => ErrorCode::Internal,
            };
            Err(ApiError::new(code, e.to_string()))
        }
    }
}

/// Pings one listed server; `null` when it did not answer. The engine paces all pings (one every
/// 50 ms, a few in flight), whatever the UI asks for.
#[tauri::command]
#[specta::specta]
pub async fn public_server_ping(
    engine: State<'_, Engine>,
    host: String,
    port: u16,
) -> ApiResult<Option<PublicServerPing>> {
    Ok(engine
        .public_server_ping(&host, port)
        .await
        .map(|answer| PublicServerPing {
            ping_ms: answer.ping_ms,
            users: answer.users,
            max_users: answer.max_users,
        }))
}
