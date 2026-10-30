//! Portable mode on Windows (issue #181): a file named [`MARKER`] next to `Mumcord.exe` keeps
//! the database, settings, logs, caches and the web view's data in [`DATA_DIR`] beside the
//! exe instead of `%APPDATA%` and `%LOCALAPPDATA%`, so the folder can live on a USB stick.
//! The identity certificate stays in Windows Credential Manager (per Windows user), because a
//! private key in a plain file on a stick is easy to copy. Without the marker, and on Linux
//! and macOS, the paths are Tauri's usual ones.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tauri::{Manager, Runtime};

/// The marker file next to the exe that switches portable mode on.
#[cfg(any(target_os = "windows", test))]
const MARKER: &str = "portable";
/// The data folder next to the exe in portable mode.
#[cfg(any(target_os = "windows", test))]
const DATA_DIR: &str = "MumcordData";

/// The folders inside the portable data folder. The cache shares the web view's folder, as
/// Tauri's `%LOCALAPPDATA%` layout does on Windows (see `cache_dirs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub data: PathBuf,
    pub local_data: PathBuf,
    pub cache: PathBuf,
    pub log: PathBuf,
}

impl Dirs {
    fn in_root(root: &Path) -> Self {
        let local = root.join("local");
        Self {
            data: root.join("data"),
            local_data: local.clone(),
            cache: local,
            log: root.join("logs"),
        }
    }
}

/// The portable data folder for the program at `exe` if `is_file` finds the marker beside it.
#[cfg(any(target_os = "windows", test))]
fn root_for(exe: &Path, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let dir = exe.parent()?;
    is_file(&dir.join(MARKER)).then(|| dir.join(DATA_DIR))
}

#[cfg(target_os = "windows")]
fn detect() -> Option<PathBuf> {
    match std::env::current_exe() {
        Ok(exe) => root_for(&exe, Path::is_file),
        Err(e) => {
            eprintln!("cannot locate Mumcord.exe, portable mode is off: {e}");
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn detect() -> Option<PathBuf> {
    None
}

/// The portable folders, or `None` for a normal installation. Decided once per run.
pub fn dirs() -> Option<&'static Dirs> {
    static DIRS: OnceLock<Option<Dirs>> = OnceLock::new();
    DIRS.get_or_init(|| detect().as_deref().map(Dirs::in_root))
        .as_ref()
}

/// Whether this run is portable (About, diagnostics, update hint).
pub fn is_active() -> bool {
    dirs().is_some()
}

/// The database, settings and plugin folder.
pub fn data_dir<R: Runtime>(app: &impl Manager<R>) -> tauri::Result<PathBuf> {
    dirs().map_or_else(|| app.path().app_data_dir(), |d| Ok(d.data.clone()))
}

/// The web view's data (`EBWebView`) on Windows.
pub fn local_data_dir<R: Runtime>(app: &impl Manager<R>) -> tauri::Result<PathBuf> {
    dirs().map_or_else(
        || app.path().app_local_data_dir(),
        |d| Ok(d.local_data.clone()),
    )
}

/// Temporary files.
pub fn cache_dir<R: Runtime>(app: &impl Manager<R>) -> tauri::Result<PathBuf> {
    dirs().map_or_else(|| app.path().app_cache_dir(), |d| Ok(d.cache.clone()))
}

/// The log files.
pub fn log_dir<R: Runtime>(app: &impl Manager<R>) -> tauri::Result<PathBuf> {
    dirs().map_or_else(|| app.path().app_log_dir(), |d| Ok(d.log.clone()))
}

/// The web view data folder every window must use in portable mode; `None` keeps Tauri's
/// default. All windows share it, since WebView2 needs one data folder per process.
pub fn webview_data_dir() -> Option<PathBuf> {
    dirs().map(|d| d.local_data.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_next_to_the_exe_switches_portable_mode_on() {
        let exe = Path::new("/stick/Mumcord/Mumcord.exe");
        let root = root_for(exe, |p| p == Path::new("/stick/Mumcord/portable"));
        assert_eq!(root, Some(PathBuf::from("/stick/Mumcord/MumcordData")));
    }

    #[test]
    fn without_the_marker_the_normal_paths_stay() {
        let exe = Path::new("/stick/Mumcord/Mumcord.exe");
        assert_eq!(root_for(exe, |_| false), None);
        // A marker elsewhere, e.g. in the data folder, does not count.
        assert_eq!(
            root_for(exe, |p| p
                == Path::new("/stick/Mumcord/MumcordData/portable")),
            None
        );
    }

    #[test]
    fn an_exe_without_a_folder_is_never_portable() {
        assert_eq!(root_for(Path::new(""), |_| true), None);
    }

    #[test]
    fn everything_lives_under_the_data_folder() {
        let root = Path::new("/stick/Mumcord/MumcordData");
        let dirs = Dirs::in_root(root);
        assert_eq!(dirs.data, root.join("data"));
        assert_eq!(dirs.local_data, root.join("local"));
        assert_eq!(dirs.log, root.join("logs"));
        // Like Tauri's Windows layout: the cache shares the web view folder, so the "Clear
        // cache" action lists no separate temp category.
        assert_eq!(dirs.cache, dirs.local_data);
        for dir in [&dirs.data, &dirs.local_data, &dirs.cache, &dirs.log] {
            assert!(dir.starts_with(root));
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn only_windows_has_a_portable_mode() {
        assert!(!is_active());
        assert_eq!(webview_data_dir(), None);
    }

    /// The test binary in `target\debug\deps` has no marker beside it.
    #[cfg(target_os = "windows")]
    #[test]
    fn an_exe_without_the_marker_uses_the_normal_paths() {
        assert_eq!(detect(), None);
        assert!(!is_active());
    }
}
