//! Cache sizes and the "Clear cache" action. Only the directories listed in
//! [`CacheLocations`] are ever touched; the database, settings, certificates and the web
//! view's `localStorage` live elsewhere and are never part of a category.
//!
//! The web engine cannot be cleared while it runs without wiping its `localStorage` too
//! (which holds the UI settings), so that category is deleted at the next start: clearing
//! only writes a marker file, [`apply_pending_web_clear`] does the deleting before any web
//! view exists.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

/// One selectable group of cached data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CacheKind {
    /// Link previews, inline images and GIF renditions kept by the engine.
    Previews,
    /// The built-in browser's HTTP cache and cache storage.
    WebEngine,
    /// Temporary files in the app cache directory.
    Temp,
}

/// Where each category lives on this system.
#[derive(Debug, Clone)]
pub struct CacheLocations {
    pub previews: PathBuf,
    /// Cache-only directories of the web engine; never its `localStorage` or `IndexedDB`.
    pub web: Vec<PathBuf>,
    /// The app cache directory; the entries named in `web` are skipped (macOS keeps the
    /// web cache inside it).
    pub temp: Option<PathBuf>,
    /// Existence of this file means the web cache is deleted at the next start.
    pub web_marker: PathBuf,
}

/// Bytes used per category. A web cache waiting for deletion counts as empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheSizes {
    pub previews: u64,
    pub web: u64,
    pub temp: u64,
}

/// What a clear-out did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheCleared {
    /// Bytes freed now, plus the web cache that the next start will free.
    pub freed: u64,
    /// The web cache is only deleted at the next start.
    pub web_pending: bool,
}

/// Measures every category.
pub fn sizes(locations: &CacheLocations) -> CacheSizes {
    CacheSizes {
        previews: dir_size(&locations.previews, &[]),
        web: if locations.web_marker.exists() {
            0
        } else {
            locations.web.iter().map(|d| dir_size(d, &[])).sum()
        },
        temp: locations
            .temp
            .as_ref()
            .map_or(0, |d| dir_size(d, &locations.web)),
    }
}

/// Deletes the preview files and the temp files now (`kinds` permitting) and schedules the
/// web cache. The engine's in-memory preview cache is cleared by the caller.
pub fn clear_files(locations: &CacheLocations, kinds: &[CacheKind]) -> CacheCleared {
    let mut cleared = CacheCleared::default();
    if kinds.contains(&CacheKind::Previews) {
        cleared.freed += remove_contents(&locations.previews, &[]);
    }
    if kinds.contains(&CacheKind::Temp)
        && let Some(temp) = &locations.temp
    {
        cleared.freed += remove_contents(temp, &locations.web);
    }
    if kinds.contains(&CacheKind::WebEngine) {
        let size = sizes(locations).web;
        match fs::write(&locations.web_marker, b"") {
            Ok(()) => {
                cleared.freed += size;
                cleared.web_pending = true;
            }
            Err(e) => tracing::warn!(error = %e, "scheduling the web cache clean-up failed"),
        }
    }
    cleared
}

/// Deletes the web cache if a clear-out asked for it. Runs at start, before the web view.
pub fn apply_pending_web_clear(locations: &CacheLocations) {
    if !locations.web_marker.exists() {
        return;
    }
    for dir in &locations.web {
        if let Err(e) = remove_path(dir) {
            eprintln!("removing the web cache failed: {e}");
            // Keep the marker so the next start tries again.
            return;
        }
    }
    if let Err(e) = fs::remove_file(&locations.web_marker) {
        eprintln!("removing the web cache marker failed: {e}");
    }
}

/// Total size of the files below `dir`, without the entries listed in `skip`.
fn dir_size(dir: &Path, skip: &[PathBuf]) -> u64 {
    let Ok(read) = fs::read_dir(dir) else {
        return 0;
    };
    read.filter_map(Result::ok)
        .filter(|entry| !skip.contains(&entry.path()))
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path(), &[]),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// Deletes everything inside `dir` (not `dir` itself) except the entries in `skip`; returns
/// the freed bytes.
fn remove_contents(dir: &Path, skip: &[PathBuf]) -> u64 {
    let Ok(read) = fs::read_dir(dir) else {
        return 0;
    };
    let mut freed = 0;
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        if skip.contains(&path) {
            continue;
        }
        let size = match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&path, &[]),
            _ => entry.metadata().map_or(0, |m| m.len()),
        };
        match remove_path(&path) {
            Ok(()) => freed += size,
            Err(e) => tracing::warn!(error = %e, "removing a cache entry failed"),
        }
    }
    freed
}

/// Removes a file or a whole directory; a missing path is fine.
fn remove_path(path: &Path) -> io::Result<()> {
    let removed = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) => Err(e),
    };
    match removed {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake data directory with caches next to data that must survive.
    struct Fixture {
        _root: tempfile::TempDir,
        root: PathBuf,
        locations: CacheLocations,
    }

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        fs::write(path, vec![0u8; bytes]).expect("file");
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let data = root.join("data");
        let cache = root.join("cache");
        write(&data.join("link-previews/a.json"), 100);
        write(&data.join("link-previews/images/b.json"), 200);
        write(&data.join("WebKitCache/c"), 400);
        write(&data.join("CacheStorage/d"), 800);
        write(&cache.join("gstreamer/registry"), 1600);
        write(&cache.join("WebKit/e"), 3200);
        // Never part of a category.
        write(&data.join("mumcord.sqlite3"), 10);
        write(&data.join("localstorage/settings"), 10);
        write(&data.join("storage/certificate"), 10);
        let locations = CacheLocations {
            previews: data.join("link-previews"),
            web: vec![
                data.join("WebKitCache"),
                data.join("CacheStorage"),
                cache.join("WebKit"),
            ],
            temp: Some(cache),
            web_marker: data.join("clear-web-cache.pending"),
        };
        Fixture {
            _root: temp,
            root,
            locations,
        }
    }

    fn survivors(f: &Fixture) {
        for kept in [
            "data/mumcord.sqlite3",
            "data/localstorage/settings",
            "data/storage/certificate",
        ] {
            assert!(f.root.join(kept).is_file(), "{kept} must survive");
        }
    }

    #[test]
    fn measures_each_category_without_overlap() {
        let f = fixture();
        assert_eq!(
            sizes(&f.locations),
            CacheSizes {
                previews: 300,
                web: 400 + 800 + 3200,
                temp: 1600
            }
        );
    }

    #[test]
    fn missing_directories_count_as_empty() {
        let f = fixture();
        let nothing = CacheLocations {
            previews: f.root.join("nope"),
            web: vec![f.root.join("nope2")],
            temp: None,
            web_marker: f.root.join("marker"),
        };
        assert_eq!(sizes(&nothing), CacheSizes::default());
    }

    #[test]
    fn clears_only_the_selected_caches() {
        let f = fixture();
        let cleared = clear_files(&f.locations, &[CacheKind::Previews]);
        assert_eq!(cleared.freed, 300);
        assert!(!cleared.web_pending);
        assert!(!f.root.join("data/link-previews/a.json").exists());
        assert!(f.root.join("data/link-previews").is_dir());
        assert!(f.root.join("data/WebKitCache/c").is_file());
        assert!(f.root.join("cache/gstreamer/registry").is_file());
        survivors(&f);
    }

    #[test]
    fn temp_clearing_leaves_the_web_cache_inside_it() {
        let f = fixture();
        let cleared = clear_files(&f.locations, &[CacheKind::Temp]);
        assert_eq!(cleared.freed, 1600);
        assert!(!f.root.join("cache/gstreamer").exists());
        assert!(f.root.join("cache/WebKit/e").is_file());
        survivors(&f);
    }

    #[test]
    fn web_cache_is_deleted_at_the_next_start_only() {
        let f = fixture();
        let cleared = clear_files(&f.locations, &[CacheKind::WebEngine]);
        assert!(cleared.web_pending);
        assert_eq!(cleared.freed, 4400);
        assert!(f.root.join("data/WebKitCache/c").is_file(), "still running");
        assert_eq!(sizes(&f.locations).web, 0, "pending counts as empty");

        apply_pending_web_clear(&f.locations);
        assert!(!f.root.join("data/WebKitCache").exists());
        assert!(!f.root.join("data/CacheStorage").exists());
        assert!(!f.root.join("cache/WebKit").exists());
        assert!(!f.locations.web_marker.exists());
        assert!(f.root.join("cache/gstreamer/registry").is_file());
        assert!(f.root.join("data/link-previews/a.json").is_file());
        survivors(&f);
    }

    #[test]
    fn nothing_happens_at_start_without_a_marker() {
        let f = fixture();
        apply_pending_web_clear(&f.locations);
        assert!(f.root.join("data/WebKitCache/c").is_file());
    }
}
