#[path = "build/version.rs"]
mod version;

use std::path::Path;
use std::process::Command;

fn main() {
    set_display_version();

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    if target_os == "windows" && target_env == "msvc" {
        // tauri-build embeds the Common Controls v6 manifest only into binaries, so the
        // lib unit-test executable starts without it and Windows aborts it with
        // STATUS_ENTRYPOINT_NOT_FOUND. Embedding the manifest through linker args instead
        // covers every linked target (app, tests, cdylib); tauri-build's own copy is
        // disabled to avoid a duplicate manifest resource in the app binary.
        let manifest = std::path::Path::new(
            &std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        )
        .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());

        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("failed to run tauri-build");
    } else {
        tauri_build::build()
    }
}

/// Exposes the display version (see `build/version.rs`) to the app as `MUMCORD_DISPLAY_VERSION`.
fn set_display_version() {
    println!("cargo:rerun-if-env-changed=MUMCORD_VERSION");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let override_version = std::env::var("MUMCORD_VERSION").ok();
    let dirty = if override_version
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty())
    {
        None
    } else {
        git_dirty(Path::new(&manifest_dir))
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let display =
        version::display_version(override_version.as_deref(), dirty, version::civil_date(now));
    println!("cargo:rustc-env=MUMCORD_DISPLAY_VERSION={display}");
}

/// Whether the work tree has uncommitted changes; `None` without git or outside a repository,
/// so tarball builds still work. Reruns the build script whenever HEAD moves (checkout, commit,
/// reset). Watching the index instead would rebuild the app on every `git status`, so the date
/// and the dirty mark can lag behind plain file edits until the next commit or clean build.
fn git_dirty(dir: &Path) -> Option<bool> {
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    };
    for file in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", file]) {
            println!("cargo:rerun-if-changed={}", path.trim());
        }
    }
    // --no-optional-locks: a status run must not rewrite the index.
    git(&["--no-optional-locks", "status", "--porcelain"]).map(|out| version::is_dirty(&out))
}
