//! Loading ONNX Runtime at run time and the "effects unavailable" fallback (FR-8.33).
//!
//! The library is loaded once per process. When it is missing or fails to load, every
//! background other than None is turned off with the reason from here, and the camera keeps
//! working. No `ort` API may be called before [`availability`] returned `Ok`: `ort` would
//! otherwise try to load the library itself and panic when that fails.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::hooks;

#[cfg(target_os = "windows")]
const LIBRARY: &str = "onnxruntime.dll";
#[cfg(target_os = "macos")]
const LIBRARY: &str = "libonnxruntime.1.23.2.dylib";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const LIBRARY: &str = "libonnxruntime.so.1.23.2";

static RUNTIME: OnceLock<Result<(), String>> = OnceLock::new();

/// Whether background effects can run on this PC; the error is the user-facing reason
/// ("Background effects are not available on this PC: {reason}").
pub fn availability() -> Result<(), String> {
    RUNTIME.get_or_init(load).clone()
}

fn load() -> Result<(), String> {
    if hooks::onnx_unavailable() {
        return Err("ONNX Runtime is turned off for this test run.".to_owned());
    }
    let candidates = candidates();
    let Some(path) = candidates.iter().find(|path| path.is_file()) else {
        tracing::warn!(?candidates, "ONNX Runtime library not found");
        return Err("the ONNX Runtime library is missing.".to_owned());
    };
    match ort::init_from(path) {
        Ok(builder) => {
            // `commit` reports false when an environment existed already, which is fine.
            builder.with_name("mumcord-effects").commit();
            tracing::info!(path = %path.display(), "ONNX Runtime loaded");
            Ok(())
        }
        Err(err) => {
            tracing::warn!(%err, "ONNX Runtime failed to load");
            Err(format!("the ONNX Runtime library failed to load ({err})."))
        }
    }
}

/// Where the library is looked for, in order: the places an installed package carries it
/// (see [`beside_executable`]), and in debug builds the build's download cache.
fn candidates() -> Vec<PathBuf> {
    let mut paths = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(beside_executable))
        .unwrap_or_default();
    let build_copy = env!("MUMCORD_ORT_BUILD_DYLIB");
    if cfg!(debug_assertions) && !build_copy.is_empty() {
        paths.push(PathBuf::from(build_copy));
    }
    paths
}

/// The library's places relative to the executable's folder: next to it (Windows installer and
/// portable zip, development builds), the folder above it (test binaries in `deps/`), the app
/// bundle's Frameworks and Resources folders (macOS), and the `lib/mumcord` folder beside `bin`
/// (Linux AppImage, .deb and .rpm; scripts/stage-onnxruntime.js puts it in all of them).
fn beside_executable(dir: &Path) -> Vec<PathBuf> {
    let mut paths = vec![dir.join(LIBRARY)];
    if let Some(parent) = dir.parent() {
        paths.push(parent.join(LIBRARY));
        if cfg!(target_os = "macos") {
            paths.push(parent.join("Frameworks").join(LIBRARY));
            paths.push(parent.join("Resources").join(LIBRARY));
        } else if cfg!(target_os = "linux") {
            paths.push(parent.join("lib").join(LIBRARY));
            paths.push(parent.join("lib/mumcord").join(LIBRARY));
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The installers put the library where the packaged executable finds it.
    #[test]
    fn finds_the_library_where_the_packages_put_it() {
        #[cfg(target_os = "linux")]
        let (exe_dir, bundled) = ("/usr/bin", "/usr/lib/mumcord/libonnxruntime.so.1.23.2");
        // FreeBSD has no ONNX Runtime build, so background effects stay off there; the search
        // order below is the same as on Linux.
        #[cfg(target_os = "freebsd")]
        let (exe_dir, bundled) = (
            "/usr/local/bin",
            "/usr/local/lib/mumcord/libonnxruntime.so.1.23.2",
        );
        #[cfg(target_os = "macos")]
        let (exe_dir, bundled) = (
            "/Applications/Mumcord.app/Contents/MacOS",
            "/Applications/Mumcord.app/Contents/Frameworks/libonnxruntime.1.23.2.dylib",
        );
        #[cfg(target_os = "windows")]
        let (exe_dir, bundled) = (r"C:\Mumcord", r"C:\Mumcord\onnxruntime.dll");
        assert!(beside_executable(Path::new(exe_dir)).contains(&PathBuf::from(bundled)));
    }
}
