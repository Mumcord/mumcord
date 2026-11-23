//! Fetches the ONNX Runtime library for the camera background effects (spec 08, FR-8.33).
//!
//! The segmentation models are bundled in the `bgmatte` crate; ONNX Runtime 1.23.2 (MIT, the
//! official Microsoft release, CPU only) is not, because bgmatte loads it at run time from a
//! library the application provides. It is not committed to the repository either: Microsoft's
//! own package of that release is downloaded once from a pinned URL, checked against its
//! SHA-256 and kept in a shared cache (`MUMCORD_FETCH_CACHE`, or `mumcord-fetch` in the user's
//! cache folder), named by its hash so worktrees and target folders share one copy. A
//! pre-filled cache makes the build work offline.
//!
//! The package is the NuGet one rather than the release's per-platform archives: a GitHub
//! release asset under a pinned URL can disappear (the whole `microsoft/onnxruntime`
//! repository answered 404 for every download on 2026-11-19, which failed the build and its
//! tests on macOS, Windows and Linux at once), while a published NuGet version stays on the
//! feed, unchangeable, for as long as the package exists. It carries every platform's library
//! of this release, so all targets share one download and one hash.
//!
//! The library is copied next to the build's executables for development and test runs, and
//! its cached path is passed to the crate as `MUMCORD_ORT_BUILD_DYLIB` (used by debug builds
//! only, and read by scripts/stage-onnxruntime.js, which copies the library into the
//! installers at one of the places `src/runtime.rs` looks in). A failed download is only a
//! warning: Mumcord then runs with background effects disabled and says why (FR-8.33), which is
//! the same state a user sees when the library is missing.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use zip::ZipArchive;

#[path = "src/artifact_dir.rs"]
mod artifact_dir;

struct Download {
    url: &'static str,
    sha256: &'static str,
    /// File name in the cache (after the hash).
    name: &'static str,
}

/// The one package every target takes its library from.
const PACKAGE: Download = Download {
    url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/1.23.2/microsoft.ml.onnxruntime.1.23.2.nupkg",
    sha256: "25fe172f7fcdf34f2b5b02c8b997f6b88abc282956849d4af4335af23d0c4e4a",
    name: "microsoft.ml.onnxruntime.1.23.2.nupkg",
};

struct Runtime {
    /// Path of the library inside [`PACKAGE`].
    member: &'static str,
    /// File name the library gets next to the executables, which `src/runtime.rs` looks for.
    library: &'static str,
}

/// ONNX Runtime per target (`target_os`, `target_arch`). 1.23.2 is the newest release that
/// still ships an Intel macOS build, which the Intel macOS app needs.
fn runtime_for(os: &str, arch: &str) -> Option<Runtime> {
    let runtime = match (os, arch) {
        ("linux", "x86_64") => Runtime {
            member: "runtimes/linux-x64/native/libonnxruntime.so",
            library: "libonnxruntime.so.1.23.2",
        },
        ("linux", "aarch64") => Runtime {
            member: "runtimes/linux-arm64/native/libonnxruntime.so",
            library: "libonnxruntime.so.1.23.2",
        },
        ("windows", "x86_64") => Runtime {
            member: "runtimes/win-x64/native/onnxruntime.dll",
            library: "onnxruntime.dll",
        },
        ("macos", "aarch64") => Runtime {
            member: "runtimes/osx-arm64/native/libonnxruntime.dylib",
            library: "libonnxruntime.1.23.2.dylib",
        },
        ("macos", "x86_64") => Runtime {
            member: "runtimes/osx-x64/native/libonnxruntime.dylib",
            library: "libonnxruntime.1.23.2.dylib",
        },
        _ => return None,
    };
    Some(runtime)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/artifact_dir.rs");
    println!("cargo:rerun-if-env-changed=MUMCORD_FETCH_CACHE");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let cache = cache_dir();
    if let Err(err) = fs::create_dir_all(&cache) {
        panic!(
            "cannot create the download cache {}: {err}",
            cache.display()
        );
    }

    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let library = match runtime_for(&os, &arch) {
        Some(runtime) => match fetch_runtime(&cache, &runtime, &out_dir) {
            Ok(path) => path.display().to_string(),
            Err(err) => {
                println!(
                    "cargo:warning=ONNX Runtime not fetched ({err}); camera background effects will be unavailable in this build"
                );
                String::new()
            }
        },
        None => {
            println!(
                "cargo:warning=no ONNX Runtime build for {os}/{arch}; camera background effects will be unavailable"
            );
            String::new()
        }
    };
    println!("cargo:rustc-env=MUMCORD_ORT_BUILD_DYLIB={library}");
}

fn cache_dir() -> PathBuf {
    if let Some(dir) = env::var_os("MUMCORD_FETCH_CACHE").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    // The build script runs on the host, so cfg! here describes the host.
    let base = if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
    } else {
        env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    };
    base.unwrap_or_else(env::temp_dir).join("mumcord-fetch")
}

/// Returns the cached, verified file, downloading it first if needed.
fn fetch(cache: &Path, download: &Download) -> Result<PathBuf, String> {
    let path = cache.join(format!("{}-{}", &download.sha256[..16], download.name));
    if path.is_file() && sha256_of(&path)? == download.sha256 {
        return Ok(path);
    }
    let partial = path.with_extension(format!("part{}", std::process::id()));
    let status = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--retry", "3", "--output"])
        .arg(&partial)
        .arg(download.url)
        .status()
        .map_err(|err| format!("cannot run curl to fetch {}: {err}", download.name))?;
    if !status.success() {
        let _ = fs::remove_file(&partial);
        return Err(format!("downloading {} failed ({status})", download.url));
    }
    let actual = sha256_of(&partial)?;
    if actual != download.sha256 {
        let _ = fs::remove_file(&partial);
        return Err(format!(
            "{} has SHA-256 {actual}, expected {}",
            download.url, download.sha256
        ));
    }
    fs::rename(&partial, &path).map_err(|err| format!("cannot store {}: {err}", path.display()))?;
    Ok(path)
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Fetches the runtime package, extracts the library into the cache once, and copies it next
/// to the build's executables. Returns the cached library path.
fn fetch_runtime(cache: &Path, runtime: &Runtime, out_dir: &Path) -> Result<PathBuf, String> {
    let library = cache.join(format!("{}-{}", &PACKAGE.sha256[..16], runtime.library));
    if !library.is_file() {
        let package = fetch(cache, &PACKAGE)?;
        let partial = library.with_extension(format!("part{}", std::process::id()));
        extract(&package, runtime.member, &partial)
            .and_then(|()| make_executable(&partial))
            .and_then(|()| {
                fs::rename(&partial, &library)
                    .map_err(|err| format!("cannot store {}: {err}", library.display()))
            })
            .inspect_err(|_| {
                let _ = fs::remove_file(&partial);
            })?;
    }
    // Executables live in the profile directory. `OUT_DIR` is under `build/` at a depth that
    // changed in Cargo 1.100, so walk up instead of counting parents (see `artifact_dir`).
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    if let Some(exe_dir) = artifact_dir::artifact_dir(out_dir, &profile) {
        let target = exe_dir.join(runtime.library);
        if !target.is_file()
            || fs::metadata(&target).map(|m| m.len()).ok()
                != fs::metadata(&library).map(|m| m.len()).ok()
        {
            fs::copy(&library, &target)
                .map_err(|err| format!("cannot copy the library to {}: {err}", target.display()))?;
        }
    } else {
        println!(
            "cargo:warning=ONNX Runtime library was not copied next to the executables (no {profile} directory above {})",
            out_dir.display()
        );
    }
    Ok(library)
}

/// Writes one member of the package to `into`. The package is a zip, so this needs no `tar` or
/// `unzip` to be installed on the build host.
fn extract(package: &Path, member: &str, into: &Path) -> Result<(), String> {
    let file = fs::File::open(package)
        .map_err(|err| format!("cannot read {}: {err}", package.display()))?;
    let mut archive =
        ZipArchive::new(file).map_err(|err| format!("cannot open {}: {err}", package.display()))?;
    let mut entry = archive
        .by_name(member)
        .map_err(|err| format!("cannot find {member} in {}: {err}", package.display()))?;
    let mut out =
        fs::File::create(into).map_err(|err| format!("cannot write {}: {err}", into.display()))?;
    std::io::copy(&mut entry, &mut out)
        .map_err(|err| format!("cannot write {}: {err}", into.display()))?;
    Ok(())
}

/// The loader maps the library as code, and the installers copy its permissions along; give it
/// what the release's own archives carry.
fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|err| format!("cannot make {} executable: {err}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
