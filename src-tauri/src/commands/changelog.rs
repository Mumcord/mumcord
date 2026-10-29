//! The "What's new" window (issue #145): About opens it, and the main window opens it once after
//! an update. The changelog itself is a static file of the UI (changelog.json), generated from
//! CHANGELOG.md at build time; the look comes from the main window as for the licences window.

use mumcord_engine::Engine;
use tauri::{AppHandle, State};

use super::types::{ApiError, ApiResult, ErrorCode};
use crate::aux_window::CHANGELOG;

/// Opens the "What's new" window, or brings it to the front if it is already open.
#[tauri::command]
#[specta::specta]
pub async fn changelog_open(app: AppHandle) -> ApiResult<()> {
    // Async: creating a window from a synchronous command deadlocks on Windows.
    crate::aux_window::open(&app, &CHANGELOG).map_err(|e| {
        tracing::warn!(error = %e, "opening the changelog window failed");
        ApiError::new(
            ErrorCode::Io,
            format!("opening the changelog window failed: {e}"),
        )
    })
}

/// Closes the "What's new" window (Ctrl+W or its Close button).
#[tauri::command]
#[specta::specta]
pub fn changelog_close(app: AppHandle) -> ApiResult<()> {
    crate::aux_window::close(&app, &CHANGELOG);
    Ok(())
}

/// Remembers that this version's "What's new" was shown or skipped, so it opens only once.
#[tauri::command]
#[specta::specta]
pub async fn whats_new_mark_seen(engine: State<'_, Engine>) -> ApiResult<()> {
    let mut settings = engine.settings()?;
    settings.whats_new_seen = Some(crate::version::DISPLAY.to_owned());
    engine.update_settings(settings).await?;
    Ok(())
}
