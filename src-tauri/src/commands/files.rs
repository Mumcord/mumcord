//! File sharing (spec 07): the composer's attachment tray, file cards in the chat, downloads
//! and Open / Open folder. The engine owns every state machine; this module only wraps its API
//! as typed commands and events, supplies the system Downloads folder and opens saved files.

use std::path::{Path, PathBuf};

use mumcord_engine::Engine;
use mumcord_engine::files::card::{
    CardState, DownloadBlock as EngineBlock, FailReason as EngineFail, FileCardView,
    FilesUnavailable as EngineUnavailable, OwnShareView, Unavailable as EngineGone,
};
use mumcord_engine::files::sender::ChipView;
use mumcord_engine::files::sender::{ChipError as EngineChipError, ChipState as EngineChipState};
use mumcord_engine::files::share::EndReason as EngineEnd;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use super::chat::ChatMessage;
use super::p2p::RelayUnavailable;
use super::settings::{FilesMaxSize, P2pMode};
use super::types::{ApiError, ApiResult, ChannelId, ErrorCode, ServerId, to_engine_server_id};

/// Why file sharing is not available on a server (FR-7.9), in the order the spec shows them.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilesUnavailable {
    NoRelay,
    RelayNoFiles,
    NoCertificate,
    /// File sharing needs the relay; the peer-to-peer setting is `mode` (`off` or `directOnly`).
    P2pMode {
        mode: P2pMode,
    },
}

/// Why a card's Download button is disabled (FR-7.21).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DownloadBlock {
    /// Own offer: no Download at all.
    Own,
    Unavailable {
        reason: FilesUnavailable,
    },
    ReceiveOff,
    /// Larger than the user's limit.
    TooLarge {
        limit: FilesMaxSize,
    },
    AnotherDownload,
}

/// Why a card is unavailable (FR-7.20).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum UnavailableReason {
    SenderOffline,
    /// The sender answered that the share is gone.
    Ended,
    Expired,
}

/// Why a download failed (FR-7.20, FR-7.34).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FailReason {
    NoAnswer,
    Busy,
    /// The relay connection failed; `reason` is `null` when the relay gave none.
    Connect {
        reason: Option<RelayUnavailable>,
    },
    Stalled,
    ConnectionLost,
    /// Not enough disk space in `folder`; nothing was requested.
    NoSpace {
        folder: String,
    },
    /// Writing the file failed; `message` is the system's text.
    Io {
        message: String,
    },
}

/// A card's state as the recipient sees it (section 7.2).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FileCardState {
    Available,
    Unavailable {
        reason: UnavailableReason,
    },
    Requested,
    /// 1 = next.
    Queued {
        position: u32,
    },
    Downloading {
        received: u32,
        bytes_per_sec: u32,
    },
    Checking,
    /// `path` is where the file was saved.
    Done {
        path: String,
    },
    /// The file did not match the offer and was deleted.
    Mismatch,
    Failed {
        reason: FailReason,
    },
    Cancelled,
}

/// Why an own share ended (the sender's card says "Sharing ended").
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ShareEndReason {
    Expired,
    Stopped,
    LeftServer,
    Quit,
    FileChanged,
}

/// The running transfer of an own share.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SendingTo {
    pub recipient: String,
    pub percent: u32,
}

/// The sender's view of an own share (FR-7.16).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnShare {
    /// True once sharing ended; `endReason` is `null` when it ended before this run of Mumcord.
    pub ended: bool,
    pub end_reason: Option<ShareEndReason>,
    pub downloaded_by: Vec<String>,
    pub sending_to: Option<SendingTo>,
    pub waiting: u32,
}

/// A file card in the chat. Sizes and times are plain numbers (bytes, Unix seconds).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileCard {
    pub share_id: String,
    /// Sanitized name.
    pub name: String,
    pub size: u32,
    /// Display only; untrusted.
    pub mime: String,
    pub sha256: String,
    pub expires: u32,
    pub sender_name: String,
    pub own: bool,
    /// A program or script: the risky warning replaces the plain confirmation (FR-7.24).
    pub risky: bool,
    pub state: FileCardState,
    pub download_block: Option<DownloadBlock>,
    pub share: Option<OwnShare>,
}

/// A card changed: a received offer's state or progress, or an own share's details.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FileCardChanged {
    pub server_id: ServerId,
    pub card: FileCard,
}

/// Why a chip could not be shared.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChipError {
    /// `name` is larger than 2 GB.
    TooLarge {
        name: String,
    },
    Unreadable {
        path: String,
    },
    /// The server refused the offer message; `reason` is its text.
    ServerRefused {
        reason: String,
    },
    Other {
        message: String,
    },
}

/// A chip's state in the attachment tray (mockup 1b).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChipState {
    Zipping {
        done: u32,
        total: u32,
    },
    Ready,
    Checking {
        done: u32,
        total: u32,
    },
    Failed {
        error: ChipError,
    },
    /// The offer was posted; the chip leaves the tray.
    Sent,
}

/// A file or folder in the composer's attachment tray.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileChip {
    pub id: u32,
    pub name: String,
    pub size: Option<u32>,
    pub state: ChipState,
}

/// A chip changed. Not tied to a server: chips exist before they are sent.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FileChipChanged {
    pub chip: FileChip,
}

/// Byte counts above 4 GB cannot occur for files Mumcord shares (2 GB cap) and are clamped for
/// foreign offers, so a hostile size cannot break the UI's arithmetic.
fn clamp(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

impl From<EngineUnavailable> for FilesUnavailable {
    fn from(r: EngineUnavailable) -> Self {
        match r {
            EngineUnavailable::NoRelay => Self::NoRelay,
            EngineUnavailable::RelayNoFiles => Self::RelayNoFiles,
            EngineUnavailable::NoCertificate => Self::NoCertificate,
            EngineUnavailable::P2pMode(mode) => Self::P2pMode { mode: mode.into() },
        }
    }
}

impl From<EngineBlock> for DownloadBlock {
    fn from(b: EngineBlock) -> Self {
        match b {
            EngineBlock::Own => Self::Own,
            EngineBlock::Unavailable(reason) => Self::Unavailable {
                reason: reason.into(),
            },
            EngineBlock::ReceiveOff => Self::ReceiveOff,
            EngineBlock::TooLarge(limit) => Self::TooLarge {
                limit: limit.into(),
            },
            EngineBlock::AnotherDownload => Self::AnotherDownload,
        }
    }
}

impl From<EngineGone> for UnavailableReason {
    fn from(r: EngineGone) -> Self {
        match r {
            EngineGone::SenderOffline => Self::SenderOffline,
            EngineGone::Ended => Self::Ended,
            EngineGone::Expired => Self::Expired,
        }
    }
}

impl From<EngineFail> for FailReason {
    fn from(r: EngineFail) -> Self {
        match r {
            EngineFail::NoAnswer => Self::NoAnswer,
            EngineFail::Busy => Self::Busy,
            EngineFail::Connect(reason) => Self::Connect {
                reason: reason.map(Into::into),
            },
            EngineFail::Stalled => Self::Stalled,
            EngineFail::ConnectionLost => Self::ConnectionLost,
            EngineFail::NoSpace(folder) => Self::NoSpace {
                folder: path_text(&folder),
            },
            EngineFail::Io(message) => Self::Io { message },
        }
    }
}

impl From<CardState> for FileCardState {
    fn from(s: CardState) -> Self {
        match s {
            CardState::Available => Self::Available,
            CardState::Unavailable(reason) => Self::Unavailable {
                reason: reason.into(),
            },
            CardState::Requested => Self::Requested,
            CardState::Queued { position } => Self::Queued {
                position: position.into(),
            },
            CardState::Downloading {
                received,
                bytes_per_sec,
            } => Self::Downloading {
                received: clamp(received),
                bytes_per_sec: clamp(bytes_per_sec),
            },
            CardState::Checking => Self::Checking,
            CardState::Done { path } => Self::Done {
                path: path_text(&path),
            },
            CardState::Mismatch => Self::Mismatch,
            CardState::Failed(reason) => Self::Failed {
                reason: reason.into(),
            },
            CardState::Cancelled => Self::Cancelled,
        }
    }
}

impl From<EngineEnd> for ShareEndReason {
    fn from(r: EngineEnd) -> Self {
        match r {
            EngineEnd::Expired => Self::Expired,
            EngineEnd::Stopped => Self::Stopped,
            EngineEnd::LeftServer => Self::LeftServer,
            EngineEnd::Quit => Self::Quit,
            EngineEnd::FileChanged => Self::FileChanged,
        }
    }
}

impl From<OwnShareView> for OwnShare {
    fn from(v: OwnShareView) -> Self {
        Self {
            ended: v.ended.is_some(),
            end_reason: v.ended.flatten().map(Into::into),
            downloaded_by: v.downloaded_by,
            sending_to: v.sending_to.map(|(recipient, percent)| SendingTo {
                recipient,
                percent: percent.into(),
            }),
            waiting: u32::try_from(v.waiting).unwrap_or(u32::MAX),
        }
    }
}

impl From<FileCardView> for FileCard {
    fn from(v: FileCardView) -> Self {
        Self {
            share_id: v.share_id,
            name: v.name,
            size: clamp(v.size),
            mime: v.mime,
            sha256: v.sha256,
            expires: clamp(v.expires),
            sender_name: v.sender_name,
            own: v.own,
            risky: v.risky,
            state: v.state.into(),
            download_block: v.download_block.map(Into::into),
            share: v.share.map(Into::into),
        }
    }
}

impl From<EngineChipError> for ChipError {
    fn from(e: EngineChipError) -> Self {
        match e {
            EngineChipError::TooLarge(name) => Self::TooLarge { name },
            EngineChipError::Unreadable(path) => Self::Unreadable {
                path: path_text(&path),
            },
            EngineChipError::ServerRefused(reason) => Self::ServerRefused { reason },
            EngineChipError::Other(message) => Self::Other { message },
        }
    }
}

impl From<EngineChipState> for ChipState {
    fn from(s: EngineChipState) -> Self {
        match s {
            EngineChipState::Zipping { done, total } => Self::Zipping {
                done: clamp(done),
                total: clamp(total),
            },
            EngineChipState::Ready => Self::Ready,
            EngineChipState::Checking { done, total } => Self::Checking {
                done: clamp(done),
                total: clamp(total),
            },
            EngineChipState::Failed(error) => Self::Failed {
                error: error.into(),
            },
            EngineChipState::Sent => Self::Sent,
        }
    }
}

impl From<ChipView> for FileChip {
    fn from(v: ChipView) -> Self {
        Self {
            id: u32::try_from(v.id).unwrap_or(u32::MAX),
            name: v.name,
            size: v.size.map(clamp),
            state: v.state.into(),
        }
    }
}

fn downloads_dir(app: &AppHandle) -> ApiResult<PathBuf> {
    app.path()
        .download_dir()
        .map_err(|e| ApiError::new(ErrorCode::Io, format!("no Downloads folder: {e}")))
}

/// Opens `path` with the default program. Windows and macOS use the opener plugin; Linux goes
/// through the launchers of `browser`, which repair the environment inside an AppImage. Nothing
/// touches the file's mark-of-the-web, so the system's own warning still applies.
#[cfg(not(target_os = "linux"))]
fn open_file(path: &Path) -> ApiResult<()> {
    tauri_plugin_opener::open_path(path, None::<&str>)
        .map_err(|e| ApiError::new(ErrorCode::Io, e.to_string()))
}

#[cfg(target_os = "linux")]
fn open_file(path: &Path) -> ApiResult<()> {
    crate::browser::open_url(&path_text(path))
        .map_err(|e| ApiError::new(ErrorCode::Io, e.to_string()))
}

/// Shows `path` in the file manager; Linux has no portable "select", so it opens the folder.
#[cfg(not(target_os = "linux"))]
fn reveal_file(path: &Path) -> ApiResult<()> {
    tauri_plugin_opener::reveal_item_in_dir(path)
        .map_err(|e| ApiError::new(ErrorCode::Io, e.to_string()))
}

#[cfg(target_os = "linux")]
fn reveal_file(path: &Path) -> ApiResult<()> {
    let folder = path.parent().unwrap_or(path);
    open_file(folder)
}

/// Why file sharing is unavailable on the server, or `null` when it works (FR-7.9). The
/// paperclip button's disabled state and tooltip.
#[tauri::command]
#[specta::specta]
pub fn files_availability(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> Option<FilesUnavailable> {
    engine
        .files_availability(to_engine_server_id(server_id))
        .err()
        .map(Into::into)
}

/// Adds a file or folder to the attachment tray (FR-7.11 – 7.13) and returns its chip; a folder
/// is zipped in the background and reported as `fileChipChanged`.
#[tauri::command]
#[specta::specta]
pub fn files_chip_add(engine: State<'_, Engine>, path: String) -> FileChip {
    engine.files_chip_add(PathBuf::from(path)).into()
}

/// Takes a chip out of the tray (and drops its zip).
#[tauri::command]
#[specta::specta]
pub fn files_chip_remove(engine: State<'_, Engine>, chip_id: u32) {
    engine.files_chip_remove(chip_id.into());
}

/// Posts one offer message per chip into the channel (FR-7.14) and returns the posted own
/// messages. A chip the server refused stays in the tray as failed. Errors: `invalidInput`
/// above 10 chips, `permissionDenied` when file sharing is unavailable.
#[tauri::command]
#[specta::specta]
pub async fn files_send(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
    chip_ids: Vec<u32>,
) -> ApiResult<Vec<ChatMessage>> {
    let posted = engine
        .files_send(
            to_engine_server_id(server_id),
            channel_id,
            chip_ids.into_iter().map(u64::from).collect(),
        )
        .await?;
    Ok(posted.into_iter().map(Into::into).collect())
}

/// Ends an own share ("Stop sharing", FR-7.16).
#[tauri::command]
#[specta::specta]
pub fn files_stop(engine: State<'_, Engine>, share_id: String) {
    engine.files_stop(&share_id);
}

/// The current card of a share, or `null` when unknown.
#[tauri::command]
#[specta::specta]
pub fn files_card(
    engine: State<'_, Engine>,
    server_id: ServerId,
    share_id: String,
) -> Option<FileCard> {
    engine
        .file_card(to_engine_server_id(server_id), &share_id)
        .map(Into::into)
}

/// Starts the download of a card after the user confirmed it (FR-7.22); also used by Retry.
/// Returns once the request is on its way; the card reports progress as `fileCardChanged`.
#[tauri::command]
#[specta::specta]
pub fn files_download(
    app: AppHandle,
    engine: State<'_, Engine>,
    server_id: ServerId,
    share_id: String,
) -> ApiResult<()> {
    let downloads = downloads_dir(&app)?;
    Ok(engine.files_download(to_engine_server_id(server_id), &share_id, &downloads)?)
}

/// Cancels a running download of the card.
#[tauri::command]
#[specta::specta]
pub fn files_cancel(engine: State<'_, Engine>, server_id: ServerId, share_id: String) {
    engine.files_cancel(to_engine_server_id(server_id), &share_id);
}

/// The folder downloads are saved to: the setting, else the system Downloads folder.
#[tauri::command]
#[specta::specta]
pub fn files_save_folder(app: AppHandle, engine: State<'_, Engine>) -> ApiResult<String> {
    let downloads = downloads_dir(&app)?;
    Ok(path_text(&engine.files_save_folder(&downloads)))
}

/// Opens a downloaded file with the system's default program (FR-7.29). The saved file keeps
/// its mark-of-the-web, so the system still warns about it. Errors: `notFound` with "The file is
/// no longer in {folder}" when it was moved or deleted.
#[tauri::command]
#[specta::specta]
pub fn files_open(
    engine: State<'_, Engine>,
    server_id: ServerId,
    share_id: String,
) -> ApiResult<()> {
    let path = engine.files_saved_path(to_engine_server_id(server_id), &share_id)?;
    open_file(&path)
}

/// Shows a downloaded file in the system's file manager (FR-7.29). Same errors as `files_open`.
#[tauri::command]
#[specta::specta]
pub fn files_open_folder(
    engine: State<'_, Engine>,
    server_id: ServerId,
    share_id: String,
) -> ApiResult<()> {
    let path = engine.files_saved_path(to_engine_server_id(server_id), &share_id)?;
    reveal_file(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_engine::peer::RelayUnavailable as EngineRelay;
    use mumcord_engine::store::{FilesMaxSize as StoreMax, P2pMode as StoreMode};

    fn json<T: Serialize>(v: &T) -> serde_json::Value {
        serde_json::to_value(v).expect("serializes")
    }

    #[test]
    fn unavailable_reasons_map() {
        let cases = [
            (EngineUnavailable::NoRelay, "noRelay"),
            (EngineUnavailable::RelayNoFiles, "relayNoFiles"),
            (EngineUnavailable::NoCertificate, "noCertificate"),
        ];
        for (engine, kind) in cases {
            assert_eq!(json(&FilesUnavailable::from(engine))["kind"], kind);
        }
        let mode = FilesUnavailable::from(EngineUnavailable::P2pMode(StoreMode::DirectOnly));
        assert_eq!(json(&mode)["mode"], "directOnly");
    }

    #[test]
    fn download_blocks_map() {
        assert_eq!(json(&DownloadBlock::from(EngineBlock::Own))["kind"], "own");
        assert_eq!(
            json(&DownloadBlock::from(EngineBlock::AnotherDownload))["kind"],
            "anotherDownload"
        );
        let too_large = DownloadBlock::from(EngineBlock::TooLarge(StoreMax::M500));
        assert_eq!(json(&too_large)["limit"], "500M");
        let nested = DownloadBlock::from(EngineBlock::Unavailable(EngineUnavailable::NoRelay));
        assert_eq!(json(&nested)["reason"]["kind"], "noRelay");
    }

    #[test]
    fn failure_reasons_map() {
        let relay = FailReason::from(EngineFail::Connect(Some(EngineRelay::RelayFull)));
        assert_eq!(json(&relay)["reason"]["kind"], "relayFull");
        let none = FailReason::from(EngineFail::Connect(None));
        assert!(json(&none)["reason"].is_null());
        let space = FailReason::from(EngineFail::NoSpace(PathBuf::from("/data")));
        assert_eq!(json(&space)["folder"], "/data");
        let io = FailReason::from(EngineFail::Io("disk full".into()));
        assert_eq!(json(&io)["message"], "disk full");
        for (engine, kind) in [
            (EngineFail::NoAnswer, "noAnswer"),
            (EngineFail::Busy, "busy"),
            (EngineFail::Stalled, "stalled"),
            (EngineFail::ConnectionLost, "connectionLost"),
        ] {
            assert_eq!(json(&FailReason::from(engine))["kind"], kind);
        }
    }

    #[test]
    fn card_states_map() {
        let queued = FileCardState::from(CardState::Queued { position: 2 });
        assert_eq!(json(&queued)["position"], 2);
        let downloading = FileCardState::from(CardState::Downloading {
            received: 10,
            bytes_per_sec: 5,
        });
        assert_eq!(json(&downloading)["bytesPerSec"], 5);
        let gone = FileCardState::from(CardState::Unavailable(EngineGone::Ended));
        assert_eq!(json(&gone)["reason"], "ended");
        let done = FileCardState::from(CardState::Done {
            path: PathBuf::from("/d/a.txt"),
        });
        assert_eq!(json(&done)["path"], "/d/a.txt");
        let failed = FileCardState::from(CardState::Failed(EngineFail::Busy));
        assert_eq!(json(&failed)["reason"]["kind"], "busy");
    }

    #[test]
    fn hostile_sizes_are_clamped() {
        let state = FileCardState::from(CardState::Downloading {
            received: u64::MAX,
            bytes_per_sec: 1,
        });
        assert_eq!(json(&state)["received"], u32::MAX);
    }

    #[test]
    fn own_share_end_distinguishes_unknown_from_active() {
        let view = |ended| OwnShareView {
            ended,
            downloaded_by: vec!["Ann".into()],
            sending_to: Some(("Bob".into(), 40)),
            waiting: 3,
        };
        let active = OwnShare::from(view(None));
        assert!(!active.ended && active.end_reason.is_none());
        let before_run = OwnShare::from(view(Some(None)));
        assert!(before_run.ended && before_run.end_reason.is_none());
        let stopped = OwnShare::from(view(Some(Some(EngineEnd::Stopped))));
        assert_eq!(stopped.end_reason, Some(ShareEndReason::Stopped));
        assert_eq!(stopped.waiting, 3);
        assert_eq!(stopped.sending_to.map(|s| s.percent), Some(40));
    }

    #[test]
    fn chip_states_map() {
        let zipping = ChipState::from(EngineChipState::Zipping { done: 1, total: 4 });
        assert_eq!(json(&zipping)["total"], 4);
        let refused = ChipState::from(EngineChipState::Failed(EngineChipError::ServerRefused(
            "no".into(),
        )));
        assert_eq!(json(&refused)["error"]["kind"], "serverRefused");
        let large = ChipState::from(EngineChipState::Failed(EngineChipError::TooLarge(
            "a.iso".into(),
        )));
        assert_eq!(json(&large)["error"]["name"], "a.iso");
        let chip = FileChip::from(ChipView {
            id: 7,
            name: "a".into(),
            size: None,
            state: EngineChipState::Ready,
        });
        assert_eq!((chip.id, chip.size), (7, None));
    }
}
