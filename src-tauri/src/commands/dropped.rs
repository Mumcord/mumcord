//! Images dropped from the file manager onto the message input (FR-1.30), or copied there
//! and pasted (issue #50).
//!
//! The native drag-and-drop handler hands the UI only file paths, so the UI asks for the
//! file contents with `read_dropped_image`. To keep that from becoming a way to read any
//! file, only paths that were just dropped onto the main window may be read, each once.
//! A paste works the same way: `clipboard_files` reads the copied paths from the system
//! clipboard (never from the UI) and allows them like dropped ones. Copied image data
//! without a file behind it, such as a screenshot, comes from `clipboard_image` (issue #110).

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, DragDropEvent, Manager, State, Window, WindowEvent};

use super::types::{ApiError, ApiResult, ErrorCode};
use crate::window::MAIN;

/// Larger files are refused; the UI would stall decoding them (matches `MAX_SOURCE_BYTES`).
pub const MAX_DROPPED_BYTES: u64 = 25 * 1024 * 1024;

/// How long a dropped path stays readable; the UI reads it right after the drop.
const ALLOW_FOR: Duration = Duration::from_secs(60);

/// Paths of the most recent drops and pastes with the time they were dropped or pasted.
#[derive(Default)]
pub struct DroppedFiles {
    allowed: Mutex<HashMap<PathBuf, Instant>>,
}

impl DroppedFiles {
    fn allow(&self, paths: &[PathBuf], now: Instant) {
        let mut allowed = self.lock();
        allowed.retain(|_, at| now.duration_since(*at) < ALLOW_FOR);
        for path in paths {
            allowed.insert(path.clone(), now);
        }
    }

    /// Removes `path` from the list; true when it was dropped recently.
    fn take(&self, path: &Path, now: Instant) -> bool {
        self.lock()
            .remove(path)
            .is_some_and(|at| now.duration_since(at) < ALLOW_FOR)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Instant>> {
        // The map stays consistent even if a holder panicked; keep using it.
        self.allowed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Records the paths of a drop onto the main window so the UI may read them.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN {
        return;
    }
    if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
        window.state::<DroppedFiles>().allow(paths, Instant::now());
    }
}

/// A dropped image file, ready to go into the attachment tray.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct DroppedImage {
    /// File name without the folder.
    pub name: String,
    /// MIME type detected from the file contents, not the extension.
    pub mime: String,
    /// The file contents, base64-encoded.
    pub data: String,
}

/// Detects the image types the composer can send from the first bytes of the file.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn too_large() -> ApiError {
    ApiError::new(ErrorCode::MessageTooLarge, "the file is larger than 25 MB")
}

fn not_a_file() -> ApiError {
    ApiError::new(ErrorCode::InvalidInput, "not a file")
}

fn read_image(path: &Path) -> ApiResult<DroppedImage> {
    let io =
        |e: std::io::Error| ApiError::new(ErrorCode::Io, format!("reading the file failed: {e}"));
    // Checked before opening: opening a FIFO or a device, also behind a symlink, can block
    // or have side effects, and a huge file should not be read at all.
    let meta = std::fs::metadata(path).map_err(io)?;
    if !meta.is_file() {
        return Err(not_a_file());
    }
    if meta.len() > MAX_DROPPED_BYTES {
        return Err(too_large());
    }
    let file = File::open(path).map_err(io)?;
    // The path may have been replaced between the check and the open.
    if !file.metadata().map_err(io)?.is_file() {
        return Err(not_a_file());
    }
    let mut bytes = Vec::new();
    // Reads one byte past the cap so a file that grew after the check is still refused.
    file.take(MAX_DROPPED_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_DROPPED_BYTES {
        return Err(too_large());
    }
    let mime = sniff_image(&bytes)
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidInput, "not a supported image"))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(DroppedImage {
        name,
        mime: mime.to_owned(),
        data: STANDARD.encode(&bytes),
    })
}

fn ensure_dropped(dropped: &DroppedFiles, path: &Path, now: Instant) -> ApiResult<()> {
    if dropped.take(path, now) {
        Ok(())
    } else {
        Err(ApiError::new(
            ErrorCode::NotFound,
            "the file was not dropped onto Mumcord",
        ))
    }
}

/// Reads an image file that was just dropped onto the window (once per drop). Fails with
/// `notFound` for any other path, `messageTooLarge` above 25 MB and `invalidInput` when the
/// file is not a PNG, JPEG, GIF or WebP image.
#[tauri::command]
#[specta::specta]
pub async fn read_dropped_image(
    dropped: State<'_, DroppedFiles>,
    path: String,
) -> ApiResult<DroppedImage> {
    let path = PathBuf::from(path);
    ensure_dropped(&dropped, &path, Instant::now())?;
    // Reading up to 25 MB must not hold up the async runtime.
    tauri::async_runtime::spawn_blocking(move || read_image(&path))
        .await
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
}

/// The local path of a `file://` URI from the clipboard; other schemes and remote hosts are
/// skipped.
#[cfg(target_os = "linux")]
fn local_path(uri: &str) -> Option<PathBuf> {
    let (path, host) = gtk::glib::filename_from_uri(uri).ok()?;
    match host.as_deref() {
        None | Some("") | Some("localhost") => Some(path),
        Some(_) => None,
    }
}

/// Reads the files copied in the file manager. WebKitGTK lists them on paste but hands the
/// page neither the files nor their paths, so they come from the GTK clipboard instead,
/// which also works on Wayland, where only the focused window may read the clipboard.
#[cfg(target_os = "linux")]
async fn copied_files(app: &AppHandle) -> ApiResult<Vec<PathBuf>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).request_uris(move |_, uris| {
            let paths = uris.iter().filter_map(|uri| local_path(uri)).collect();
            // Fails only when the command was cancelled; nobody waits for the paths then.
            let _ = tx.send(paths);
        });
    })
    .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
    rx.await
        .map_err(|_| ApiError::new(ErrorCode::Io, "the clipboard did not answer"))
}

/// WebView2 and WKWebView hand pasted files to the page as `File` objects, so the UI does
/// not need this there.
#[cfg(not(target_os = "linux"))]
async fn copied_files(_app: &AppHandle) -> ApiResult<Vec<PathBuf>> {
    Ok(Vec::new())
}

/// Paths of the files copied to the clipboard, for a paste the webview cannot hand over
/// (issue #50). Each may then be read once with `read_dropped_image`. Paths that are not
/// valid UTF-8 are skipped: the UI could not hand them back unchanged.
#[tauri::command]
#[specta::specta]
pub async fn clipboard_files(
    app: AppHandle,
    dropped: State<'_, DroppedFiles>,
) -> ApiResult<Vec<String>> {
    let paths = copied_files(&app).await.inspect_err(|e| {
        tracing::warn!(code = ?e.code, "reading the copied files from the clipboard failed");
    })?;
    dropped.allow(&paths, Instant::now());
    Ok(paths
        .into_iter()
        .filter_map(|path| path.into_os_string().into_string().ok())
        .collect())
}

/// Reads PNG data from the clipboard, empty when it holds none. The WebKitGTK bundled in the
/// AppImage hands the page nothing on paste for copied image data (issue #110). Screenshot
/// tools and browsers all offer PNG, so no other format is asked for.
#[cfg(target_os = "linux")]
async fn copied_png(app: &AppHandle) -> ApiResult<Vec<u8>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let png = gdk::Atom::intern("image/png");
        gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).request_contents(&png, move |_, data| {
            // Fails only when the command was cancelled; nobody waits for the data then.
            let _ = tx.send(data.data());
        });
    })
    .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
    rx.await
        .map_err(|_| ApiError::new(ErrorCode::Io, "the clipboard did not answer"))
}

/// WebView2 and WKWebView hand pasted image data to the page as a `File`, so the UI does
/// not need this there.
#[cfg(not(target_os = "linux"))]
async fn copied_png(_app: &AppHandle) -> ApiResult<Vec<u8>> {
    Ok(Vec::new())
}

fn clipboard_png(bytes: Vec<u8>) -> ApiResult<Option<DroppedImage>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() as u64 > MAX_DROPPED_BYTES {
        return Err(too_large());
    }
    if sniff_image(&bytes) != Some("image/png") {
        return Err(ApiError::new(
            ErrorCode::InvalidInput,
            "not a supported image",
        ));
    }
    Ok(Some(DroppedImage {
        name: "image.png".to_owned(),
        mime: "image/png".to_owned(),
        data: STANDARD.encode(&bytes),
    }))
}

/// The image data copied to the clipboard, for a paste the webview cannot hand over (issue
/// #110); `null` when the clipboard holds none. Fails with `messageTooLarge` above 25 MB and
/// `invalidInput` when the data is not a PNG image.
#[tauri::command]
#[specta::specta]
pub async fn clipboard_image(app: AppHandle) -> ApiResult<Option<DroppedImage>> {
    let bytes = copied_png(&app).await.inspect_err(|e| {
        tracing::warn!(code = ?e.code, "reading the copied image from the clipboard failed");
    })?;
    clipboard_png(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    fn read_allowed(dropped: &DroppedFiles, path: &Path, now: Instant) -> ApiResult<DroppedImage> {
        ensure_dropped(dropped, path, now)?;
        read_image(path)
    }

    fn temp_file(contents: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("picture.png");
        File::create(&path)
            .and_then(|mut f| f.write_all(contents))
            .expect("write temp file");
        (dir, path)
    }

    #[test]
    fn takes_png_data_from_the_clipboard() {
        assert_eq!(
            clipboard_png(Vec::new()).expect("no data is no error"),
            None
        );
        let image = clipboard_png(PNG.to_vec())
            .expect("png data is taken")
            .expect("png data is an image");
        assert_eq!(image.name, "image.png");
        assert_eq!(image.mime, "image/png");
        assert_eq!(STANDARD.decode(image.data).expect("base64"), PNG);
    }

    #[test]
    fn refuses_clipboard_data_that_is_no_png_or_too_large() {
        let err = clipboard_png(b"<html>".to_vec()).expect_err("html is refused");
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let mut big = PNG.to_vec();
        big.resize(usize::try_from(MAX_DROPPED_BYTES).expect("fits") + 1, 0);
        let err = clipboard_png(big).expect_err("too large is refused");
        assert_eq!(err.code, ErrorCode::MessageTooLarge);
    }

    #[test]
    fn sniffs_supported_image_types() {
        assert_eq!(sniff_image(PNG), Some("image/png"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a..."), Some("image/gif"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image(b"%PDF-1.7"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[test]
    fn reads_a_dropped_image_once() {
        let (_dir, path) = temp_file(PNG);
        let dropped = DroppedFiles::default();
        let now = Instant::now();
        dropped.allow(std::slice::from_ref(&path), now);
        let image = read_allowed(&dropped, &path, now).expect("dropped image is readable");
        assert_eq!(image.name, "picture.png");
        assert_eq!(image.mime, "image/png");
        assert_eq!(STANDARD.decode(image.data).expect("base64"), PNG);
        let again = read_allowed(&dropped, &path, now).expect_err("second read is refused");
        assert_eq!(again.code, ErrorCode::NotFound);
    }

    #[test]
    fn refuses_paths_that_were_not_dropped() {
        let (_dir, path) = temp_file(PNG);
        let dropped = DroppedFiles::default();
        let err = read_allowed(&dropped, &path, Instant::now()).expect_err("not dropped");
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn dropped_paths_expire() {
        let (_dir, path) = temp_file(PNG);
        let dropped = DroppedFiles::default();
        let then = Instant::now();
        dropped.allow(std::slice::from_ref(&path), then);
        let err = read_allowed(&dropped, &path, then + ALLOW_FOR).expect_err("expired");
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn refuses_files_that_are_not_images() {
        let (_dir, path) = temp_file(b"#!/bin/sh\necho hi\n");
        let dropped = DroppedFiles::default();
        let now = Instant::now();
        dropped.allow(std::slice::from_ref(&path), now);
        let err = read_allowed(&dropped, &path, now).expect_err("not an image");
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn refuses_files_over_the_cap() {
        let size = usize::try_from(MAX_DROPPED_BYTES).expect("fits") + 1;
        let mut contents = PNG.to_vec();
        contents.resize(size, 0);
        let (_dir, path) = temp_file(&contents);
        let dropped = DroppedFiles::default();
        let now = Instant::now();
        dropped.allow(std::slice::from_ref(&path), now);
        let err = read_allowed(&dropped, &path, now).expect_err("too large");
        assert_eq!(err.code, ErrorCode::MessageTooLarge);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn takes_only_local_file_uris() {
        assert_eq!(
            local_path("file:///home/me/My%20cat.png"),
            Some(PathBuf::from("/home/me/My cat.png"))
        );
        assert_eq!(
            local_path("file://localhost/tmp/a.png"),
            Some(PathBuf::from("/tmp/a.png"))
        );
        assert_eq!(local_path("file://other-host/tmp/a.png"), None);
        assert_eq!(local_path("https://example.com/a.png"), None);
        assert_eq!(local_path("/tmp/a.png"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn refuses_a_symlink_to_a_fifo_without_blocking() {
        let dir = tempfile::tempdir().expect("temp dir");
        let fifo = dir.path().join("pipe");
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).expect("path");
        // SAFETY: `c_path` is a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let link = dir.path().join("picture.png");
        std::os::unix::fs::symlink(&fifo, &link).expect("symlink");
        let dropped = DroppedFiles::default();
        let now = Instant::now();
        dropped.allow(std::slice::from_ref(&link), now);
        let err = read_allowed(&dropped, &link, now).expect_err("a FIFO is no file");
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn refuses_a_sparse_file_over_the_cap_before_reading_it() {
        let (_dir, path) = temp_file(PNG);
        // A 1 GB file that takes no disk space: refused from its size alone.
        File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_len(1 << 30))
            .expect("grow file");
        let dropped = DroppedFiles::default();
        let now = Instant::now();
        dropped.allow(std::slice::from_ref(&path), now);
        let err = read_allowed(&dropped, &path, now).expect_err("too large");
        assert_eq!(err.code, ErrorCode::MessageTooLarge);
    }

    #[test]
    fn refuses_directories() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dropped = DroppedFiles::default();
        let now = Instant::now();
        dropped.allow(&[dir.path().to_path_buf()], now);
        assert!(read_allowed(&dropped, dir.path(), now).is_err());
    }
}
