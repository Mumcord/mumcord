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

    #[test]
    fn layer_dir_follows_xdg_data_home() {
        // Only checks the suffix; the env is process-wide, so this does not set it.
        let dir = layer_dir().expect("a data dir");
        assert!(dir.ends_with("vulkan/implicit_layer.d"));
    }
}
