//! Images dropped from the file manager onto the message input (FR-1.30).
//!
//! The native drag-and-drop handler hands the UI only file paths, so the UI asks for the
//! file contents with `read_dropped_image`. To keep that from becoming a way to read any
//! file, only paths that were just dropped onto the main window may be read, each once.

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
use tauri::{DragDropEvent, Manager, State, Window, WindowEvent};

use super::types::{ApiError, ApiResult, ErrorCode};
use crate::window::MAIN;

/// Larger files are refused; the UI would stall decoding them (matches `MAX_SOURCE_BYTES`).
pub const MAX_DROPPED_BYTES: u64 = 25 * 1024 * 1024;

/// How long a dropped path stays readable; the UI reads it right after the drop.
const ALLOW_FOR: Duration = Duration::from_secs(60);

/// Paths of the most recent drops with the time they were dropped.
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

fn read_image(path: &Path) -> ApiResult<DroppedImage> {
    let io =
        |e: std::io::Error| ApiError::new(ErrorCode::Io, format!("reading the file failed: {e}"));
    let file = File::open(path).map_err(io)?;
    if !file.metadata().map_err(io)?.is_file() {
        return Err(ApiError::new(ErrorCode::InvalidInput, "not a file"));
    }
    let mut bytes = Vec::new();
    // Reads one byte past the cap so a file that grew after the check is still refused.
    file.take(MAX_DROPPED_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_DROPPED_BYTES {
        return Err(ApiError::new(
            ErrorCode::MessageTooLarge,
            "the file is larger than 25 MB",
        ));
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

    #[test]
    fn refuses_directories() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dropped = DroppedFiles::default();
        let now = Instant::now();
        dropped.allow(&[dir.path().to_path_buf()], now);
        assert!(read_allowed(&dropped, dir.path(), now).is_err());
    }
}
