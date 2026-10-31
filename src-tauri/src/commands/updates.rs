//! In-app updater (FR-3.144–FR-3.148) and AppImage menu entry (FR-2.55).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

#[cfg(not(target_os = "linux"))]
use super::types::not_implemented;
use super::types::{ApiError, ApiResult, ErrorCode};
use crate::updater::Updater;

/// How this copy was installed, which decides how it is updated (FR-3.147).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum InstallKind {
    /// Replaces itself.
    AppImage,
    /// The AppImage or its folder cannot be written: download by hand.
    AppImageReadOnly,
    /// The NSIS installer: runs the new installer silently for this user.
    WindowsInstaller,
    /// The portable zip (#181): notify only, a new zip is unpacked by hand.
    WindowsPortable,
    /// Swaps the signed `.app` in place.
    MacApp,
    /// A Mac app outside an `.app` bundle (a development run).
    OtherMac,
    /// Packages: the package manager updates them.
    Deb,
    Rpm,
    Aur,
    OtherLinux,
}

/// Where the updater is (FR-3.144, FR-3.145).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum UpdatePhase {
    /// No check ran yet (or updates are off).
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    /// Checking the signature and `SHA256SUMS`.
    Verifying,
    Installing,
    /// Installed (Windows: verified, installs on restart); waits for "Restart to update".
    Ready,
    /// The download or a check of it failed; see `error`. The offer stays.
    Failed,
}

/// Why the last step failed. Offline checks are not shown as errors, only in Settings.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum UpdateError {
    /// GitHub could not be reached or answered with an error.
    Network,
    /// The update manifest is missing, broken or points outside the release.
    Manifest,
    /// The signature does not match the updater key: the download was discarded.
    Signature,
    /// The download does not match `SHA256SUMS`: it was discarded.
    Checksum,
    /// Replacing the app failed; the installed version is unchanged.
    Install,
}

/// The changes of one release between the installed and the offered version.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNote {
    pub version: String,
    /// `YYYY-MM-DD` of publication.
    pub date: Option<String>,
    /// The release's CHANGELOG section (`### Added` …) as Markdown, English only; untrusted.
    pub changes: String,
}

/// A newer release than the installed one.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOffer {
    pub version: String,
    /// Newest first: every release since the installed one.
    pub notes: Vec<ReleaseNote>,
    /// The release page, built from the version (never taken from the response).
    pub url: String,
}

/// Bytes received of the bundle, and its size when the server announced it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub received: f64,
    pub total: Option<f64>,
}

/// Everything the update UI shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub install: InstallKind,
    /// The updater can download and install by itself (otherwise: notify only).
    pub can_install: bool,
    /// A development build: it never checks for updates.
    pub dev_build: bool,
    pub phase: UpdatePhase,
    pub offer: Option<UpdateOffer>,
    pub progress: Option<UpdateProgress>,
    pub error: Option<UpdateError>,
    /// When the last check ran, in milliseconds since the Unix epoch.
    pub last_check_ms: Option<f64>,
}

/// The updater's state changed. `prompt` asks the UI to open the "Update available" or "ready"
/// dialog (a scheduled or manual check found something new, or a download finished).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusChanged {
    pub status: UpdateStatus,
    pub prompt: bool,
}

/// The updater's current state.
#[tauri::command]
#[specta::specta]
pub fn update_status(updater: State<'_, Updater>) -> UpdateStatus {
    updater.status()
}

/// "Check now": asks GitHub right away, unless updates are off or this is a development build
/// (then nothing is contacted). Failures show in the returned status.
#[tauri::command]
#[specta::specta]
pub async fn update_check_now(updater: State<'_, Updater>) -> ApiResult<UpdateStatus> {
    Ok(updater.check(true).await?)
}

/// "Download and install": downloads, verifies and installs the offered version in the
/// background. Progress arrives as `UpdateStatusChanged`.
#[tauri::command]
#[specta::specta]
pub fn update_download(updater: State<'_, Updater>) -> ApiResult<()> {
    if updater.download() {
        Ok(())
    } else {
        Err(ApiError::new(ErrorCode::NotFound, "no update to download"))
    }
}

/// "Cancel": stops the download; nothing is kept.
#[tauri::command]
#[specta::specta]
pub fn update_cancel(updater: State<'_, Updater>) {
    updater.cancel();
}

/// "Restart to update": the user agreed to restart. Disconnects cleanly (the next start
/// reconnects and rejoins), finishes the install where it waits for the restart, and restarts.
#[tauri::command]
#[specta::specta]
pub fn update_restart(app: tauri::AppHandle, updater: State<'_, Updater>) -> ApiResult<()> {
    if updater.restart(&app) {
        Ok(())
    } else {
        Err(ApiError::new(ErrorCode::NotFound, "no update is ready"))
    }
}

/// AppImage only: adds Mumcord to the app menu (desktop entry and icon). The UI calls this
/// after the user said yes and then stores the choice in `appimageMenuEntry`.
#[cfg(target_os = "linux")]
#[tauri::command]
#[specta::specta]
pub fn appimage_menu_entry_add(app: tauri::AppHandle) -> ApiResult<()> {
    crate::desktop_entry::add_for_appimage(&app)
        .map_err(|e| ApiError::new(ErrorCode::Io, format!("adding the menu entry failed: {e}")))
}

/// Only the Linux AppImage has a menu entry to add.
#[cfg(not(target_os = "linux"))]
#[tauri::command]
#[specta::specta]
pub fn appimage_menu_entry_add() -> ApiResult<()> {
    Err(not_implemented("appimage_menu_entry_add"))
}
