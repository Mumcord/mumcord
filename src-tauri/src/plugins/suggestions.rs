//! Server-suggested plugins (FR-6.77–6.79): a registered bot's `mumcord.srvplugins` list goes
//! through the B34 logic and the survivors reach the notification centre. Nothing is ever
//! sent back to the bot.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mumcord_engine::plugins::PluginPeer;
use mumcord_engine::{Engine, ServerId};
use mumcord_plugins::registry::{Client, UrlPolicy, fetch_official, official_base};
use mumcord_plugins::service::cached_revocations;
use mumcord_plugins::suggestions::{self, Environment, Suggestion, SuggestionSource};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::{Plugins, identity_of};
use crate::commands::types::{
    ApiError, ApiResult, ErrorCode, ServerId as ApiServerId, to_api_server_id, to_engine_server_id,
};

/// Where a suggested plugin comes from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginSuggestionSource {
    /// The official list: Install or Details.
    Official { version: String },
    /// A third-party repository: opens the Add repository dialog with `key` as Expected key.
    Repository { url: String, key: Option<String> },
}

/// `plugins://suggestion`: the server suggests a plugin. Every text is untrusted plain text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "plugins://suggestion")]
pub struct PluginSuggestion {
    pub server_id: ApiServerId,
    pub plugin: String,
    /// The plugin's name from the official list; the id for third-party plugins.
    pub name: String,
    /// Why the server suggests it, written by the server.
    pub reason: String,
    /// The announcement number as text (a new one makes "Not now" lapse).
    pub seq: String,
    pub source: PluginSuggestionSource,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// Makes sure the official list is known; the UI may not have opened the store yet. A cached
/// copy is good enough, so an offline start still resolves suggestions.
pub(crate) async fn ensure_index(plugins: &Arc<Plugins>, engine: &Engine) {
    if plugins.index().is_some() {
        return;
    }
    let result = async {
        let settings = engine.settings().map_err(|e| e.to_string())?;
        let client =
            Client::new(UrlPolicy::new(settings.developer_mode)).map_err(|e| e.to_string())?;
        let base =
            official_base(settings.plugins_mirror_url.as_deref()).map_err(|e| e.to_string())?;
        let cache = plugins.registry_cache().join("official");
        std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let last_seen = u64::try_from(settings.plugins_last_official_sequence)
            .ok()
            .filter(|s| *s > 0);
        fetch_official(&client, &cache, &base, last_seen)
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    match result {
        Ok(loaded) => *plugins.index() = Some(loaded.index),
        Err(e) => tracing::info!(error = %e, "official plugin list unavailable for suggestions"),
    }
}

/// Handles one `mumcord.srvplugins` message from the pinned, registered sender.
pub async fn handle(
    app: &AppHandle,
    plugins: &Arc<Plugins>,
    engine: &Engine,
    server: ServerId,
    sender: PluginPeer,
    data: Vec<u8>,
) {
    let Some(pin) = sender.cert_hash else {
        return;
    };
    let settings = match engine.settings() {
        Ok(settings) => settings,
        Err(e) => {
            tracing::warn!(error = %e, "reading settings for plugin suggestions failed");
            return;
        }
    };
    let Some(announcement) = suggestions::parse(&data, settings.developer_mode) else {
        return;
    };
    let Some(identity) = identity_of(app, Some(server)).await else {
        return;
    };
    ensure_index(plugins, engine).await;
    let installed: Vec<String> = match plugins
        .call(|s| Ok(s.list()?.into_iter().map(|p| p.id).collect()))
        .await
    {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(error = %e.message, "listing plugins for suggestions failed");
            return;
        }
    };
    let language = settings.language.clone().unwrap_or_else(|| "en".to_owned());
    let store = engine.store_handle();
    let worker = Arc::clone(plugins);
    let processed = tauri::async_runtime::spawn_blocking(move || {
        let revoked = cached_revocations(&worker.registry_cache().join("official"));
        let index = worker.index();
        let is_installed = |id: &str| installed.iter().any(|i| i == id);
        let env = Environment {
            index: index.as_ref(),
            revoked: &revoked,
            installed: &is_installed,
        };
        let store = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let shown = suggestions::process(
            &store,
            server,
            identity,
            &pin,
            &announcement,
            &env,
            now_millis(),
        )?;
        Ok::<_, mumcord_engine::store::Error>(
            shown
                .into_iter()
                .map(|s| {
                    let name = index
                        .as_ref()
                        .and_then(|i| i.plugin(&s.plugin_id))
                        .map_or_else(|| s.plugin_id.clone(), |p| p.name(&language).to_owned());
                    (s, name)
                })
                .collect::<Vec<_>>(),
        )
    })
    .await;
    let shown = match processed {
        Ok(Ok(shown)) => shown,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "processing plugin suggestions failed");
            return;
        }
        Err(e) => {
            tracing::warn!(error = %e, "processing plugin suggestions failed");
            return;
        }
    };
    for (suggestion, name) in shown {
        let event = notice(server, suggestion, name);
        if let Err(e) = event.emit(app) {
            tracing::warn!(error = %e, "emitting a plugin suggestion failed");
        }
    }
}

fn notice(server: ServerId, suggestion: Suggestion, name: String) -> PluginSuggestion {
    PluginSuggestion {
        server_id: to_api_server_id(server),
        plugin: suggestion.plugin_id,
        name,
        reason: suggestion.reason,
        seq: suggestion.seq.to_string(),
        source: match suggestion.source {
            SuggestionSource::Official { version } => PluginSuggestionSource::Official { version },
            SuggestionSource::Repository { url, key } => {
                PluginSuggestionSource::Repository { url, key }
            }
        },
    }
}

async fn identity_for(
    app: &AppHandle,
    server_id: ApiServerId,
) -> ApiResult<(ServerId, mumcord_engine::IdentityId)> {
    let server = to_engine_server_id(server_id);
    identity_of(app, Some(server))
        .await
        .map(|identity| (server, identity))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "no identity for this server"))
}

/// "Not now": the suggestion stays hidden until the server's list changes or 30 days pass.
#[tauri::command]
#[specta::specta]
pub async fn plugin_suggestion_not_now(
    app: AppHandle,
    engine: State<'_, Engine>,
    server_id: ApiServerId,
    plugin: String,
) -> ApiResult<()> {
    let (server, identity) = identity_for(&app, server_id).await?;
    let store = engine.store_handle();
    let store = store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    suggestions::not_now(&store, server, identity, &plugin, now_millis())
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))
}

/// "Don't ask again for this server".
#[tauri::command]
#[specta::specta]
pub async fn plugin_suggestions_dismiss_server(
    app: AppHandle,
    engine: State<'_, Engine>,
    server_id: ApiServerId,
) -> ApiResult<()> {
    let (server, identity) = identity_for(&app, server_id).await?;
    let store = engine.store_handle();
    let store = store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    suggestions::dismiss_server(&store, server, identity)
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_suggestion_keeps_its_expected_key() {
        let shown = notice(
            ServerId::from(3),
            Suggestion {
                plugin_id: "other".into(),
                reason: "Fun".into(),
                seq: 9_007_199_254_740_993,
                source: SuggestionSource::Repository {
                    url: "https://example.org/r".into(),
                    key: Some("abc".into()),
                },
            },
            "other".into(),
        );
        assert_eq!(shown.seq, "9007199254740993");
        assert_eq!(
            shown.source,
            PluginSuggestionSource::Repository {
                url: "https://example.org/r".into(),
                key: Some("abc".into())
            }
        );
    }
}
