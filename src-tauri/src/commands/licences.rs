//! The third-party licences window (issue #138): About opens it, and the main window shares its
//! look (theme, effects, language) with it, since that window has no access to the settings.
//! The licence list itself is a static file of the UI (third-party-notices.json).

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::types::{ApiError, ApiResult, ErrorCode};
use crate::aux_window::{FOLLOW_LOOK, LICENCES};

/// The look of the main window's root element, for a window of its own to copy.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct WindowLook {
    /// `data-*` attributes of the main window's root element (theme, visual effects, hints).
    pub attributes: BTreeMap<String, String>,
    /// CSS custom properties set on the main window's root element (theme colours).
    pub properties: BTreeMap<String, String>,
    pub language: String,
}

/// The main window's look changed; sent to the licences and "What's new" windows only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct WindowLookChanged {
    pub look: WindowLook,
}

/// The look the main window shared last, for a window that opens later.
#[derive(Default)]
pub struct SharedLook(Mutex<Option<WindowLook>>);

/// Opens the third-party licences window, or brings it to the front if it is already open.
#[tauri::command]
#[specta::specta]
pub async fn licences_open(app: AppHandle) -> ApiResult<()> {
    // Async: creating a window from a synchronous command deadlocks on Windows.
    crate::aux_window::open(&app, &LICENCES).map_err(|e| {
        tracing::warn!(error = %e, "opening the licences window failed");
        ApiError::new(
            ErrorCode::Io,
            format!("opening the licences window failed: {e}"),
        )
    })
}

/// Closes the licences window (Ctrl+W in that window).
#[tauri::command]
#[specta::specta]
pub fn licences_close(app: AppHandle) -> ApiResult<()> {
    crate::aux_window::close(&app, &LICENCES);
    Ok(())
}

/// Called by the main window whenever its theme or language change.
#[tauri::command]
#[specta::specta]
pub fn window_look_share(
    app: AppHandle,
    shared: State<'_, SharedLook>,
    look: WindowLook,
) -> ApiResult<()> {
    *shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(look.clone());
    for spec in FOLLOW_LOOK {
        if let Err(e) = (WindowLookChanged { look: look.clone() }).emit_to(&app, spec.label) {
            tracing::warn!(error = %e, window = spec.label, "updating a window's look failed");
        }
    }
    Ok(())
}

/// The look the main window shared last; `null` before it shared one.
#[tauri::command]
#[specta::specta]
pub fn window_look(shared: State<'_, SharedLook>) -> ApiResult<Option<WindowLook>> {
    Ok(shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone())
}
