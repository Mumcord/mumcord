//! AppImage only: lets the bundled GStreamer also load the host's plugins (FR-2.41a).
//!
//! The AppImage bundles GStreamer without H.264/AAC decoders (no gst-libav), so Twitch and
//! H.264 videos only play when the user installed gst-libav on the host. linuxdeploy's AppRun
//! hook points `GST_PLUGIN_PATH_1_0` and `GST_PLUGIN_SYSTEM_PATH_1_0` at the bundled plugins and
//! `GST_PLUGIN_SCANNER_1_0` at the bundled scanner. GStreamer scans `GST_PLUGIN_PATH` before
//! `GST_PLUGIN_SYSTEM_PATH` and keeps the first plugin of each file name, so the host directories
//! appended to the system path only add plugins the bundle lacks; bundled ones always win.
//!
//! The bundled scanner stays: it matches the bundled GStreamer core, and as a separate process it
//! keeps a host plugin that crashes on load from taking the app down. Builds on non-Debian hosts must
//! hand linuxdeploy the scanner (`scripts/gst-helpers-dir.sh`, see RELEASING.md). A host plugin built for a
//! different GStreamer version may fail to load; GStreamer then skips it and playback simply
//! stays without H.264 (the player shows the install hint).
//!
//! The plugin registry gets its own file: GStreamer's default one is shared with the host's
//! apps, and a host plugin that fails to load here would be cached there as broken and hidden
//! from the host's own players until it is updated. The AppImage's mount path changes on every
//! start, so a stale cache cannot hide plugins either: changed paths are rescanned.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Where distributions install GStreamer 1.0 plugins: Debian/Ubuntu (multiarch), Arch, Fedora
/// and openSUSE. Searched in this order.
const HOST_PLUGIN_DIRS: [&str; 4] = [
    "/usr/lib/x86_64-linux-gnu/gstreamer-1.0",
    "/usr/lib/aarch64-linux-gnu/gstreamer-1.0",
    "/usr/lib/gstreamer-1.0",
    "/usr/lib64/gstreamer-1.0",
];

const SYSTEM_PATH_VAR: &str = "GST_PLUGIN_SYSTEM_PATH_1_0";
const REGISTRY_VARS: [&str; 2] = ["GST_REGISTRY_1_0", "GST_REGISTRY"];

/// Extends the plugin path and sets the registry file. Must run before WebKit or GStreamer
/// start and before any other thread exists; does nothing outside an AppImage.
pub fn prepare(identifier: &str) {
    let appimage = crate::desktop_entry::is_appimage();
    let var = |name: &str| std::env::var_os(name);
    for (name, value) in changes(appimage, &var, identifier, |dir| dir.is_dir()) {
        // SAFETY: called at the very start of `run`, before Tauri, GTK or the async runtime
        // start any thread, so nothing reads the environment concurrently.
        unsafe { std::env::set_var(name, value) };
    }
}

/// The variables to set, read from the environment through `var`.
fn changes(
    appimage: bool,
    var: &dyn Fn(&str) -> Option<OsString>,
    identifier: &str,
    exists: impl Fn(&Path) -> bool,
) -> Vec<(&'static str, OsString)> {
    if !appimage {
        return Vec::new();
    }
    let mut changes = Vec::new();
    if let Some(path) = system_path(var(SYSTEM_PATH_VAR).as_deref(), exists) {
        changes.push((SYSTEM_PATH_VAR, path));
    }
    // A registry the user chose stays.
    if !REGISTRY_VARS.iter().any(|name| var(name).is_some())
        && let Some(file) = registry_file(
            var("XDG_CACHE_HOME").as_deref(),
            var("HOME").as_deref(),
            identifier,
        )
    {
        changes.push((REGISTRY_VARS[0], file.into_os_string()));
    }
    changes
}

/// The bundled system path with the existing host plugin directories appended, or `None` when
/// there is nothing to change. Without a bundled path (no AppRun hook) GStreamer already uses
/// the host's plugins, so it is left alone.
fn system_path(current: Option<&OsStr>, exists: impl Fn(&Path) -> bool) -> Option<OsString> {
    let current = current.filter(|path| !path.is_empty())?;
    // The AppImage runtime leaves an empty entry behind ("bundled:"); it names no directory.
    let mut dirs: Vec<PathBuf> = std::env::split_paths(current)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect();
    let before = dirs.len();
    for dir in HOST_PLUGIN_DIRS.map(Path::new) {
        if exists(dir) && !dirs.iter().any(|known| known == dir) {
            dirs.push(dir.to_path_buf());
        }
    }
    if dirs.len() == before {
        return None;
    }
    // The directories come from the hook and the constants above; none contains a `:`.
    std::env::join_paths(dirs).ok()
}

/// The AppImage's own registry file in the user's cache directory (GStreamer creates the
/// folder), named per architecture like GStreamer's default one.
fn registry_file(
    xdg_cache_home: Option<&OsStr>,
    home: Option<&OsStr>,
    identifier: &str,
) -> Option<PathBuf> {
    // The XDG spec ignores relative paths.
    let cache = match xdg_cache_home.map(Path::new) {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        _ => Path::new(home.filter(|home| !home.is_empty())?).join(".cache"),
    };
    let name = format!("registry.{}.bin", std::env::consts::ARCH);
    Some(cache.join(identifier).join("gstreamer-1.0").join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLED: &str = "/tmp/.mount_Mumcor1/usr/lib/gstreamer-1.0";

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let vars: Vec<(String, OsString)> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
            .collect();
        move |name| {
            vars.iter()
                .find(|(known, _)| known == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn changes_nothing_outside_an_appimage() {
        let var = env(&[(SYSTEM_PATH_VAR, BUNDLED), ("HOME", "/h")]);
        assert!(changes(false, &var, "app.id", |_| true).is_empty());
    }

    #[test]
    fn in_an_appimage_extends_the_path_and_sets_the_registry() {
        let var = env(&[(SYSTEM_PATH_VAR, BUNDLED), ("HOME", "/h")]);
        let changes = changes(true, &var, "app.id", |dir| {
            dir == Path::new("/usr/lib/gstreamer-1.0")
        });
        let arch = std::env::consts::ARCH;
        assert_eq!(
            changes,
            vec![
                (
                    SYSTEM_PATH_VAR,
                    OsString::from(format!("{BUNDLED}:/usr/lib/gstreamer-1.0"))
                ),
                (
                    "GST_REGISTRY_1_0",
                    OsString::from(format!(
                        "/h/.cache/app.id/gstreamer-1.0/registry.{arch}.bin"
                    ))
                ),
            ]
        );
    }

    #[test]
    fn keeps_a_registry_the_user_chose() {
        for name in REGISTRY_VARS {
            let var = env(&[(name, "/mine.bin"), ("HOME", "/h")]);
            assert!(changes(true, &var, "app.id", |_| false).is_empty());
        }
    }

    #[test]
    fn appends_existing_host_dirs_after_the_bundled_ones_in_order() {
        let path = system_path(Some(OsStr::new(BUNDLED)), |dir| {
            dir != Path::new("/usr/lib/aarch64-linux-gnu/gstreamer-1.0")
        });
        assert_eq!(
            path.as_deref(),
            Some(OsStr::new(
                "/tmp/.mount_Mumcor1/usr/lib/gstreamer-1.0:\
                 /usr/lib/x86_64-linux-gnu/gstreamer-1.0:\
                 /usr/lib/gstreamer-1.0:\
                 /usr/lib64/gstreamer-1.0"
            ))
        );
    }

    #[test]
    fn drops_the_empty_entry_the_runtime_leaves() {
        let current = format!("{BUNDLED}:");
        let path = system_path(Some(OsStr::new(&current)), |dir| {
            dir == Path::new("/usr/lib/gstreamer-1.0")
        });
        assert_eq!(
            path,
            Some(OsString::from(format!("{BUNDLED}:/usr/lib/gstreamer-1.0")))
        );
    }

    #[test]
    fn skips_missing_host_dirs() {
        let path = system_path(Some(OsStr::new(BUNDLED)), |dir| {
            dir == Path::new("/usr/lib64/gstreamer-1.0")
        });
        assert_eq!(
            path.as_deref(),
            Some(OsStr::new(
                "/tmp/.mount_Mumcor1/usr/lib/gstreamer-1.0:/usr/lib64/gstreamer-1.0"
            ))
        );
    }

    #[test]
    fn leaves_the_path_alone_without_a_bundled_path_or_host_dirs() {
        assert_eq!(system_path(None, |_| true), None);
        assert_eq!(system_path(Some(OsStr::new("")), |_| true), None);
        assert_eq!(system_path(Some(OsStr::new(BUNDLED)), |_| false), None);
    }

    #[test]
    fn does_not_add_a_dir_twice() {
        let current = format!("{BUNDLED}:/usr/lib/gstreamer-1.0");
        let path = system_path(Some(OsStr::new(&current)), |dir| {
            dir == Path::new("/usr/lib/gstreamer-1.0")
        });
        assert_eq!(path, None);
    }

    #[test]
    fn registry_lives_in_the_apps_cache_folder() {
        let arch = std::env::consts::ARCH;
        assert_eq!(
            registry_file(Some(OsStr::new("/c")), Some(OsStr::new("/h")), "app.id"),
            Some(PathBuf::from(format!(
                "/c/app.id/gstreamer-1.0/registry.{arch}.bin"
            )))
        );
        assert_eq!(
            registry_file(Some(OsStr::new("rel")), Some(OsStr::new("/h")), "app.id"),
            Some(PathBuf::from(format!(
                "/h/.cache/app.id/gstreamer-1.0/registry.{arch}.bin"
            )))
        );
        assert_eq!(registry_file(None, None, "app.id"), None);
    }
}
