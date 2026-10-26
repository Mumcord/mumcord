//! Install and remove the Vulkan implicit-layer manifest (Linux) that makes games load Mumcord's
//! overlay layer (FR-1.46b). Writing the manifest is all that is needed; the Vulkan loader finds
//! it in `$XDG_DATA_HOME/vulkan/implicit_layer.d` (default `~/.local/share`).
//!
//! The manifest names `MUMCORD_OVERLAY_DISABLE` as its `disable_environment`, so a game started
//! with that variable never loads the layer (the Steam launch-option opt-out). The layer is
//! otherwise always loaded while installed; it then asks Mumcord over IPC whether to draw, and
//! draws nothing when Mumcord is not running or the game is not allowed. So "global overlay off"
//! removes the manifest (nothing is loaded at all), and "on" installs it.

use std::path::{Path, PathBuf};

/// File name of the installed manifest.
pub const MANIFEST_FILE: &str = "mumcord_overlay.json";
/// The layer's name; must match `mumcord_overlay_layer::vulkan::LAYER_NAME`.
pub const LAYER_NAME: &str = "VK_LAYER_MUMCORD_overlay";

/// `$XDG_DATA_HOME/vulkan/implicit_layer.d`, falling back to `~/.local/share`.
pub fn layer_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
        })?;
    Some(base.join("vulkan/implicit_layer.d"))
}

/// Flatpak app id of Steam. Its sandbox sees `$HOME` only partly: the loader inside looks in the
/// app's own `$XDG_DATA_HOME` (`~/.var/app/<id>/data`), not in the host's `~/.local/share`.
const FLATPAK_STEAM_ID: &str = "com.valvesoftware.Steam";

/// The layer manifest folder of Flatpak Steam under `home`, only when that Flatpak has been run
/// (its data folder exists). Mumcord never creates a Flatpak's folders itself. The library copy
/// lands next to it, in the same private data folder, so its path is valid inside the sandbox
/// too. Needs no `flatpak override`.
fn flatpak_steam_layer_dir(home: &Path) -> Option<PathBuf> {
    let data = home.join(".var/app").join(FLATPAK_STEAM_ID).join("data");
    data.is_dir().then(|| data.join("vulkan/implicit_layer.d"))
}

/// All manifest folders to keep in sync: the native one plus Flatpak Steam's, if present.
pub fn layer_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = layer_dir().into_iter().collect();
    let flatpak = std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|home| flatpak_steam_layer_dir(&home));
    dirs.extend(flatpak);
    dirs
}

/// The manifest JSON pointing at the layer library at `library_path`.
pub fn manifest_json(library_path: &Path) -> String {
    // The path is a plain filesystem path; escape backslashes and quotes for JSON just in case.
    let path = library_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        r#"{{
  "file_format_version": "1.2.0",
  "layer": {{
    "name": "{LAYER_NAME}",
    "type": "GLOBAL",
    "library_path": "{path}",
    "api_version": "1.3.0",
    "implementation_version": "1",
    "description": "Mumcord in-game overlay",
    "functions": {{
      "vkNegotiateLoaderLayerInterfaceVersion": "mumcord_vkNegotiateLoaderLayerInterfaceVersion"
    }},
    "disable_environment": {{
      "MUMCORD_OVERLAY_DISABLE": "1"
    }}
  }}
}}
"#
    )
}

/// Writes the manifest into `dir` (created if missing). Returns the manifest path.
pub fn install_into(dir: &Path, library_path: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(MANIFEST_FILE);
    std::fs::write(&path, manifest_json(library_path))?;
    Ok(path)
}

/// Whether the installed manifest already points at `library_path` with current content.
fn is_current(dir: &Path, library_path: &Path) -> bool {
    std::fs::read_to_string(dir.join(MANIFEST_FILE))
        .is_ok_and(|text| text == manifest_json(library_path))
}

/// Folder name of Mumcord's data directory (the Tauri identifier), next to `vulkan/` in the XDG
/// data home.
const APP_DATA_DIR: &str = "app.mumcord.client";

/// The per-version folder the layer library is copied to, derived from the manifest directory
/// `<data home>/vulkan/implicit_layer.d`. Games map the library from here, so replacing or
/// removing Mumcord (or unmounting an AppImage) never pulls a mapped file away from a running game.
pub fn versioned_library_dir(manifest_dir: &Path) -> PathBuf {
    let data_home = manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(manifest_dir);
    data_home
        .join(APP_DATA_DIR)
        .join("overlay-layer")
        .join(env!("CARGO_PKG_VERSION"))
}

/// Copies `library_path` into the per-version folder and returns the copy's path. An existing copy
/// of the same size is kept untouched: a running game may have it mapped. The copy goes through a
/// temporary file and a rename, so a loader never sees a half-written library.
fn persist_library(manifest_dir: &Path, library_path: &Path) -> std::io::Result<PathBuf> {
    let name = library_path
        .file_name()
        .ok_or_else(|| std::io::Error::other("the layer library path has no file name"))?;
    let dir = versioned_library_dir(manifest_dir);
    let target = dir.join(name);
    let wanted = std::fs::metadata(library_path)?.len();
    if std::fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() == wanted) {
        return Ok(target);
    }
    std::fs::create_dir_all(&dir)?;
    let temp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = std::fs::copy(library_path, &temp).and_then(|_| std::fs::rename(&temp, &target));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map(|()| target)
}

/// Name of the stable link next to the per-version folders. It always points at the installed
/// version's folder, so a Steam launch option naming it survives updates and AppImage restarts.
const CURRENT_LINK: &str = "current";

/// The folder holding the per-version folders and the `current` link.
fn layer_root(manifest_dir: &Path) -> PathBuf {
    versioned_library_dir(manifest_dir)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.to_path_buf())
}

/// The stable library path (`.../overlay-layer/current/<library file>`) for the manifest folder
/// `manifest_dir`; this is what `LD_PRELOAD` names for OpenGL games (FR-2.57).
pub fn current_library_path(manifest_dir: &Path, library_name: &std::ffi::OsStr) -> PathBuf {
    layer_root(manifest_dir)
        .join(CURRENT_LINK)
        .join(library_name)
}

/// Points the `current` link at the per-version folder. The link is built under a temporary name
/// and renamed over the old one, so a loader resolving it never finds it missing, and games that
/// already mapped the library keep their file. The target is relative so the link stays valid if
/// the data folder is moved.
#[cfg(unix)]
fn update_current_link(manifest_dir: &Path) -> std::io::Result<()> {
    let root = layer_root(manifest_dir);
    let version = versioned_library_dir(manifest_dir);
    let target = version
        .file_name()
        .ok_or_else(|| std::io::Error::other("the versioned layer folder has no name"))?;
    let link = root.join(CURRENT_LINK);
    if std::fs::read_link(&link).is_ok_and(|existing| existing == Path::new(target)) {
        return Ok(());
    }
    let temp = root.join(format!(".{CURRENT_LINK}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    std::os::unix::fs::symlink(target, &temp)?;
    let result = std::fs::rename(&temp, &link);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(not(unix))]
fn update_current_link(_manifest_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Brings the manifest in `dir` in line with `library_path`: the library is copied to the
/// per-version folder and the manifest rewritten when it is missing or points elsewhere (an
/// AppImage mounts at a new path on every launch). Removed when no library exists so a stale
/// manifest never names a vanished file.
pub fn refresh_in(dir: &Path, library_path: Option<&Path>) -> std::io::Result<()> {
    match library_path {
        Some(lib) => {
            let stored = persist_library(dir, lib)?;
            update_current_link(dir)?;
            if is_current(dir, &stored) {
                Ok(())
            } else {
                install_into(dir, &stored).map(|_| ())
            }
        }
        None => uninstall_from(dir),
    }
}

/// Removes the manifest from `dir` if present. Missing is success.
pub fn uninstall_from(dir: &Path) -> std::io::Result<()> {
    let path = dir.join(MANIFEST_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Whether the manifest is installed in `dir`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_installed_in(dir: &Path) -> bool {
    dir.join(MANIFEST_FILE).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_uninstall_round_trip() {
        let dir = std::env::temp_dir().join(format!("mumcord-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let target = dir.join("implicit_layer.d");
        let lib = Path::new("/opt/mumcord/libmumcord_overlay_layer.so");
        assert!(!is_installed_in(&target));
        let path = install_into(&target, lib).expect("install");
        assert!(is_installed_in(&target));
        let json = std::fs::read_to_string(&path).expect("read");
        assert!(json.contains(LAYER_NAME));
        assert!(json.contains(&lib.to_string_lossy().into_owned()));
        assert!(json.contains("MUMCORD_OVERLAY_DISABLE"));
        // Valid JSON.
        serde_json::from_str::<serde_json::Value>(&json).expect("valid json");
        uninstall_from(&target).expect("uninstall");
        assert!(!is_installed_in(&target));
        // Removing again is fine.
        uninstall_from(&target).expect("uninstall again");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mumcord-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A scratch data home with a fake library; returns (data home, manifest dir, library).
    fn fake_install(name: &str, content: &[u8]) -> (PathBuf, PathBuf, PathBuf) {
        let home = scratch(name);
        let lib_dir = home.join("src");
        std::fs::create_dir_all(&lib_dir).expect("mkdir");
        let lib = lib_dir.join("libx.so");
        std::fs::write(&lib, content).expect("write lib");
        (home.clone(), home.join("vulkan/implicit_layer.d"), lib)
    }

    #[test]
    fn refresh_points_the_manifest_at_a_per_version_copy() {
        let (home, dir, lib) = fake_install("refresh-copy", b"v1");
        refresh_in(&dir, Some(&lib)).expect("refresh");
        let stored = versioned_library_dir(&dir).join("libx.so");
        assert!(stored.starts_with(home.join("app.mumcord.client/overlay-layer")));
        assert_eq!(std::fs::read(&stored).expect("copy"), b"v1");
        let json = std::fs::read_to_string(dir.join(MANIFEST_FILE)).expect("read");
        assert!(json.contains(&stored.to_string_lossy().into_owned()));
        assert!(!json.contains("/src/"));
        // The original can vanish (update, AppImage unmount) without breaking the manifest target.
        std::fs::remove_file(&lib).expect("remove original");
        assert!(stored.is_file());
        std::fs::remove_dir_all(&home).expect("cleanup");
    }

    #[test]
    fn refresh_keeps_an_existing_same_size_copy() {
        let (home, dir, lib) = fake_install("refresh-keep", b"v1");
        refresh_in(&dir, Some(&lib)).expect("refresh");
        let stored = versioned_library_dir(&dir).join("libx.so");
        let before = std::fs::metadata(&stored)
            .expect("meta")
            .modified()
            .expect("mtime");
        refresh_in(&dir, Some(&lib)).expect("refresh again");
        let after = std::fs::metadata(&stored)
            .expect("meta")
            .modified()
            .expect("mtime");
        assert_eq!(before, after);
        assert!(
            !std::fs::read_dir(versioned_library_dir(&dir))
                .expect("ls")
                .any(|e| e
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp"))
        );
        std::fs::remove_dir_all(&home).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn current_link_follows_the_installed_version_and_is_stable() {
        let (_home, dir, lib) = fake_install("current", b"v1");
        refresh_in(&dir, Some(&lib)).expect("test setup");
        let stable = current_library_path(&dir, lib.file_name().expect("test setup"));
        assert_eq!(std::fs::read(&stable).expect("test setup"), b"v1");
        // Same path after a restart from a different mount, and an old link is swapped in place.
        let root = layer_root(&dir);
        std::os::unix::fs::symlink("0.0.0-old", root.join("tmp-old")).expect("test setup");
        std::fs::rename(root.join("tmp-old"), root.join(CURRENT_LINK)).expect("test setup");
        refresh_in(&dir, Some(&lib)).expect("test setup");
        assert_eq!(
            current_library_path(&dir, lib.file_name().expect("test setup")),
            stable
        );
        assert_eq!(std::fs::read(&stable).expect("test setup"), b"v1");
    }

    #[test]
    fn refresh_removes_the_manifest_without_a_library() {
        let dir = scratch("refresh-missing");
        install_into(&dir, Path::new("/gone/libx.so")).expect("install");
        refresh_in(&dir, None).expect("refresh");
        assert!(!is_installed_in(&dir));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn flatpak_steam_dir_only_when_the_flatpak_has_data() {
        let home = scratch("flatpak-home");
        assert_eq!(flatpak_steam_layer_dir(&home), None);
        let data = home.join(".var/app/com.valvesoftware.Steam/data");
        std::fs::create_dir_all(&data).expect("mkdir");
        let dir = flatpak_steam_layer_dir(&home).expect("flatpak dir");
        assert_eq!(dir, data.join("vulkan/implicit_layer.d"));
        // The library copy stays inside the Flatpak's data folder, visible to the sandbox.
        assert!(versioned_library_dir(&dir).starts_with(data.join("app.mumcord.client")));
        std::fs::remove_dir_all(&home).expect("cleanup");
    }

    #[test]
    fn layer_dir_follows_xdg_data_home() {
        // Only checks the suffix; the env is process-wide, so this does not set it.
        let dir = layer_dir().expect("a data dir");
        assert!(dir.ends_with("vulkan/implicit_layer.d"));
    }
}
