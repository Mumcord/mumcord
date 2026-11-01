//! Window close behaviour and `mumble://` link handling (FR-1.6, FR-1.11, FR-1.38).

use mumcord_engine::{Engine, store};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::servers::DeepLinkOpened;
use super::types::ApiResult;
#[cfg(not(target_os = "macos"))]
use super::types::{ApiError, ErrorCode};
use crate::{deeplink, window};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CloseChoice {
    MinimizeToTray,
    Quit,
}

/// The window was closed while connected and the close behaviour is "ask".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct CloseRequested {
    /// Servers connected or being connected; the dialog says how many are left (FR-2.1).
    pub connected_servers: u32,
}

/// Answers `CloseRequested`; `remember` stores the choice as the close behaviour.
#[tauri::command]
#[specta::specta]
pub async fn close_choice(
    app: AppHandle,
    engine: State<'_, Engine>,
    choice: CloseChoice,
    remember: bool,
) -> ApiResult<()> {
    if remember {
        let mut settings = engine.settings()?;
        settings.close_behaviour = match choice {
            CloseChoice::MinimizeToTray => store::CloseBehaviour::MinimizeToTray,
            CloseChoice::Quit => store::CloseBehaviour::Quit,
        };
        engine.update_settings(settings).await?;
    }
    match choice {
        CloseChoice::MinimizeToTray => window::minimize_to_tray(&app),
        CloseChoice::Quit => window::quit(&app),
    }
    Ok(())
}

/// The main window's page holds its start screen; the window can be shown without an empty
/// frame. Called by the inline script in `ui/index.html`, before the app bundle has loaded.
#[tauri::command]
#[specta::specta]
pub fn app_start_painted(app: AppHandle) {
    window::on_start_painted(&app);
}

/// Disconnects cleanly and starts Mumcord again, e.g. to apply compatibility mode (FR-1.48).
#[tauri::command]
#[specta::specta]
pub fn app_restart(app: AppHandle) -> ApiResult<()> {
    window::restart(&app);
    Ok(())
}

/// Whether Mumcord is the registered handler for `mumble://` links.
#[tauri::command]
#[specta::specta]
pub fn link_handler_status(app: AppHandle) -> ApiResult<bool> {
    handler_status(&app)
}

/// Makes Mumcord the handler for `mumble://` links (FR-1.6).
#[tauri::command]
#[specta::specta]
pub fn link_handler_register(app: AppHandle) -> ApiResult<()> {
    handler_register(&app)
}

/// Returns a `mumble://` link that arrived before the UI listened (e.g. the one that launched
/// the app), once. Later links arrive only as `deepLinkOpened`.
#[tauri::command]
#[specta::specta]
pub fn deep_link_take_pending(app: AppHandle) -> ApiResult<Option<DeepLinkOpened>> {
    Ok(deeplink::take_pending(&app))
}

/// Returns the plugin id of a `mumcord://plugins/<id>` link that arrived before the UI listened
/// (e.g. the one that launched the app), once. Later links arrive as `plugins://open-details`.
#[tauri::command]
#[specta::specta]
pub fn plugins_take_pending_link(app: AppHandle) -> ApiResult<Option<String>> {
    Ok(deeplink::take_pending_plugin_link(&app))
}

#[cfg(not(target_os = "macos"))]
fn link_error(e: &dyn std::fmt::Display) -> ApiError {
    tracing::warn!(error = %e, "mumble:// handler registration failed");
    ApiError::new(ErrorCode::Internal, e.to_string())
}

// macOS registers the scheme through the app bundle's Info.plist; nothing to do at runtime.
#[cfg(target_os = "macos")]
fn handler_status(_app: &AppHandle) -> ApiResult<bool> {
    Ok(true)
}

#[cfg(target_os = "macos")]
fn handler_register(_app: &AppHandle) -> ApiResult<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn handler_status(app: &AppHandle) -> ApiResult<bool> {
    use tauri_plugin_deep_link::DeepLinkExt;
    for scheme in deeplink::SCHEMES {
        if !app
            .deep_link()
            .is_registered(scheme)
            .map_err(|e| link_error(&e))?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(target_os = "windows")]
fn handler_register(app: &AppHandle) -> ApiResult<()> {
    use tauri_plugin_deep_link::DeepLinkExt;
    for scheme in deeplink::SCHEMES {
        app.deep_link()
            .register(scheme)
            .map_err(|e| link_error(&e))?;
    }
    Ok(())
}

// Linux uses the app-ID desktop entry, so the handler and the taskbar share one entry.
#[cfg(target_os = "linux")]
fn handler_status(app: &AppHandle) -> ApiResult<bool> {
    crate::desktop_entry::is_handler(app).map_err(|e| link_error(&e))
}

#[cfg(target_os = "linux")]
fn handler_register(app: &AppHandle) -> ApiResult<()> {
    crate::desktop_entry::register_handler(app).map_err(|e| link_error(&e))
}
