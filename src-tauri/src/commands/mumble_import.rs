//! Import from the official Mumble client (FR-3.93, FR-3.94, issue #66). Passwords and the
//! certificate stay in the engine; the UI only sees what was found and what was done.

use mumcord_engine::mumble_import::{KeyIssue, MumbleEnv, MumbleVersion};
use mumcord_engine::{Engine, MumbleImportChoice, MumbleImportPreview, MumbleImportResult, store};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::identity::Identity;
use super::servers::SavedServer;
use super::settings::TalkMode;
use super::types::ApiResult;

/// Which settings format of the official client was found.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum MumbleSettingsFormat {
    /// `mumble_settings.json`, Mumble 1.5 and later.
    Json,
    /// Mumble 1.4 and earlier (INI file, property list or registry).
    Legacy,
    /// Only the favourites database was found.
    DatabaseOnly,
}

/// Why Mumble's push-to-talk shortcut cannot be used in Mumcord.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum MumblePttIssue {
    ModifierOnly,
    TooManyKeys,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleFavourite {
    pub label: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub has_password: bool,
    /// Already in Mumcord; skipped by the import.
    pub duplicate: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleCertificate {
    /// Common name; empty while locked by a password.
    pub name: String,
    /// End of validity, RFC 3339.
    pub valid_until: Option<String>,
    pub needs_password: bool,
    pub already_imported: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleDevice {
    pub id: String,
    pub name: String,
}

/// What an installed official client offers for import.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleImportFound {
    pub format: MumbleSettingsFormat,
    pub favourites: Vec<MumbleFavourite>,
    pub favourites_failed: bool,
    pub certificate: Option<MumbleCertificate>,
    pub input_device: Option<MumbleDevice>,
    pub output_device: Option<MumbleDevice>,
    pub devices_missing: bool,
    /// In the `pttKey` settings format.
    pub ptt_key: Option<String>,
    pub ptt_issue: Option<MumblePttIssue>,
    pub talk_mode: Option<TalkMode>,
    pub output_volume_percent: Option<u16>,
}

impl From<MumbleImportPreview> for MumbleImportFound {
    fn from(p: MumbleImportPreview) -> Self {
        Self {
            format: match p.version {
                MumbleVersion::Json => MumbleSettingsFormat::Json,
                MumbleVersion::Legacy => MumbleSettingsFormat::Legacy,
                MumbleVersion::DatabaseOnly => MumbleSettingsFormat::DatabaseOnly,
            },
            favourites: p
                .favourites
                .into_iter()
                .map(|f| MumbleFavourite {
                    label: f.label,
                    host: f.host,
                    port: f.port,
                    username: f.username,
                    has_password: f.has_password,
                    duplicate: f.duplicate,
                })
                .collect(),
            favourites_failed: p.favourites_failed,
            certificate: p.identity.map(|i| MumbleCertificate {
                name: i.name,
                valid_until: i.valid_until.map(super::chat::rfc3339),
                needs_password: i.needs_password,
                already_imported: i.already_imported,
            }),
            input_device: p.input_device.map(|d| MumbleDevice {
                id: d.id,
                name: d.name,
            }),
            output_device: p.output_device.map(|d| MumbleDevice {
                id: d.id,
                name: d.name,
            }),
            devices_missing: p.devices_missing,
            ptt_key: p.ptt_key,
            ptt_issue: p.ptt_issue.map(|i| match i {
                KeyIssue::ModifierOnly => MumblePttIssue::ModifierOnly,
                KeyIssue::TooManyKeys => MumblePttIssue::TooManyKeys,
                KeyIssue::Unknown => MumblePttIssue::Unknown,
            }),
            talk_mode: p.talk_mode.map(|m| match m {
                store::TalkMode::VoiceActivation => TalkMode::VoiceActivation,
                store::TalkMode::PushToTalk => TalkMode::PushToTalk,
            }),
            output_volume_percent: p.output_volume_percent,
        }
    }
}

/// A favourite chosen in the preview, by host and port.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleServerKey {
    pub host: String,
    pub port: u16,
}

/// No `Debug`: it carries the certificate password.
#[derive(Serialize, Deserialize, Clone, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleImportRequest {
    pub favourites: Vec<MumbleServerKey>,
    pub passwords: bool,
    pub certificate: bool,
    /// Only for a certificate that needs one.
    pub certificate_password: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleImportDone {
    pub added: Vec<SavedServer>,
    /// Labels of chosen servers that were already in Mumcord.
    pub skipped: Vec<String>,
    pub passwords_saved: u32,
    pub identity: Option<Identity>,
    pub identity_skipped: bool,
    /// End of validity of the imported certificate, RFC 3339.
    pub identity_valid_until: Option<String>,
}

impl From<MumbleImportResult> for MumbleImportDone {
    fn from(r: MumbleImportResult) -> Self {
        Self {
            added: r.added.into_iter().map(SavedServer::from).collect(),
            skipped: r.skipped,
            passwords_saved: r.passwords_saved,
            identity: r.identity.map(Identity::from),
            identity_skipped: r.identity_skipped,
            identity_valid_until: r.identity_valid_until.map(super::chat::rfc3339),
        }
    }
}

/// Looks for an installed official Mumble client; `null` when there is none.
#[tauri::command]
#[specta::specta]
pub async fn mumble_import_scan(engine: State<'_, Engine>) -> ApiResult<Option<MumbleImportFound>> {
    Ok(engine
        .mumble_import_preview(MumbleEnv::from_process())
        .await?
        .map(MumbleImportFound::from))
}

/// Imports the chosen favourites, passwords and certificate into Mumcord. Settings are saved
/// by the UI with `settings_update`.
#[tauri::command]
#[specta::specta]
pub async fn mumble_import_apply(
    engine: State<'_, Engine>,
    request: MumbleImportRequest,
) -> ApiResult<MumbleImportDone> {
    let choice = MumbleImportChoice {
        favourites: request
            .favourites
            .into_iter()
            .map(|k| (k.host, k.port))
            .collect(),
        passwords: request.passwords,
        identity: request.certificate,
        certificate_password: request.certificate_password,
    };
    Ok(engine
        .mumble_import_apply(MumbleEnv::from_process(), choice)
        .await?
        .into())
}

/// Removes the servers and the identity the last import added.
#[tauri::command]
#[specta::specta]
pub async fn mumble_import_undo(engine: State<'_, Engine>) -> ApiResult<()> {
    Ok(engine.mumble_import_undo().await?)
}
