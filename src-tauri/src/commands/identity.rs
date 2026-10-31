//! Identities: the user's client certificates (FR-1.2, FR-1.3, issue #148).

use std::path::PathBuf;

use mumcord_engine::{Engine, IdentityView};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::types::{
    ApiResult, ServerId, to_api_identity_id, to_api_server_id, to_engine_identity_id,
};

/// Public view of an identity; the certificate and its key never leave the engine.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: u32,
    /// Local label the user chose; the certificate is not changed by it.
    pub label: String,
    /// Default username, from the certificate's common name.
    pub name: String,
    /// SHA-1 hash of the certificate, as Mumble servers identify it.
    pub cert_hash: String,
    /// When the identity was added to Mumcord, RFC 3339 (like message times).
    pub added_at: String,
    pub is_default: bool,
    /// Servers that use this identity directly (not through the default).
    pub server_ids: Vec<ServerId>,
}

impl From<IdentityView> for Identity {
    fn from(v: IdentityView) -> Self {
        Self {
            id: to_api_identity_id(v.id),
            label: v.label,
            name: v.name,
            cert_hash: v.cert_hash,
            added_at: super::chat::rfc3339(v.added_at),
            is_default: v.is_default,
            server_ids: v.server_ids.into_iter().map(to_api_server_id).collect(),
        }
    }
}

/// An identity whose key material could not be read, so a server fell back to the default.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ServerIdentityMissing {
    pub server_id: ServerId,
}

/// The default identity, or `null` before the wizard created one (first start).
#[tauri::command]
#[specta::specta]
pub async fn identity_get(engine: State<'_, Engine>) -> ApiResult<Option<Identity>> {
    Ok(engine.identity().await?.map(Identity::from))
}

/// Every identity: the default first, then by the date it was added.
#[tauri::command]
#[specta::specta]
pub async fn identity_list(engine: State<'_, Engine>) -> ApiResult<Vec<Identity>> {
    Ok(engine
        .identity_list()
        .await?
        .into_iter()
        .map(Identity::from)
        .collect())
}

/// Creates a new self-signed identity and stores it in the OS secure storage.
#[tauri::command]
#[specta::specta]
pub async fn identity_create(
    engine: State<'_, Engine>,
    name: String,
    label: Option<String>,
) -> ApiResult<Identity> {
    Ok(engine.identity_create(name, label).await?.into())
}

/// Imports a PKCS#12 file (e.g. exported from the official client) as another identity.
#[tauri::command]
#[specta::specta]
pub async fn identity_import(
    engine: State<'_, Engine>,
    path: String,
    password: Option<String>,
    label: Option<String>,
) -> ApiResult<Identity> {
    Ok(engine
        .identity_import(PathBuf::from(path), password, label)
        .await?
        .into())
}

/// Changes the local label of an identity; the certificate stays the same.
#[tauri::command]
#[specta::specta]
pub async fn identity_rename(
    engine: State<'_, Engine>,
    id: u32,
    label: String,
) -> ApiResult<Identity> {
    Ok(engine
        .identity_rename(to_engine_identity_id(id), label)
        .await?
        .into())
}

/// Exports the identity as a password-protected PKCS#12 file.
#[tauri::command]
#[specta::specta]
pub async fn identity_export(
    engine: State<'_, Engine>,
    id: u32,
    path: String,
    password: String,
) -> ApiResult<()> {
    Ok(engine
        .identity_export(to_engine_identity_id(id), PathBuf::from(path), password)
        .await?)
}

/// Removes an identity with its key material; the last identity cannot be removed.
#[tauri::command]
#[specta::specta]
pub async fn identity_delete(engine: State<'_, Engine>, id: u32) -> ApiResult<()> {
    Ok(engine.identity_delete(to_engine_identity_id(id)).await?)
}

/// Makes an identity the default one; returns the new list.
#[tauri::command]
#[specta::specta]
pub async fn identity_set_default(engine: State<'_, Engine>, id: u32) -> ApiResult<Vec<Identity>> {
    Ok(engine
        .identity_set_default(to_engine_identity_id(id))
        .await?
        .into_iter()
        .map(Identity::from)
        .collect())
}

/// Chooses the identity a saved server connects with; `null` uses the default.
#[tauri::command]
#[specta::specta]
pub async fn server_set_identity(
    engine: State<'_, Engine>,
    server_id: ServerId,
    identity_id: Option<u32>,
) -> ApiResult<super::servers::SavedServer> {
    Ok(engine
        .server_set_identity(server_id.into(), identity_id.map(to_engine_identity_id))
        .await?
        .into())
}
