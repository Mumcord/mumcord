//! Identity: the user's client certificate (FR-1.2, FR-1.3).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::{ApiResult, not_implemented};

/// Public view of the identity; the certificate itself never leaves the engine.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    /// Default username.
    pub name: String,
    /// SHA-1 hash of the certificate, as Mumble servers identify it.
    pub cert_hash: String,
}

/// Current identity, or `null` before the wizard created one (first start).
#[tauri::command]
#[specta::specta]
pub fn identity_get() -> ApiResult<Option<Identity>> {
    not_implemented()
}

/// Creates a new self-signed identity and stores it in the OS secure storage.
#[tauri::command]
#[specta::specta]
pub fn identity_create(name: String) -> ApiResult<Identity> {
    let _ = name;
    not_implemented()
}

/// Imports a PKCS#12 file (e.g. exported from the official client), replacing the current identity.
#[tauri::command]
#[specta::specta]
pub fn identity_import(path: String, password: Option<String>) -> ApiResult<Identity> {
    let _ = (path, password);
    not_implemented()
}

/// Exports the identity as a password-protected PKCS#12 file.
#[tauri::command]
#[specta::specta]
pub fn identity_export(path: String, password: String) -> ApiResult<()> {
    let _ = (path, password);
    not_implemented()
}
