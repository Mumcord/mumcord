//! Identity: the user's client certificate (FR-1.2, FR-1.3).

use std::path::PathBuf;

use mumcord_engine::{Engine, IdentityView};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::types::ApiResult;

/// Public view of the identity; the certificate itself never leaves the engine.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    /// Default username.
    pub name: String,
    /// SHA-1 hash of the certificate, as Mumble servers identify it.
    pub cert_hash: String,
}

impl From<IdentityView> for Identity {
    fn from(v: IdentityView) -> Self {
        Self {
            name: v.name,
            cert_hash: v.cert_hash,
        }
    }
}

/// Current identity, or `null` before the wizard created one (first start).
#[tauri::command]
#[specta::specta]
pub async fn identity_get(engine: State<'_, Engine>) -> ApiResult<Option<Identity>> {
    Ok(engine.identity().await?.map(Identity::from))
}

/// Creates a new self-signed identity and stores it in the OS secure storage.
#[tauri::command]
#[specta::specta]
pub async fn identity_create(engine: State<'_, Engine>, name: String) -> ApiResult<Identity> {
    Ok(engine.identity_create(name).await?.into())
}

/// Imports a PKCS#12 file (e.g. exported from the official client), replacing the current identity.
#[tauri::command]
#[specta::specta]
pub async fn identity_import(
    engine: State<'_, Engine>,
    path: String,
    password: Option<String>,
) -> ApiResult<Identity> {
    Ok(engine
        .identity_import(PathBuf::from(path), password)
        .await?
        .into())
}

/// Exports the identity as a password-protected PKCS#12 file.
#[tauri::command]
#[specta::specta]
pub async fn identity_export(
    engine: State<'_, Engine>,
    path: String,
    password: String,
) -> ApiResult<()> {
    Ok(engine
        .identity_export(PathBuf::from(path), password)
        .await?)
}
