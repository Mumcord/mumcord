//! The background image library for the preview and Settings → Camera (FR-8.11, FR-8.14,
//! FR-8.15), and camera hot-plug notifications for the device pickers (FR-8.27).

use std::path::{Path, PathBuf};

use mumcord_camera::{CameraError, HotplugCallback, HotplugWatch};
use mumcord_store::{CameraBackgrounds, OwnBackground};

use super::session::CameraFailure;
use crate::session::{Engine, EngineResult, lock, store_error};

/// File types the built-in folder may hold, in lookup order (same as the effects resolver).
const BUILTIN_EXTENSIONS: [&str; 3] = ["jpg", "png", "webp"];

/// One built-in background image shipped with the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinBackground {
    /// Used as `builtin:{id}` in the settings.
    pub id: String,
    pub path: PathBuf,
}

impl Engine {
    /// The built-in background images, sorted by id. Empty when the resource folder is missing.
    pub fn camera_builtin_backgrounds(&self) -> Vec<BuiltinBackground> {
        let dir = lock(self.camera_state()).builtin_backgrounds.clone();
        dir.map(|dir| builtin_in(&dir)).unwrap_or_default()
    }

    /// The user's own background images, oldest first.
    pub fn camera_own_backgrounds(&self) -> EngineResult<Vec<OwnBackground>> {
        self.own_backgrounds().list().map_err(store_error)
    }

    /// Copies the image at `source` into the profile's own images: checked, scaled and
    /// re-encoded without metadata; the source path is not kept (FR-8.14).
    pub fn camera_own_background_add(&self, source: &Path) -> EngineResult<OwnBackground> {
        self.own_backgrounds().add_file(source).map_err(store_error)
    }

    /// Deletes an own image; when it was the default background, the default becomes Blur
    /// (FR-8.15).
    pub fn camera_own_background_remove(&self, id: &str) -> EngineResult<()> {
        let store = self.store_handle();
        let store = lock(&store);
        self.own_backgrounds()
            .delete(&store, id)
            .map_err(store_error)
    }

    /// Registers `changed` for OS camera hot-plug notifications. `Ok(None)` means this OS path
    /// has none and the caller must poll [`Engine::camera_devices`] (every second for the 2 s of
    /// FR-8.27).
    pub fn camera_watch_devices(
        &self,
        changed: HotplugCallback,
    ) -> Result<Option<HotplugWatch>, CameraFailure> {
        let backend = lock(self.camera_state())
            .backend()
            .ok_or(CameraFailure::Camera(CameraError::NoCamera))?;
        backend.watch(changed).map_err(CameraFailure::Camera)
    }

    fn own_backgrounds(&self) -> CameraBackgrounds {
        CameraBackgrounds::new(self.data_dir())
    }
}

/// The images in `dir` named `{id}.{jpg|png|webp}`; the first extension wins for a duplicate id.
fn builtin_in(dir: &Path) -> Vec<BuiltinBackground> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<BuiltinBackground> = Vec::new();
    for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
        let (Some(id), Some(ext)) = (
            path.file_stem().and_then(|s| s.to_str()),
            path.extension().and_then(|s| s.to_str()),
        ) else {
            continue;
        };
        let Some(rank) = BUILTIN_EXTENSIONS.iter().position(|known| *known == ext) else {
            continue;
        };
        match found.iter_mut().find(|known| known.id == id) {
            Some(known) => {
                if Some(rank) < extension_rank(&known.path) {
                    known.path = path;
                }
            }
            None => found.push(BuiltinBackground {
                id: id.to_owned(),
                path,
            }),
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

fn extension_rank(path: &Path) -> Option<usize> {
    let ext = path.extension()?.to_str()?;
    BUILTIN_EXTENSIONS.iter().position(|known| *known == ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_builtin_images_by_id_and_skips_other_files() {
        let dir = tempfile::tempdir().expect("test setup");
        for name in [
            "studio.jpg",
            "beach.webp",
            "beach.jpg",
            "notes.txt",
            "README",
        ] {
            std::fs::write(dir.path().join(name), b"x").expect("test setup");
        }
        let found = builtin_in(dir.path());
        let ids: Vec<&str> = found.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["beach", "studio"]);
        assert_eq!(found[0].path, dir.path().join("beach.jpg"));
    }

    #[test]
    fn a_missing_folder_has_no_builtin_images() {
        assert!(builtin_in(Path::new("/nonexistent/camera-backgrounds")).is_empty());
    }
}
