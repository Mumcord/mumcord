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
//!
//! A host directory reached through a symlink (`/usr/lib64` -> `/usr/lib` on Arch) is added only
//! once. GStreamer keys its registry by plugin file name, so the same plugins under two paths
//! replace each other on every scan: each new web process (every window) then rescanned all host
//! plugins, which delayed the first paint by seconds.
//!
//! The AppRun hook also turns off reusing the scanner, so every plugin got its own scanner
//! process. As the mount path changes on every start, all bundled plugins are rescanned on every
//! start, and a hundred process starts cost about half a second before the first paint. One
//! reused scanner is still a separate process, so a crashing plugin still cannot take the app
//! down: GStreamer restarts the scanner and skips that plugin.

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
const REUSE_SCANNER_VAR: &str = "GST_REGISTRY_REUSE_PLUGIN_SCANNER";

/// Extends the plugin path and sets the registry file. Must run before WebKit or GStreamer
/// start and before any other thread exists; does nothing outside an AppImage.
pub fn prepare(identifier: &str) {
    let appimage = crate::desktop_entry::is_appimage();
    let var = |name: &str| std::env::var_os(name);
    let resolve = |dir: &Path| std::fs::canonicalize(dir).ok().filter(|real| real.is_dir());
    for (name, value) in changes(appimage, &var, identifier, resolve) {
        // SAFETY: called at the very start of `run`, before Tauri, GTK or the async runtime
        // start any thread, so nothing reads the environment concurrently.
        unsafe { std::env::set_var(name, value) };
    }
}

/// What the AppImage's last GStreamer scan says about H.264 (#567). The host's gst-libav only
/// helps when the bundled GStreamer can load it; a host plugin built for a newer GStreamer is
/// skipped without a message, so the hint "install gst-libav" would be wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H264Status {
    /// The decoder is in the registry.
    Available,
    /// The host has gst-libav, but the registry lacks its decoder: it failed to load.
    HostPluginNotLoaded,
    /// No gst-libav on the host.
    HostPluginMissing,
    /// No registry yet (no video played since the cache was cleared) or not an AppImage.
    Unknown,
}

/// Logs [`H264Status`] once at start; outside an AppImage GStreamer is the system's own.
pub fn log_h264_status(identifier: &str) {
    if !crate::desktop_entry::is_appimage() {
        return;
    }
    let var = |name: &str| std::env::var_os(name);
    let registry = REGISTRY_VARS
        .iter()
        .find_map(|name| var(name).map(PathBuf::from))
        .or_else(|| {
            registry_file(
                var("XDG_CACHE_HOME").as_deref(),
                var("HOME").as_deref(),
                identifier,
            )
        });
    let registry = registry.and_then(|file| std::fs::read(file).ok());
    let host_libav = HOST_PLUGIN_DIRS
        .iter()
        .any(|dir| Path::new(dir).join("libgstlibav.so").is_file());
    let status = h264_status(registry.as_deref(), host_libav);
    match status {
        H264Status::HostPluginNotLoaded => tracing::warn!(
            ?status,
            "the system's gst-libav is installed but the AppImage's GStreamer cannot load it \
             (it is built for a newer GStreamer); H.264 videos such as Twitch need the \
             distribution package of Mumcord"
        ),
        _ => tracing::info!(?status, "AppImage H.264 support"),
    }
}

fn h264_status(registry: Option<&[u8]>, host_libav: bool) -> H264Status {
    let Some(registry) = registry else {
        return H264Status::Unknown;
    };
    let has = |needle: &[u8]| registry.windows(needle.len()).any(|w| w == needle);
    if has(b"avdec_h264") {
        H264Status::Available
    } else if host_libav {
        H264Status::HostPluginNotLoaded
    } else {
        H264Status::HostPluginMissing
    }
}

/// The variables to set, read from the environment through `var`.
fn changes(
    appimage: bool,
    var: &dyn Fn(&str) -> Option<OsString>,
    identifier: &str,
    resolve: impl Fn(&Path) -> Option<PathBuf>,
) -> Vec<(&'static str, OsString)> {
    if !appimage {
        return Vec::new();
    }
    let mut changes = Vec::new();
    if let Some(path) = system_path(var(SYSTEM_PATH_VAR).as_deref(), resolve) {
        changes.push((SYSTEM_PATH_VAR, path));
    }
    if var(REUSE_SCANNER_VAR).as_deref() != Some(OsStr::new("yes")) {
        changes.push((REUSE_SCANNER_VAR, OsString::from("yes")));
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
/// the host's plugins, so it is left alone. `resolve` gives a directory's real path, or `None`
/// when it does not exist; directories with the same real path are added once.
fn system_path(
    current: Option<&OsStr>,
    resolve: impl Fn(&Path) -> Option<PathBuf>,
) -> Option<OsString> {
    let current = current.filter(|path| !path.is_empty())?;
    // The AppImage runtime leaves an empty entry behind ("bundled:"); it names no directory.
    let mut dirs: Vec<PathBuf> = std::env::split_paths(current)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect();
    let before = dirs.len();
    let mut added: Vec<PathBuf> = Vec::new();
    for dir in HOST_PLUGIN_DIRS.map(Path::new) {
        let Some(real) = resolve(dir) else {
            continue;
        };
        if !dirs.iter().any(|known| known == dir) && !added.contains(&real) {
            dirs.push(dir.to_path_buf());
            added.push(real);
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
    fn h264_status_tells_a_host_plugin_that_did_not_load() {
        assert_eq!(h264_status(None, true), H264Status::Unknown);
        assert_eq!(
            h264_status(Some(b"..avdec_h264.."), true),
            H264Status::Available
        );
        assert_eq!(
            h264_status(Some(b"..vp9dec.."), true),
            H264Status::HostPluginNotLoaded
        );
        assert_eq!(
            h264_status(Some(b"..vp9dec.."), false),
            H264Status::HostPluginMissing
        );
    }

    #[test]
    fn changes_nothing_outside_an_appimage() {
        let var = env(&[(SYSTEM_PATH_VAR, BUNDLED), ("HOME", "/h")]);
        assert!(changes(false, &var, "app.id", |dir| Some(dir.to_path_buf())).is_empty());
    }

    #[test]
    fn in_an_appimage_extends_the_path_and_sets_the_registry() {
        let var = env(&[(SYSTEM_PATH_VAR, BUNDLED), ("HOME", "/h")]);
        let changes = changes(true, &var, "app.id", |dir| {
            (dir == Path::new("/usr/lib/gstreamer-1.0")).then(|| dir.to_path_buf())
        });
        let arch = std::env::consts::ARCH;
        assert_eq!(
            changes,
            vec![
                (
                    SYSTEM_PATH_VAR,
                    OsString::from(format!("{BUNDLED}:/usr/lib/gstreamer-1.0"))
                ),
                (REUSE_SCANNER_VAR, OsString::from("yes")),
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
            let changes = changes(true, &var, "app.id", |_| None);
            assert!(changes.iter().all(|(set, _)| !REGISTRY_VARS.contains(set)));
        }
    }

    #[test]
    fn appends_existing_host_dirs_after_the_bundled_ones_in_order() {
        let path = system_path(Some(OsStr::new(BUNDLED)), |dir| {
            (dir != Path::new("/usr/lib/aarch64-linux-gnu/gstreamer-1.0"))
                .then(|| dir.to_path_buf())
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
            (dir == Path::new("/usr/lib/gstreamer-1.0")).then(|| dir.to_path_buf())
        });
        assert_eq!(
            path,
            Some(OsString::from(format!("{BUNDLED}:/usr/lib/gstreamer-1.0")))
        );
    }

    #[test]
    fn skips_missing_host_dirs() {
        let path = system_path(Some(OsStr::new(BUNDLED)), |dir| {
            (dir == Path::new("/usr/lib64/gstreamer-1.0")).then(|| dir.to_path_buf())
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
        assert_eq!(system_path(None, |dir| Some(dir.to_path_buf())), None);
        assert_eq!(
            system_path(Some(OsStr::new("")), |dir| Some(dir.to_path_buf())),
            None
        );
        assert_eq!(system_path(Some(OsStr::new(BUNDLED)), |_| None), None);
    }

    #[test]
    fn does_not_add_a_dir_twice() {
        let current = format!("{BUNDLED}:/usr/lib/gstreamer-1.0");
        let path = system_path(Some(OsStr::new(&current)), |dir| {
            (dir == Path::new("/usr/lib/gstreamer-1.0")).then(|| dir.to_path_buf())
        });
        assert_eq!(path, None);
    }

    #[test]
    fn reuses_one_scanner_process() {
        let hook = env(&[(REUSE_SCANNER_VAR, "no"), ("HOME", "/h")]);
        let changes = changes(true, &hook, "app.id", |_| None);
        assert!(changes.contains(&(REUSE_SCANNER_VAR, OsString::from("yes"))));
        let already = env(&[(REUSE_SCANNER_VAR, "yes"), ("HOME", "/h")]);
        let changes = super::changes(true, &already, "app.id", |_| None);
        assert!(changes.iter().all(|(set, _)| *set != REUSE_SCANNER_VAR));
    }

    #[test]
    fn adds_a_symlinked_host_dir_only_once() {
        // Arch: /usr/lib64 -> /usr/lib.
        let path = system_path(Some(OsStr::new(BUNDLED)), |dir| {
            [
                Path::new("/usr/lib/gstreamer-1.0"),
                Path::new("/usr/lib64/gstreamer-1.0"),
            ]
            .contains(&dir)
            .then(|| PathBuf::from("/usr/lib/gstreamer-1.0"))
        });
        assert_eq!(
            path,
            Some(OsString::from(format!("{BUNDLED}:/usr/lib/gstreamer-1.0")))
        );
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
