//! Positional audio status and the official game plugins (FR-1.41–1.43, FR-1.72). The switch
//! and the distances live in the settings record (`settings_update`); so do the enabled
//! plugins, but they are changed through `plugin_set_enabled`.

use mumcord_engine::Engine;
use mumcord_engine::positional_sources::{
    LinkSource, PluginHost, PluginInfo, PluginProblem, PositionSource, plugins::PluginHostError,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiError, ApiResult, ErrorCode};

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

/// Why a listed game plugin delivers no positions.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum GamePluginProblem {
    /// Built for a plugin interface Mumcord does not implement.
    Incompatible,
    /// The plugin failed to start.
    InitFailed,
    /// The plugin cannot work on this system.
    Unsupported,
    /// The plugin failed and is not used until it is enabled again.
    Failed,
}

/// An official Mumble game plugin found on this computer (FR-1.42b).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct GamePlugin {
    /// Stable identifier (the file name), as stored in the settings.
    pub id: String,
    /// Name the plugin reports. Plain text from a third-party library.
    pub name: String,
    /// Version the plugin reports. Plain text.
    pub version: Option<String>,
    pub enabled: bool,
    /// The plugin found its game and delivers positions.
    pub active: bool,
    pub problem: Option<GamePluginProblem>,
}

impl From<PluginInfo> for GamePlugin {
    fn from(p: PluginInfo) -> Self {
        Self {
            id: p.id,
            name: p.name,
            version: p.version,
            enabled: p.enabled,
            active: p.active,
            problem: p.problem.map(|problem| match problem {
                PluginProblem::Incompatible => GamePluginProblem::Incompatible,
                PluginProblem::InitFailed => GamePluginProblem::InitFailed,
                PluginProblem::Unsupported => GamePluginProblem::Unsupported,
                PluginProblem::Failed => GamePluginProblem::Failed,
            }),
        }
    }
}

impl From<PluginHostError> for ApiError {
    fn from(e: PluginHostError) -> Self {
        ApiError::new(ErrorCode::Internal, e.to_string())
    }
}

/// Enables exactly `ids` in the plugin host and hands the engine its position sources:
/// Mumble Link first, then the enabled plugins in the stored order.
pub fn apply_plugins(
    engine: &Engine,
    host: &PluginHost,
    ids: &[String],
) -> Result<(), PluginHostError> {
    host.set_enabled(ids)?;
    let mut sources: Vec<Box<dyn PositionSource>> = vec![Box::new(LinkSource::new())];
    sources.extend(host.sources(ids));
    engine.set_position_sources(sources);
    Ok(())
}

/// Lists the plugins without holding up the async runtime (the first call loads them).
async fn list(host: &PluginHost) -> ApiResult<Vec<PluginInfo>> {
    let host = host.clone();
    tauri::async_runtime::spawn_blocking(move || host.list())
        .await
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
        .map_err(ApiError::from)
}

/// The official Mumble game plugins found in Mumcord's and Mumble's plugin folders.
#[tauri::command]
#[specta::specta]
pub async fn plugins_list(host: State<'_, PluginHost>) -> ApiResult<Vec<GamePlugin>> {
    Ok(list(&host).await?.into_iter().map(Into::into).collect())
}

/// Enables or disables one game plugin and stores the choice; returns the updated list.
#[tauri::command]
#[specta::specta]
pub async fn plugin_set_enabled(
    engine: State<'_, Engine>,
    host: State<'_, PluginHost>,
    id: String,
    enabled: bool,
) -> ApiResult<Vec<GamePlugin>> {
    if enabled && !list(&host).await?.iter().any(|p| p.id == id) {
        return Err(ApiError::new(ErrorCode::NotFound, "no such game plugin"));
    }
    let mut stored = engine.settings()?;
    stored.enabled_plugins = with_plugin(stored.enabled_plugins, id, enabled);
    let stored = engine.update_settings(stored).await?;
    apply_plugins(&engine, &host, &stored.enabled_plugins)?;
    Ok(list(&host).await?.into_iter().map(Into::into).collect())
}

/// The enabled plugin identifiers with `id` switched; a newly enabled plugin comes last.
fn with_plugin(mut ids: Vec<String>, id: String, enabled: bool) -> Vec<String> {
    ids.retain(|p| *p != id);
    if enabled {
        ids.push(id);
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_a_plugin_keeps_the_others_in_order() {
        let ids = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            with_plugin(ids(&["a", "b"]), "c".into(), true),
            ids(&["a", "b", "c"])
        );
        assert_eq!(
            with_plugin(ids(&["a", "b"]), "a".into(), true),
            ids(&["b", "a"])
        );
        assert_eq!(
            with_plugin(ids(&["a", "b"]), "a".into(), false),
            ids(&["b"])
        );
        assert_eq!(with_plugin(ids(&["a"]), "x".into(), false), ids(&["a"]));
    }

    #[test]
    fn plugin_problems_map_one_to_one() {
        let info = PluginInfo {
            id: "gtav".into(),
            name: "Grand Theft Auto V".into(),
            version: None,
            enabled: true,
            active: false,
            problem: Some(PluginProblem::InitFailed),
        };
        let plugin = GamePlugin::from(info);
        assert_eq!(plugin.problem, Some(GamePluginProblem::InitFailed));
        assert_eq!(plugin.name, "Grand Theft Auto V");
    }
}
