//! Diagnostic file and About information (FR-1.39, FR-1.40).

use std::path::Path;

use mumcord_engine::diag;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::{ApiError, ApiResult, ErrorCode};

/// Contents of `THIRD_PARTY_NOTICES`, embedded at build time.
const THIRD_PARTY_NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    /// Contents of `THIRD_PARTY_NOTICES` for Settings → About.
    pub third_party_notices: String,
}

/// Writes the diagnostic file (no chat contents, passwords, certificates or personal data).
#[tauri::command]
#[specta::specta]
pub fn diagnostics_save(path: String) -> ApiResult<()> {
    let info = diag::collect_info(env!("CARGO_PKG_VERSION"));
    diag::save_diagnostic_file(Path::new(&path), &info).map_err(|e| {
        ApiError::new(
            ErrorCode::Io,
            format!("writing diagnostic file failed: {e}"),
        )
    })
}

#[tauri::command]
#[specta::specta]
pub fn app_info() -> ApiResult<AppInfo> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        os: diag::os_description(),
        third_party_notices: THIRD_PARTY_NOTICES.to_owned(),
    })
}
