//! Diagnostic file and About information (FR-1.39, FR-1.40).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::{ApiResult, not_implemented};

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
    let _ = path;
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn app_info() -> ApiResult<AppInfo> {
    not_implemented()
}
