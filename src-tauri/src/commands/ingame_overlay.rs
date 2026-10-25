//! In-game overlay settings API (FR-1.46b), consumed by the settings page. The overlay drawing,
//! IPC server and layer install live in `crate::ingame_overlay`; this file is the typed contract.

use mumcord_engine::store::{self, GameKey as StoreKey};
use mumcord_engine::{Engine, OverlayDecision};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::ingame_overlay::Runtime;

use super::types::{ApiError, ApiResult, ErrorCode};

/// Global switch of the in-game overlay.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum InGameOverlayMode {
    Off,
    On,
}

/// How a game is identified: its Steam app id, or its executable name.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum GameKey {
    Steam(u32),
    Exe(String),
}

/// Screen corner (same values as the window overlay).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Per-game options sent from the settings page. Ranges are clamped on save.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameOverlayEntry {
    pub key: GameKey,
    /// Display name; plain text.
    pub name: String,
    /// `null` follows the global mode; `true`/`false` overrides it for this game.
    pub enabled: Option<bool>,
    /// The stronger anti-cheat warning was confirmed for this game (needed to enable a
    /// blocklisted game, QA.56).
    pub risk_confirmed: bool,
    pub corner: Corner,
    /// Percent of the default size, 50–200.
    pub size_percent: u8,
    /// Percent, 20–100.
    pub opacity_percent: u8,
    pub show_whole_channel: bool,
    pub show_channel_name: bool,
    /// Blocklist entry when the game is on the bundled blocklist, else `null` (read-only).
    pub blocklist: Option<BlocklistInfo>,
    /// What the overlay currently does for this game given the whole settings (read-only).
    pub effective: Effect,
}

/// Why a game shows the overlay or not.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    /// Drawn in this game.
    Show,
    /// Global mode is off.
    GlobalOff,
    /// Switched off for this game.
    GameOff,
    /// On the blocklist and not confirmed.
    Blocked,
}

impl From<OverlayDecision> for Effect {
    fn from(d: OverlayDecision) -> Self {
        match d {
            OverlayDecision::Show => Effect::Show,
            OverlayDecision::GlobalOff => Effect::GlobalOff,
            OverlayDecision::GameOff => Effect::GameOff,
            OverlayDecision::Blocked => Effect::Blocked,
        }
    }
}

/// A game on the bundled blocklist (QA.56), shown read-only in the settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct BlocklistInfo {
    pub name: String,
    pub anti_cheat: String,
    pub reason: String,
    pub source: String,
}

/// A game offered to the user to configure: one it ran this session, or one on the blocklist.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct KnownGame {
    pub key: GameKey,
    pub name: String,
    /// It is running right now (connected to the overlay this session).
    pub running: bool,
    pub blocklist: Option<BlocklistInfo>,
}

/// Everything the settings page needs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct InGameOverlayState {
    /// This platform can draw inside games.
    pub supported: bool,
    /// i18n key (namespace `settings`) explaining a platform limitation, or `null`.
    pub explanation_key: Option<String>,
    pub mode: InGameOverlayMode,
    /// Games the user configured, each annotated with its blocklist status and current effect.
    pub games: Vec<GameOverlayEntry>,
    /// Games the user could add: ones running now or on the blocklist, minus the configured ones.
    pub known: Vec<KnownGame>,
    /// Keys of every game running right now, configured or not.
    pub running: Vec<GameKey>,
}

/// The in-game overlay settings changed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct InGameOverlayChanged {
    pub state: InGameOverlayState,
}

const SIZE_RANGE: (u8, u8) = (50, 200);
const OPACITY_RANGE: (u8, u8) = (20, 100);

fn to_store_key(key: &GameKey) -> StoreKey {
    match key {
        GameKey::Steam(id) => StoreKey::Steam(*id),
        GameKey::Exe(exe) => StoreKey::Exe(exe.clone()),
    }
}

fn from_store_key(key: &StoreKey) -> GameKey {
    match key {
        StoreKey::Steam(id) => GameKey::Steam(*id),
        StoreKey::Exe(exe) => GameKey::Exe(exe.clone()),
    }
}

fn to_store_corner(corner: Corner) -> store::OverlayCorner {
    match corner {
        Corner::TopLeft => store::OverlayCorner::TopLeft,
        Corner::TopRight => store::OverlayCorner::TopRight,
        Corner::BottomLeft => store::OverlayCorner::BottomLeft,
        Corner::BottomRight => store::OverlayCorner::BottomRight,
    }
}

fn from_store_corner(corner: store::OverlayCorner) -> Corner {
    match corner {
        store::OverlayCorner::TopLeft => Corner::TopLeft,
        store::OverlayCorner::TopRight => Corner::TopRight,
        store::OverlayCorner::BottomLeft => Corner::BottomLeft,
        store::OverlayCorner::BottomRight => Corner::BottomRight,
    }
}

fn blocklist_info(key: &StoreKey) -> Option<BlocklistInfo> {
    mumcord_engine::blocklist()
        .find(key)
        .map(|g| BlocklistInfo {
            name: g.name.clone(),
            anti_cheat: g.anti_cheat.clone(),
            reason: g.reason.clone(),
            source: g.source.clone(),
        })
}

fn to_ui_entry(
    entry: &store::GameOverlayEntry,
    settings: &store::InGameOverlaySettings,
) -> GameOverlayEntry {
    GameOverlayEntry {
        key: from_store_key(&entry.key),
        name: entry.name.clone(),
        enabled: entry.enabled,
        risk_confirmed: entry.risk_confirmed,
        corner: from_store_corner(entry.corner),
        size_percent: entry.size_percent,
        opacity_percent: entry.opacity_percent,
        show_whole_channel: entry.show_whole_channel,
        show_channel_name: entry.show_channel_name,
        blocklist: blocklist_info(&entry.key),
        effective: mumcord_engine::decide_overlay(settings, &entry.key).into(),
    }
}

fn state_of(engine: &Engine, runtime: &Runtime) -> ApiResult<InGameOverlayState> {
    let stored = engine.settings()?;
    let settings = &stored.ingame_overlay;
    let support = crate::ingame_overlay::support();
    let games: Vec<GameOverlayEntry> = settings
        .games
        .iter()
        .map(|e| to_ui_entry(e, settings))
        .collect();

    // Games to offer: running ones (seen this session) and the whole blocklist, minus configured.
    let configured: Vec<StoreKey> = settings.games.iter().map(|e| e.key.clone()).collect();
    let is_configured = |key: &StoreKey| configured.iter().any(|k| k.matches(key));
    let running: Vec<(StoreKey, String)> = runtime
        .shared
        .seen_games()
        .into_iter()
        .map(|g| {
            let name = blocklist_info(&g.key)
                .map(|b| b.name)
                .unwrap_or_else(|| default_name(&g.key, &g.exe));
            (g.key, name)
        })
        .collect();
    let mut known: Vec<KnownGame> = Vec::new();
    for (key, name) in &running {
        if !is_configured(key) {
            known.push(KnownGame {
                key: from_store_key(key),
                name: name.clone(),
                running: true,
                blocklist: blocklist_info(key),
            });
        }
    }
    for game in mumcord_engine::blocklist().iter() {
        let key = game.first_key();
        if !is_configured(&key) && !known.iter().any(|k| from_store_key(&key) == k.key) {
            known.push(KnownGame {
                key: from_store_key(&key),
                name: game.name.clone(),
                running: false,
                blocklist: blocklist_info(&key),
            });
        }
    }

    let running_keys = running.iter().map(|(key, _)| from_store_key(key)).collect();

    Ok(InGameOverlayState {
        supported: support.supported,
        explanation_key: support.explanation_key.map(str::to_owned),
        mode: match settings.mode {
            store::InGameOverlayMode::Off => InGameOverlayMode::Off,
            store::InGameOverlayMode::On => InGameOverlayMode::On,
        },
        games,
        known,
        running: running_keys,
    })
}

fn default_name(key: &StoreKey, exe: &str) -> String {
    match key {
        StoreKey::Exe(name) if !name.is_empty() => name.clone(),
        StoreKey::Steam(id) if exe.is_empty() => format!("Steam app {id}"),
        _ => exe.to_owned(),
    }
}

async fn save_and_announce(
    app: &AppHandle,
    engine: &Engine,
    runtime: &Runtime,
    mut stored: store::Settings,
) -> ApiResult<InGameOverlayState> {
    stored.ingame_overlay.games.retain(|e| match &e.key {
        StoreKey::Exe(exe) => !exe.is_empty(),
        StoreKey::Steam(_) => true,
    });
    let stored = engine.update_settings(stored).await?;
    crate::ingame_overlay::apply(runtime, &stored);
    let state = state_of(engine, runtime)?;
    if let Err(e) = (InGameOverlayChanged {
        state: state.clone(),
    })
    .emit(app)
    {
        tracing::warn!(error = %e, "announcing the in-game overlay state failed");
    }
    Ok(state)
}

/// Current in-game overlay settings, platform support and the games that can be configured.
#[tauri::command]
#[specta::specta]
pub fn ingame_overlay_state(
    engine: State<'_, Engine>,
    runtime: State<'_, Runtime>,
) -> ApiResult<InGameOverlayState> {
    state_of(&engine, &runtime)
}

/// Sets the global mode (off/on).
#[tauri::command]
#[specta::specta]
pub async fn ingame_overlay_set_mode(
    app: AppHandle,
    engine: State<'_, Engine>,
    runtime: State<'_, Runtime>,
    mode: InGameOverlayMode,
) -> ApiResult<InGameOverlayState> {
    let mut stored = engine.settings()?;
    stored.ingame_overlay.mode = match mode {
        InGameOverlayMode::Off => store::InGameOverlayMode::Off,
        InGameOverlayMode::On => store::InGameOverlayMode::On,
    };
    save_and_announce(&app, &engine, &runtime, stored).await
}

/// Adds or replaces a game's options. Enabling a blocklisted game needs `riskConfirmed`; without
/// it the engine keeps the overlay off for that game (the command still stores the choice).
#[tauri::command]
#[specta::specta]
pub async fn ingame_overlay_set_game(
    app: AppHandle,
    engine: State<'_, Engine>,
    runtime: State<'_, Runtime>,
    entry: GameOverlayEntry,
) -> ApiResult<InGameOverlayState> {
    if let GameKey::Exe(exe) = &entry.key
        && exe.trim().is_empty()
    {
        return Err(ApiError::new(
            ErrorCode::InvalidInput,
            "a game needs a Steam id or executable name",
        ));
    }
    let stored_entry = store::GameOverlayEntry {
        key: to_store_key(&entry.key),
        name: entry.name.clone(),
        enabled: entry.enabled,
        risk_confirmed: entry.risk_confirmed,
        corner: to_store_corner(entry.corner),
        size_percent: entry.size_percent.clamp(SIZE_RANGE.0, SIZE_RANGE.1),
        opacity_percent: entry
            .opacity_percent
            .clamp(OPACITY_RANGE.0, OPACITY_RANGE.1),
        show_whole_channel: entry.show_whole_channel,
        show_channel_name: entry.show_channel_name,
    };
    let mut stored = engine.settings()?;
    let key = stored_entry.key.clone();
    stored.ingame_overlay.games.retain(|e| !e.key.matches(&key));
    stored.ingame_overlay.games.push(stored_entry);
    save_and_announce(&app, &engine, &runtime, stored).await
}

/// Removes a game's options (it then follows the global mode again, or the blocklist default).
#[tauri::command]
#[specta::specta]
pub async fn ingame_overlay_remove_game(
    app: AppHandle,
    engine: State<'_, Engine>,
    runtime: State<'_, Runtime>,
    key: GameKey,
) -> ApiResult<InGameOverlayState> {
    let store_key = to_store_key(&key);
    let mut stored = engine.settings()?;
    stored
        .ingame_overlay
        .games
        .retain(|e| !e.key.matches(&store_key));
    save_and_announce(&app, &engine, &runtime, stored).await
}
