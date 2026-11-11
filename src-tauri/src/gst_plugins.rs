//! Linux packages: GStreamer environment for the bundled H.264/AAC decoders (FR-2.41a).
//!
//! Every Linux package carries Mumcord's own LGPL, decoders-only FFmpeg and the gst-libav plugin
//! built against it (`scripts/build-ffmpeg.sh`), so Twitch and other H.264 videos play without
//! extra distribution packages.
//!
//! **AppImage:** the plugin sits with the other bundled GStreamer plugins. linuxdeploy's AppRun
//! hook points `GST_PLUGIN_PATH_1_0` and `GST_PLUGIN_SYSTEM_PATH_1_0` at them and
//! `GST_PLUGIN_SCANNER_1_0` at the bundled scanner, so only bundled plugins load. The host's
//! plugins stay out: they are built for the host's GStreamer, which may be newer than the
//! bundled one, and then fail to load anyway.
//!
//! **.deb and .rpm:** they use the system's GStreamer, which loads the plugin from
//! `/usr/lib/mumcord/gstreamer-1.0` (its FFmpeg libraries are beside it in `/usr/lib/mumcord`).
//! That directory goes in front of `GST_PLUGIN_PATH`. GStreamer scans `GST_PLUGIN_PATH` before
//! its system path and keeps the first plugin of each file name, so the bundled gst-libav wins
//! over one the distribution installed. A plugin built for an older GStreamer minor version loads
//! into a newer one, so the 1.24 build works on every current distribution.
//!
//! The plugin registry gets its own file: GStreamer's default one is shared with the host's
//! apps, and Mumcord's plugin path would replace their gst-libav entry there (and theirs ours),
//! making every app rescan on each start. A bundled plugin that fails to load is skipped by
//! GStreamer without a message; [`log_h264_status`] reads the registry and logs it.
//!
//! The AppRun hook also turns off reusing the scanner, so every plugin got its own scanner
//! process. As the mount path changes on every start, all bundled plugins are rescanned on every
//! start, and a hundred process starts cost about half a second before the first paint. One
//! reused scanner is still a separate process, so a crashing plugin still cannot take the app
//! down: GStreamer restarts the scanner and skips that plugin.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

const PLUGIN_PATH_VARS: [&str; 2] = ["GST_PLUGIN_PATH_1_0", "GST_PLUGIN_PATH"];
const REGISTRY_VARS: [&str; 2] = ["GST_REGISTRY_1_0", "GST_REGISTRY"];
const REUSE_SCANNER_VAR: &str = "GST_REGISTRY_REUSE_PLUGIN_SCANNER";
/// The bundled plugin directory of the .deb and .rpm, relative to the directory of the binary
/// (`/usr/bin`).
const PACKAGE_PLUGIN_DIR: &str = "../lib/mumcord/gstreamer-1.0";
const PLUGIN_FILE: &str = "libgstlibav.so";

/// How this build gets its GStreamer plugins.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Bundle {
    /// The AppImage, whose AppRun hook already set up the bundled plugins.
    AppImage,
    /// A .deb or .rpm with the bundled plugin in this directory.
    Package(PathBuf),
    /// No bundled decoders (a development or source build): the system's GStreamer as it is.
    None,
}

fn bundle() -> Bundle {
    if crate::desktop_entry::is_appimage() {
        return Bundle::AppImage;
    }
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|bin| bin.join(PACKAGE_PLUGIN_DIR)))
        .and_then(|dir| std::fs::canonicalize(dir).ok());
    match dir {
        Some(dir) if dir.join(PLUGIN_FILE).is_file() => Bundle::Package(dir),
        _ => Bundle::None,
    }
}

/// Sets the plugin path and the registry file. Must run before WebKit or GStreamer start and
/// before any other thread exists; does nothing in a build without bundled decoders.
pub fn prepare(identifier: &str) {
    let var = |name: &str| std::env::var_os(name);
    for (name, value) in changes(&bundle(), &var, identifier) {
        // SAFETY: called at the very start of `run`, before Tauri, GTK or the async runtime
        // start any thread, so nothing reads the environment concurrently.
        unsafe { std::env::set_var(name, value) };
    }
}

/// Whether the bundled H.264 decoder loaded at GStreamer's last scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H264Status {
    /// The decoder is in the registry.
    Available,
    /// The registry lacks it: the bundled gst-libav failed to load.
    NotLoaded,
    /// No registry yet (no video played since the cache was cleared).
    Unknown,
}

/// Logs [`H264Status`] once at start; builds without bundled decoders log nothing.
pub fn log_h264_status(identifier: &str) {
    if bundle() == Bundle::None {
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
        })
        .and_then(|file| std::fs::read(file).ok());
    let status = h264_status(registry.as_deref());
    mumcord_engine::crash::set_context("h264", format!("{status:?}"));
    match status {
        H264Status::NotLoaded => tracing::warn!(
            ?status,
            "the bundled gst-libav did not load into GStreamer; H.264 videos such as Twitch \
             will not play"
        ),
        _ => tracing::info!(?status, "H.264 support"),
    }
}

fn h264_status(registry: Option<&[u8]>) -> H264Status {
    let Some(registry) = registry else {
        return H264Status::Unknown;
    };
    let needle = b"avdec_h264";
    if registry.windows(needle.len()).any(|w| w == needle) {
        H264Status::Available
    } else {
        H264Status::NotLoaded
    }
}

/// The variables to set, read from the environment through `var`.
fn changes(
    bundle: &Bundle,
    var: &dyn Fn(&str) -> Option<OsString>,
    identifier: &str,
) -> Vec<(&'static str, OsString)> {
    let mut changes = Vec::new();
    match bundle {
        Bundle::None => return changes,
        Bundle::AppImage => {
            if var(REUSE_SCANNER_VAR).as_deref() != Some(OsStr::new("yes")) {
                changes.push((REUSE_SCANNER_VAR, OsString::from("yes")));
            }
        }
        Bundle::Package(dir) => {
            if let Some(path) = plugin_path(dir, var) {
                changes.push((PLUGIN_PATH_VARS[0], path));
            }
        }
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

/// `dir` in front of the plugin path the user set, if any. GStreamer reads `GST_PLUGIN_PATH`
/// only when `GST_PLUGIN_PATH_1_0` is unset, so the new `GST_PLUGIN_PATH_1_0` carries it on.
/// `None` when `dir` already comes first.
fn plugin_path(dir: &Path, var: &dyn Fn(&str) -> Option<OsString>) -> Option<OsString> {
    let current = PLUGIN_PATH_VARS.iter().find_map(|name| var(name));
    let mut dirs: Vec<PathBuf> = current
        .as_deref()
        .map(|path| {
            std::env::split_paths(path)
                .filter(|dir| !dir.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default();
    if dirs.first().map(PathBuf::as_path) == Some(dir) {
        return None;
    }
    dirs.retain(|known| known != dir);
    dirs.insert(0, dir.to_path_buf());
    // The bundled directory comes from the binary's path; one with a `:` is left alone.
    std::env::join_paths(dirs).ok()
}

/// Mumcord's own registry file in the user's cache directory (GStreamer creates the folder),
/// named per architecture like GStreamer's default one.
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

    const BUNDLED: &str = "/usr/lib/mumcord/gstreamer-1.0";

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

    fn package() -> Bundle {
        Bundle::Package(PathBuf::from(BUNDLED))
    }

    fn registry(arch_dir: &str) -> OsString {
        let arch = std::env::consts::ARCH;
        OsString::from(format!("{arch_dir}/registry.{arch}.bin"))
    }

    #[test]
    fn h264_status_tells_a_bundled_plugin_that_did_not_load() {
        assert_eq!(h264_status(None), H264Status::Unknown);
        assert_eq!(h264_status(Some(b"..avdec_h264..")), H264Status::Available);
        assert_eq!(h264_status(Some(b"..vp9dec..")), H264Status::NotLoaded);
    }

    #[test]
    fn changes_nothing_without_bundled_decoders() {
        let var = env(&[("HOME", "/h")]);
        assert!(changes(&Bundle::None, &var, "app.id").is_empty());
    }

    #[test]
    fn appimage_reuses_one_scanner_and_sets_the_registry() {
        let var = env(&[(REUSE_SCANNER_VAR, "no"), ("HOME", "/h")]);
        assert_eq!(
            changes(&Bundle::AppImage, &var, "app.id"),
            vec![
                (REUSE_SCANNER_VAR, OsString::from("yes")),
                (
                    "GST_REGISTRY_1_0",
                    registry("/h/.cache/app.id/gstreamer-1.0")
                ),
            ]
        );
        let already = env(&[(REUSE_SCANNER_VAR, "yes"), ("HOME", "/h")]);
        let changes = changes(&Bundle::AppImage, &already, "app.id");
        assert!(changes.iter().all(|(set, _)| *set != REUSE_SCANNER_VAR));
    }

    #[test]
    fn appimage_leaves_the_plugin_path_of_the_hook_alone() {
        let var = env(&[("GST_PLUGIN_PATH_1_0", "/tmp/.mount_x/usr/lib/gstreamer-1.0")]);
        let changes = changes(&Bundle::AppImage, &var, "app.id");
        assert!(
            changes
                .iter()
                .all(|(set, _)| !PLUGIN_PATH_VARS.contains(set))
        );
    }

    #[test]
    fn package_puts_its_plugins_first_and_sets_the_registry() {
        let var = env(&[("XDG_CACHE_HOME", "/c")]);
        assert_eq!(
            changes(&package(), &var, "app.id"),
            vec![
                ("GST_PLUGIN_PATH_1_0", OsString::from(BUNDLED)),
                ("GST_REGISTRY_1_0", registry("/c/app.id/gstreamer-1.0")),
            ]
        );
    }

    #[test]
    fn package_keeps_the_users_plugin_path_after_its_own() {
        for name in PLUGIN_PATH_VARS {
            let var = env(&[(name, "/mine:/also")]);
            assert_eq!(
                plugin_path(Path::new(BUNDLED), &var),
                Some(OsString::from(format!("{BUNDLED}:/mine:/also")))
            );
        }
        // GStreamer ignores GST_PLUGIN_PATH when GST_PLUGIN_PATH_1_0 is set.
        let var = env(&[
            ("GST_PLUGIN_PATH_1_0", "/one"),
            ("GST_PLUGIN_PATH", "/ignored"),
        ]);
        assert_eq!(
            plugin_path(Path::new(BUNDLED), &var),
            Some(OsString::from(format!("{BUNDLED}:/one")))
        );
    }

    #[test]
    fn package_moves_its_dir_to_the_front_once() {
        let first = env(&[("GST_PLUGIN_PATH_1_0", &format!("{BUNDLED}:/mine"))]);
        assert_eq!(plugin_path(Path::new(BUNDLED), &first), None);
        let later = env(&[("GST_PLUGIN_PATH_1_0", &format!("/mine::{BUNDLED}"))]);
        assert_eq!(
            plugin_path(Path::new(BUNDLED), &later),
            Some(OsString::from(format!("{BUNDLED}:/mine")))
        );
    }

    #[test]
    fn keeps_a_registry_the_user_chose() {
        for bundle in [Bundle::AppImage, package()] {
            for name in REGISTRY_VARS {
                let var = env(&[(name, "/mine.bin"), ("HOME", "/h")]);
                let changes = changes(&bundle, &var, "app.id");
                assert!(changes.iter().all(|(set, _)| !REGISTRY_VARS.contains(set)));
            }
        }
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
