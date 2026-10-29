//! Where the caches live on this system (the categories of the "Clear cache" action). The
//! web engine's own directories differ per platform; its `localStorage` and `IndexedDB`
//! (the UI settings) are never listed here.

use std::path::{Path, PathBuf};

use mumcord_engine::cache::{CacheLocations, apply_pending_web_clear};
use tauri::{AppHandle, Manager};

/// The directories Tauri derives from the app identifier.
pub struct AppDirs {
    pub data: PathBuf,
    pub local_data: PathBuf,
    pub cache: PathBuf,
}

/// The cache locations while the app runs.
pub fn locations(app: &AppHandle) -> Result<CacheLocations, tauri::Error> {
    let path = app.path();
    Ok(locations_in(&AppDirs {
        data: path.app_data_dir()?,
        local_data: path.app_local_data_dir()?,
        cache: path.app_cache_dir()?,
    }))
}

/// Deletes a web cache that the last "Clear cache" scheduled. Called before Tauri starts, so
/// no web view has the files open.
pub fn apply_pending(identifier: &str) {
    if let Some(dirs) = startup_dirs(identifier) {
        apply_pending_web_clear(&locations_in(&dirs));
    }
}

fn locations_in(dirs: &AppDirs) -> CacheLocations {
    // Windows keeps the app cache dir and the web view data in one directory, which also
    // holds `localStorage`: it is no "temp files" category there.
    let temp =
        (dirs.cache != dirs.data && dirs.cache != dirs.local_data).then(|| dirs.cache.clone());
    CacheLocations {
        previews: dirs.data.join("link-previews"),
        web: web_dirs(dirs),
        temp,
        web_marker: dirs.data.join("clear-web-cache.pending"),
    }
}

#[cfg(target_os = "linux")]
fn web_dirs(dirs: &AppDirs) -> Vec<PathBuf> {
    vec![
        dirs.local_data.join("WebKitCache"),
        dirs.local_data.join("CacheStorage"),
    ]
}

#[cfg(target_os = "windows")]
fn web_dirs(dirs: &AppDirs) -> Vec<PathBuf> {
    let profile = dirs.local_data.join("EBWebView");
    vec![
        profile.join("Default").join("Cache"),
        profile.join("Default").join("Code Cache"),
        profile.join("Default").join("GPUCache"),
    ]
}

#[cfg(target_os = "macos")]
fn web_dirs(dirs: &AppDirs) -> Vec<PathBuf> {
    vec![dirs.cache.join("WebKit"), dirs.cache.join("fsCachedData")]
}

/// The directories before the app exists, from the same rules Tauri uses.
#[cfg(target_os = "linux")]
fn startup_dirs(identifier: &str) -> Option<AppDirs> {
    let base = |var: &str, fallback: &str| -> Option<PathBuf> {
        match std::env::var_os(var).map(PathBuf::from) {
            Some(dir) if dir.is_absolute() => Some(dir),
            _ => Some(Path::new(&std::env::var_os("HOME")?).join(fallback)),
        }
    };
    let data = base("XDG_DATA_HOME", ".local/share")?.join(identifier);
    Some(AppDirs {
        local_data: data.clone(),
        data,
        cache: base("XDG_CACHE_HOME", ".cache")?.join(identifier),
    })
}

#[cfg(target_os = "windows")]
fn startup_dirs(identifier: &str) -> Option<AppDirs> {
    let local = Path::new(&std::env::var_os("LOCALAPPDATA")?).join(identifier);
    Some(AppDirs {
        data: Path::new(&std::env::var_os("APPDATA")?).join(identifier),
        local_data: local.clone(),
        cache: local,
    })
}

#[cfg(target_os = "macos")]
fn startup_dirs(identifier: &str) -> Option<AppDirs> {
    let library = Path::new(&std::env::var_os("HOME")?).join("Library");
    let data = library.join("Application Support").join(identifier);
    Some(AppDirs {
        local_data: data.clone(),
        data,
        cache: library.join("Caches").join(identifier),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(data: &str, local: &str, cache: &str) -> AppDirs {
        AppDirs {
            data: data.into(),
            local_data: local.into(),
            cache: cache.into(),
        }
    }

    #[test]
    fn separate_cache_dir_is_the_temp_category() {
        let l = locations_in(&dirs("/d", "/d", "/c"));
        assert_eq!(l.temp, Some(PathBuf::from("/c")));
        assert_eq!(l.previews, PathBuf::from("/d/link-previews"));
    }

    #[test]
    fn cache_dir_shared_with_web_view_data_is_no_temp_category() {
        assert_eq!(
            locations_in(&dirs("/roaming", "/local", "/local")).temp,
            None
        );
    }
}
