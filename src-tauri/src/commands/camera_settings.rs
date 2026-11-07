//! Camera settings support (spec 08): the background image library with thumbnails for the
//! preview dialog and Settings → Camera (FR-8.11, FR-8.14, FR-8.15), and camera hot-plug
//! updates for their device pickers (FR-8.27). The camera settings themselves are fields of the
//! settings record (`settings_get` / `settings_update`).

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use image::ImageFormat;
use image::codecs::jpeg::JpegEncoder;
use mumcord_engine::Engine;
use mumcord_engine::store;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::camera::{CameraBackground, CameraDevice, CameraEffectQuality};
use super::types::{ApiError, ApiResult, ErrorCode};

/// Thumbnails fit into this box (the library tiles are about 120 × 68 CSS px; 2× for HiDPI).
const THUMB_WIDTH: u32 = 320;
const THUMB_HEIGHT: u32 = 180;
const THUMB_JPEG_QUALITY: u8 = 80;
/// Rescan interval when the OS gives no hot-plug notification: within the 2 s of FR-8.27.
const DEVICE_POLL: Duration = Duration::from_secs(1);

impl From<store::CameraBackground> for CameraBackground {
    fn from(b: store::CameraBackground) -> Self {
        match b {
            store::CameraBackground::None => Self::None,
            store::CameraBackground::Blur => Self::Blur,
            store::CameraBackground::Builtin(id) => Self::Builtin { id },
            store::CameraBackground::Own(id) => Self::Own { id },
        }
    }
}

impl From<store::CameraEffectQuality> for CameraEffectQuality {
    fn from(q: store::CameraEffectQuality) -> Self {
        match q {
            store::CameraEffectQuality::Auto => Self::Auto,
            store::CameraEffectQuality::Best => Self::Best,
            store::CameraEffectQuality::Light => Self::Light,
        }
    }
}

/// One background image of the library. The id goes into
/// `CameraBackground { kind: "builtin" | "own", id }`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CameraBackgroundImage {
    pub id: String,
    /// `data:image/jpeg;base64,…`, at most 320 × 180 px; `None` when the file can't be read.
    pub thumbnail: Option<String>,
}

/// The library: the six built-in images (sorted by id; the UI names them) and the user's own
/// images, oldest first ("Yours").
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CameraBackgroundLibrary {
    pub builtin: Vec<CameraBackgroundImage>,
    pub own: Vec<CameraBackgroundImage>,
}

/// The cameras changed (plugged in, removed, renamed or a new default); sent only while a
/// device watch runs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct CameraDevicesChanged {
    pub devices: Vec<CameraDevice>,
}

/// Thumbnails of the built-in images, made once per app run: decoding all of them took about a
/// second in release builds (several in debug builds), and the picker showed empty tiles that long
/// on every open. The shipped files never change while the app runs; own images are few and
/// can be replaced, so they are not cached.
static BUILTIN_THUMBNAILS: LazyLock<ThumbnailCache> = LazyLock::new(ThumbnailCache::default);

#[derive(Default)]
struct ThumbnailCache(Mutex<HashMap<PathBuf, String>>);

impl ThumbnailCache {
    /// The cached thumbnail of `path`, else the one `make` returns. A failure is not cached, so a
    /// later open tries again.
    fn get_or_make(
        &self,
        path: &Path,
        make: impl FnOnce(&Path) -> Option<String>,
    ) -> Option<String> {
        if let Some(hit) = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
        {
            return Some(hit.clone());
        }
        // Decoded without the lock, so parallel opens don't wait on each other's decoding.
        let made = make(path)?;
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_path_buf(), made.clone());
        Some(made)
    }
}

fn thumbnail(path: &Path) -> Option<String> {
    let picture = image::open(path)
        .inspect_err(|e| tracing::warn!(error = %e, "a camera background thumbnail failed"))
        .ok()?;
    let small = picture.thumbnail(THUMB_WIDTH, THUMB_HEIGHT).to_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(Cursor::new(&mut jpeg), THUMB_JPEG_QUALITY)
        .encode_image(&small)
        .inspect_err(|e| tracing::warn!(error = %e, "encoding a background thumbnail failed"))
        .ok()?;
    Some(format!(
        "data:{};base64,{}",
        ImageFormat::Jpeg.to_mime_type(),
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}

fn library_image(id: String, path: &Path) -> CameraBackgroundImage {
    CameraBackgroundImage {
        id,
        thumbnail: thumbnail(path),
    }
}

/// The background library with thumbnails.
#[tauri::command]
#[specta::specta]
pub async fn camera_backgrounds(engine: State<'_, Engine>) -> ApiResult<CameraBackgroundLibrary> {
    let engine = engine.inner().clone();
    blocking(move || {
        let builtin = engine
            .camera_builtin_backgrounds()
            .into_iter()
            .map(|b| CameraBackgroundImage {
                thumbnail: BUILTIN_THUMBNAILS.get_or_make(&b.path, thumbnail),
                id: b.id,
            })
            .collect();
        let own = engine
            .camera_own_backgrounds()?
            .into_iter()
            .map(|b| library_image(b.id, &b.path))
            .collect();
        Ok(CameraBackgroundLibrary { builtin, own })
    })
    .await
}

/// Adds an own image from the file the user picked (PNG, JPEG or WebP up to 10 MB). Only a
/// scaled, re-encoded copy without metadata is kept; `path` is not stored (FR-8.14). Error
/// `invalidInput` → "This image can't be used. Use a PNG, JPEG or WebP file up to 10 MB."
#[tauri::command]
#[specta::specta]
pub async fn camera_own_background_add(
    engine: State<'_, Engine>,
    path: String,
) -> ApiResult<CameraBackgroundImage> {
    let engine = engine.inner().clone();
    blocking(move || {
        let added = engine.camera_own_background_add(&PathBuf::from(path))?;
        Ok(library_image(added.id, &added.path))
    })
    .await
}

/// Deletes an own image. When it was the default background, the stored default becomes Blur
/// (FR-8.15); the UI reloads the settings afterwards. Error `notFound` for an unknown id.
#[tauri::command]
#[specta::specta]
pub fn camera_own_background_remove(engine: State<'_, Engine>, id: String) -> ApiResult<()> {
    Ok(engine.camera_own_background_remove(&id)?)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> ApiResult<T> + Send + 'static,
) -> ApiResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?
}

/// Signals for the device watch thread.
enum WatchSignal {
    /// The OS reported a change.
    Changed,
    Stop,
}

/// The device watch shared by the preview dialog and Settings → Camera: it runs while at least
/// one of them is open, so no camera is enumerated when nobody looks at a picker.
#[derive(Default)]
pub struct CameraDeviceWatch {
    state: Mutex<WatchState>,
}

#[derive(Default)]
struct WatchState {
    users: u32,
    thread: Option<Sender<WatchSignal>>,
}

impl CameraDeviceWatch {
    fn start(&self, app: AppHandle, engine: Engine) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.users += 1;
        if state.thread.is_none() {
            let (signals, received) = mpsc::channel();
            let notify = signals.clone();
            let spawned = std::thread::Builder::new()
                .name("camera-devices".into())
                .spawn(move || watch_devices(&app, &engine, notify, &received));
            match spawned {
                Ok(_) => state.thread = Some(signals),
                Err(e) => tracing::warn!(error = %e, "the camera device watch did not start"),
            }
        }
    }

    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.users = state.users.saturating_sub(1);
        if state.users == 0
            && let Some(thread) = state.thread.take()
        {
            // The thread may already have ended (no camera API); nothing is left to stop then.
            let _ = thread.send(WatchSignal::Stop);
        }
    }
}

fn device_list(engine: &Engine) -> Option<Vec<CameraDevice>> {
    let devices = engine
        .camera_devices()
        .inspect_err(|_| tracing::debug!("listing cameras for the device watch failed"))
        .ok()?;
    Some(
        devices
            .into_iter()
            .map(|d| CameraDevice {
                id: d.id.as_str().to_owned(),
                name: d.name,
                is_default: d.is_default,
            })
            .collect(),
    )
}

/// Uses the OS hot-plug notification where the camera API has one (Windows, macOS, Linux
/// PipeWire portal once connected) and otherwise rescans every second (Linux V4L2).
fn watch_devices(
    app: &AppHandle,
    engine: &Engine,
    notify: Sender<WatchSignal>,
    signals: &mpsc::Receiver<WatchSignal>,
) {
    let callback = Arc::new(move || {
        let _ = notify.send(WatchSignal::Changed);
    });
    let os_watch = match engine.camera_watch_devices(callback) {
        Ok(watch) => watch,
        Err(_) => {
            tracing::debug!("no camera API; the device watch ends");
            return;
        }
    };
    let polling = os_watch.is_none();
    let mut known = device_list(engine);
    loop {
        match signals.recv_timeout(DEVICE_POLL) {
            Ok(WatchSignal::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) if !polling => continue,
            Ok(WatchSignal::Changed) | Err(RecvTimeoutError::Timeout) => {}
        }
        let now = device_list(engine);
        if now.is_some() && now != known {
            if let Some(devices) = now.clone()
                && let Err(e) = (CameraDevicesChanged { devices }).emit(app)
            {
                tracing::warn!(error = %e, "sending cameraDevicesChanged failed");
            }
            known = now;
        }
    }
    drop(os_watch);
}

/// Starts sending `cameraDevicesChanged` while the preview dialog or Settings → Camera is open.
/// Every call needs a matching [`camera_devices_watch_stop`].
#[tauri::command]
#[specta::specta]
pub fn camera_devices_watch_start(
    app: AppHandle,
    engine: State<'_, Engine>,
    watch: State<'_, CameraDeviceWatch>,
) {
    watch.start(app, engine.inner().clone());
}

/// Ends one [`camera_devices_watch_start`]; the watch stops when no caller is left.
#[tauri::command]
#[specta::specta]
pub fn camera_devices_watch_stop(watch: State<'_, CameraDeviceWatch>) {
    watch.stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_backgrounds_convert_to_the_ui_shape_and_back() {
        for stored in [
            store::CameraBackground::None,
            store::CameraBackground::Blur,
            store::CameraBackground::Builtin("beach".into()),
            store::CameraBackground::Own("abc".into()),
        ] {
            let ui: CameraBackground = stored.clone().into();
            assert_eq!(store::CameraBackground::from(ui), stored);
        }
    }

    #[test]
    fn thumbnails_are_small_jpeg_data_urls() {
        let dir = tempfile::tempdir().expect("test setup");
        let path = dir.path().join("wide.png");
        image::RgbImage::new(1280, 720)
            .save(&path)
            .expect("test setup");
        let url = thumbnail(&path).expect("test setup");
        let data = url
            .strip_prefix("data:image/jpeg;base64,")
            .expect("test setup");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .expect("test setup");
        let small = image::load_from_memory(&bytes).expect("test setup");
        assert_eq!((small.width(), small.height()), (320, 180));
    }

    #[test]
    fn every_built_in_background_has_a_thumbnail() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/backgrounds");
        let mut count = 0;
        for entry in std::fs::read_dir(&dir).expect("test setup") {
            let path = entry.expect("test setup").path();
            assert!(
                thumbnail(&path).is_some(),
                "no thumbnail for {}",
                path.display()
            );
            count += 1;
        }
        assert_eq!(count, 18);
    }

    #[test]
    fn a_cached_thumbnail_is_made_only_once() {
        let cache = ThumbnailCache::default();
        let path = Path::new("beach.webp");
        let mut made = 0;
        for _ in 0..3 {
            let url = cache.get_or_make(path, |_| {
                made += 1;
                Some("data:image/jpeg;base64,AA==".to_owned())
            });
            assert_eq!(url.as_deref(), Some("data:image/jpeg;base64,AA=="));
        }
        assert_eq!(made, 1);
    }

    #[test]
    fn a_failed_thumbnail_is_tried_again() {
        let cache = ThumbnailCache::default();
        let path = Path::new("broken.webp");
        assert_eq!(cache.get_or_make(path, |_| None), None);
        let url = cache.get_or_make(path, |_| Some("data:image/jpeg;base64,AA==".to_owned()));
        assert_eq!(url.as_deref(), Some("data:image/jpeg;base64,AA=="));
    }

    #[test]
    fn an_unreadable_file_has_no_thumbnail() {
        let dir = tempfile::tempdir().expect("test setup");
        let path = dir.path().join("broken.jpg");
        std::fs::write(&path, b"not a picture").expect("test setup");
        assert_eq!(thumbnail(&path), None);
    }
}
