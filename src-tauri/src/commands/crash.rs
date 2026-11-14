//! Crash reports and the current log in Settings → Advanced & Developer (#568). Nothing here
//! uploads anything; copying and exporting are the user's own steps.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use mumcord_engine::{crash, diag};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::{ApiError, ApiResult, ErrorCode};

/// Lines "Copy last lines" puts on the clipboard.
const LOG_TAIL_LINES: usize = 200;

struct Dirs {
    crash: PathBuf,
    logs: PathBuf,
}

static DIRS: OnceLock<Dirs> = OnceLock::new();

/// The crash folder sits next to `logs`, so the paths are known before the engine opens.
pub fn crash_dir(logs: &Path) -> PathBuf {
    logs.with_file_name("crash-reports")
}

/// Starts crash reporting for this run (called before the engine opens).
pub fn install(logs: &Path, version: &str) {
    let dir = crash_dir(logs);
    if let Err(e) = crash::install(&dir, version) {
        eprintln!("crash reports unavailable: {e}");
    }
    let _ = DIRS.set(Dirs {
        crash: dir,
        logs: logs.to_path_buf(),
    });
}

fn dirs() -> ApiResult<&'static Dirs> {
    DIRS.get()
        .ok_or_else(|| ApiError::new(ErrorCode::Io, "crash reports are not set up"))
}

fn io_error(what: &str, e: &std::io::Error) -> ApiError {
    ApiError::new(ErrorCode::Io, format!("{what}: {e}"))
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CrashKind {
    Panic,
    Signal,
    Unexpected,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub id: String,
    /// Seconds since 1970.
    pub created: f64,
    pub version: String,
    pub kind: CrashKind,
    /// The signal name for [`CrashKind::Signal`], e.g. `SIGSEGV`.
    pub signal: Option<String>,
    pub cause: String,
    /// Sent with the Upload button.
    pub uploaded: bool,
}

impl From<crash::CrashSummary> for CrashReport {
    fn from(s: crash::CrashSummary) -> Self {
        let (kind, signal) = match s.kind {
            crash::CrashKind::Panic => (CrashKind::Panic, None),
            crash::CrashKind::Signal(name) => (CrashKind::Signal, Some(name)),
            crash::CrashKind::Unexpected => (CrashKind::Unexpected, None),
            // Shown like the signal it is; the cause line says that the app kept running.
            crash::CrashKind::Overrun => (CrashKind::Signal, Some("SIGXCPU".to_owned())),
        };
        Self {
            id: s.id,
            created: s.created_unix as f64,
            version: s.version,
            kind,
            signal,
            cause: s.cause,
            uploaded: s.uploaded,
        }
    }
}

/// The saved crash reports, newest first.
#[tauri::command]
#[specta::specta]
pub fn crash_list() -> ApiResult<Vec<CrashReport>> {
    Ok(crash::list(&dirs()?.crash)
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Reports from crashes the user has not been told about yet (the notice at start-up).
#[tauri::command]
#[specta::specta]
pub fn crash_take_unseen() -> ApiResult<Vec<CrashReport>> {
    Ok(crash::take_unseen(&dirs()?.crash)
        .into_iter()
        .map(Into::into)
        .collect())
}

/// The full text of one report, for the details view and "Copy".
#[tauri::command]
#[specta::specta]
pub fn crash_read(id: String) -> ApiResult<String> {
    crash::read(&dirs()?.crash, &id).map_err(|e| io_error("reading the crash report failed", &e))
}

#[tauri::command]
#[specta::specta]
pub fn crash_delete(id: String) -> ApiResult<()> {
    crash::delete(&dirs()?.crash, &id).map_err(|e| io_error("deleting the crash report failed", &e))
}

#[tauri::command]
#[specta::specta]
pub fn crash_delete_all() -> ApiResult<()> {
    crash::delete_all(&dirs()?.crash).map_err(|e| io_error("deleting crash reports failed", &e))
}

/// Saves the given reports (all when `ids` is empty) as a zip file at `path`.
#[tauri::command]
#[specta::specta]
pub fn crash_export(ids: Vec<String>, path: String) -> ApiResult<()> {
    crash::export(&dirs()?.crash, &ids, Path::new(&path))
        .map_err(|e| io_error("exporting crash reports failed", &e))
}

/// Sends one report to crash.mumcord.app after the user confirmed it in the upload dialog
/// (#573); `comment` is added as a last section when not empty. Never called by anything else.
#[tauri::command]
#[specta::specta]
pub async fn crash_upload(id: String, comment: String) -> ApiResult<()> {
    let dir = dirs()?.crash.clone();
    crash::upload(&dir, &id, &comment).await.map_err(|e| {
        ApiError::new(
            ErrorCode::ServerUnreachable,
            format!("uploading the crash report failed: {e}"),
        )
    })
}

/// The exact text the upload would send, for the preview in the confirmation.
#[tauri::command]
#[specta::specta]
pub fn crash_upload_preview(id: String, comment: String) -> ApiResult<String> {
    crash::upload_body(&dirs()?.crash, &id, &comment)
        .map_err(|e| io_error("reading the crash report failed", &e))
}

#[tauri::command]
#[specta::specta]
pub fn crash_open_folder() -> ApiResult<()> {
    super::files::open_file(&dirs()?.crash)
}

#[tauri::command]
#[specta::specta]
pub fn log_open_folder() -> ApiResult<()> {
    super::files::open_file(&dirs()?.logs)
}

/// The current log for "Copy": the last lines, or all of today's file. Lines are redacted the
/// same way as in the log file.
#[tauri::command]
#[specta::specta]
pub fn log_text(whole: bool) -> ApiResult<String> {
    if !whole {
        let lines = diag::recent_log_lines();
        let start = lines.len().saturating_sub(LOG_TAIL_LINES);
        return Ok(lines[start..].join("\n"));
    }
    let newest = std::fs::read_dir(&dirs()?.logs)
        .map_err(|e| io_error("reading the log folder failed", &e))?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".log"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    match newest {
        Some(entry) => std::fs::read_to_string(entry.path())
            .map_err(|e| io_error("reading the log failed", &e)),
        None => Ok(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crash_folder_sits_next_to_the_logs() {
        assert_eq!(
            crash_dir(Path::new("/data/app/logs")),
            Path::new("/data/app/crash-reports")
        );
    }
}
