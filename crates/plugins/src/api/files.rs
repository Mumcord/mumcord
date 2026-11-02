//! Files the user picked in a plugin's `file-picker` (FR-6.41). The plugin only gets an opaque
//! handle, and the only thing it can do with it is upload the file with `net.http` (FR-6.37).
//! Handles belong to the plugin whose picker produced them and live in memory only.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tokio::io::AsyncReadExt;

use crate::runtime::wit::http::FileHandle;

/// Largest file a plugin may upload.
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
/// Picked files remembered per plugin; picking more forgets the oldest.
pub const MAX_FILES: usize = 16;

/// Why a picked file cannot be handed to the plugin. The texts are shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("This is not a file")]
    NotAFile,
    #[error("The file is larger than 50 MiB")]
    TooLarge,
    #[error("Can't read this file")]
    Unreadable(#[source] io::Error),
}

/// The picked files of all plugins; lives in [`crate::HostContext`].
#[derive(Debug, Default)]
pub struct Files {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    picked: HashMap<String, VecDeque<Picked>>,
    last_handle: FileHandle,
}

/// One file the user picked for a plugin.
#[derive(Debug, Clone)]
pub(crate) struct Picked {
    handle: FileHandle,
    path: PathBuf,
}

impl Files {
    /// Registers a file the user picked in a `file-picker` of `plugin` and returns the handle the
    /// UI glue passes to the plugin in its `ui-action`.
    pub fn register(&self, plugin: &str, path: &Path) -> Result<FileHandle, FileError> {
        let meta = std::fs::metadata(path).map_err(FileError::Unreadable)?;
        if !meta.is_file() {
            return Err(FileError::NotAFile);
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(FileError::TooLarge);
        }
        let mut state = self.lock();
        state.last_handle += 1;
        let handle = state.last_handle;
        let list = state.picked.entry(plugin.to_owned()).or_default();
        if list.len() == MAX_FILES {
            list.pop_front();
        }
        list.push_back(Picked {
            handle,
            path: path.to_owned(),
        });
        Ok(handle)
    }

    /// The file behind `handle`, if `plugin` got that handle from its own picker.
    pub(crate) fn get(&self, plugin: &str, handle: FileHandle) -> Option<Picked> {
        self.lock()
            .picked
            .get(plugin)?
            .iter()
            .find(|p| p.handle == handle)
            .cloned()
    }

    /// Forgets every file picked for `plugin` (reset data, remove).
    pub fn forget(&self, plugin: &str) {
        self.lock().picked.remove(plugin);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Picked {
    /// The file name sent in a multipart upload.
    pub(crate) fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_owned())
    }

    /// Reads the whole file; it may have grown since it was picked, so the cap is checked again.
    pub(crate) async fn read(&self) -> io::Result<Vec<u8>> {
        let file = tokio::fs::File::open(&self.path).await?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(io::Error::other("the file is larger than 50 MiB"));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_belong_to_one_plugin() {
        let dir = tempfile::tempdir().expect("test value");
        let path = dir.path().join("song.mp3");
        std::fs::write(&path, b"abc").expect("test value");
        let files = Files::default();
        let handle = files.register("mumbot", &path).expect("test value");
        assert_eq!(
            files.get("mumbot", handle).expect("test value").name(),
            "song.mp3"
        );
        assert!(files.get("dice", handle).is_none());
        files.forget("mumbot");
        assert!(files.get("mumbot", handle).is_none());
    }

    #[test]
    fn directories_and_missing_files_are_refused() {
        let dir = tempfile::tempdir().expect("test value");
        let files = Files::default();
        assert!(matches!(
            files.register("mumbot", dir.path()),
            Err(FileError::NotAFile)
        ));
        assert!(matches!(
            files.register("mumbot", &dir.path().join("missing")),
            Err(FileError::Unreadable(_))
        ));
    }

    #[test]
    fn too_large_files_are_refused() {
        let dir = tempfile::tempdir().expect("test value");
        let path = dir.path().join("big.bin");
        let file = std::fs::File::create(&path).expect("test value");
        file.set_len(MAX_FILE_BYTES + 1).expect("test value");
        assert!(matches!(
            Files::default().register("mumbot", &path),
            Err(FileError::TooLarge)
        ));
    }

    #[test]
    fn only_the_newest_files_are_kept() {
        let dir = tempfile::tempdir().expect("test value");
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"a").expect("test value");
        let files = Files::default();
        let first = files.register("mumbot", &path).expect("test value");
        for _ in 0..MAX_FILES {
            files.register("mumbot", &path).expect("test value");
        }
        assert!(files.get("mumbot", first).is_none());
        assert!(files.get("mumbot", first + 1).is_some());
    }
}
