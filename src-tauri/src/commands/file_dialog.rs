//! Native file dialogs that Rust opens itself.
//!
//! Commands that write a file, attach one or change the download folder never take a path from
//! the web view: a script that got into the page could otherwise write anywhere (an autostart
//! entry, a shell profile) or read any file. They open the dialog here and act only on the path
//! the user picked in it. The page still supplies the texts (title, suggested name, filter),
//! which at worst change what the dialog suggests; the user always confirms the path.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::WebviewWindow;
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder, FilePath};

use super::types::{ApiError, ApiResult, ErrorCode};

/// Texts of a save dialog, translated by the UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveDialog {
    /// Window title of the dialog.
    pub title: String,
    /// Suggested file name, e.g. `mumcord-diagnostics.txt`.
    pub default_name: String,
    /// Name of the file type in the filter, e.g. "Text file".
    pub filter_name: String,
    /// Extensions of the filter without the dot; the first is added when the name has none.
    pub extensions: Vec<String>,
}

/// A file dialog in front of the window that asked for it.
fn builder(window: &WebviewWindow, title: &str) -> FileDialogBuilder<tauri::Wry> {
    window.dialog().file().set_parent(window).set_title(title)
}

/// Runs a blocking dialog off the async runtime's worker threads.
async fn run<T: Send + 'static>(open: impl FnOnce() -> T + Send + 'static) -> ApiResult<T> {
    tauri::async_runtime::spawn_blocking(open)
        .await
        .map_err(|e| ApiError::new(ErrorCode::Internal, format!("file dialog failed: {e}")))
}

fn local_path(picked: FilePath) -> ApiResult<PathBuf> {
    picked
        .into_path()
        .map_err(|e| ApiError::new(ErrorCode::InvalidInput, format!("not a local file: {e}")))
}

/// The suggested name without the filter's extension: some native dialogs (GTK) append it
/// themselves, and `with_extension` adds it afterwards only if it is still missing.
fn bare_name<'a>(name: &'a str, extension: Option<&str>) -> &'a str {
    match extension {
        Some(ext) => name
            .strip_suffix(ext)
            .and_then(|rest| rest.strip_suffix('.'))
            .unwrap_or(name),
        None => name,
    }
}

/// `path` with the filter's first extension added unless it already ends in one of them.
fn with_extension(path: PathBuf, extensions: &[String]) -> PathBuf {
    let Some(first) = extensions.first() else {
        return path;
    };
    let known = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)));
    if known {
        return path;
    }
    let mut name = path.clone().into_os_string();
    name.push(".");
    name.push(first);
    PathBuf::from(name)
}

/// Asks where to save a file; `None` when the user cancelled.
pub async fn pick_save_path(
    window: &WebviewWindow,
    dialog: SaveDialog,
) -> ApiResult<Option<PathBuf>> {
    let first = dialog.extensions.first().map(String::as_str);
    let mut builder =
        builder(window, &dialog.title).set_file_name(bare_name(&dialog.default_name, first));
    if !dialog.extensions.is_empty() {
        let extensions: Vec<&str> = dialog.extensions.iter().map(String::as_str).collect();
        builder = builder.add_filter(&dialog.filter_name, &extensions);
    }
    let Some(picked) = run(move || builder.blocking_save_file()).await? else {
        return Ok(None);
    };
    Ok(Some(with_extension(
        local_path(picked)?,
        &dialog.extensions,
    )))
}

/// Asks for one or more existing files; empty when the user cancelled.
pub async fn pick_files(window: &WebviewWindow, title: &str) -> ApiResult<Vec<PathBuf>> {
    let builder = builder(window, title);
    let picked = run(move || builder.blocking_pick_files()).await?;
    picked
        .unwrap_or_default()
        .into_iter()
        .map(local_path)
        .collect()
}

/// Asks for a folder, starting in `start`; `None` when the user cancelled.
pub async fn pick_folder(
    window: &WebviewWindow,
    title: &str,
    start: &Path,
) -> ApiResult<Option<PathBuf>> {
    let builder = builder(window, title).set_directory(start);
    run(move || builder.blocking_pick_folder())
        .await?
        .map(local_path)
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exts(list: &[&str]) -> Vec<String> {
        list.iter().map(|e| (*e).to_owned()).collect()
    }

    #[test]
    fn offers_the_suggested_name_without_the_filter_extension() {
        assert_eq!(bare_name("packets.json", Some("json")), "packets");
        assert_eq!(bare_name("packets.md", Some("json")), "packets.md");
        assert_eq!(bare_name("json", Some("json")), "json");
        assert_eq!(bare_name("song.ogg", None), "song.ogg");
    }

    #[test]
    fn adds_the_filter_extension_only_when_missing() {
        let json = exts(&["json"]);
        assert_eq!(
            with_extension(PathBuf::from("/tmp/packets"), &json),
            PathBuf::from("/tmp/packets.json")
        );
        assert_eq!(
            with_extension(PathBuf::from("/tmp/packets.JSON"), &json),
            PathBuf::from("/tmp/packets.JSON")
        );
        assert_eq!(
            with_extension(PathBuf::from("/tmp/packets.txt"), &json),
            PathBuf::from("/tmp/packets.txt.json")
        );
        assert_eq!(
            with_extension(PathBuf::from("/tmp/packets"), &[]),
            PathBuf::from("/tmp/packets")
        );
    }
}
