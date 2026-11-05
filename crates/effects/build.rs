//! Fetches the segmentation models and the ONNX Runtime library (spec 08, FR-8.33).
//!
//! Neither is committed to the repository. Each file is downloaded once from a pinned URL,
//! checked against its SHA-256 and kept in a shared cache (`MUMCORD_FETCH_CACHE`, or
//! `mumcord-fetch` in the user's cache folder), named by its hash so worktrees and target
//! folders share one copy. A pre-filled cache makes the build work offline.
//!
//! - The two MediaPipe Selfie Segmenter models (Apache-2.0, ONNX conversions published by
//!   onnx-community on Hugging Face, pinned to a commit) are embedded into the binary with
//!   `include_bytes!`, so a model can never be missing at run time. A failed download fails the
//!   build.
//! - ONNX Runtime 1.23.2 (MIT, the official Microsoft release, CPU only) is loaded at run time.
//!   The library is copied next to the build's executables for development and test runs, and
//!   its cached path is passed to the crate as `MUMCORD_ORT_BUILD_DYLIB` (used by debug builds
//!   only). Installers have to carry the library in one of the places `src/runtime.rs` looks
//!   in; the copy next to the release executable is the one to bundle. A failed
//!   download is only a warning: Mumcord then runs with background effects disabled and says
//!   why (FR-8.33), which is the same state a user sees when the library is missing.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

struct Download {
    url: &'static str,
    sha256: &'static str,
    /// File name in the cache (after the hash) and, for models, in `OUT_DIR`.
    name: &'static str,
}

const MODELS: [Download; 2] = [
    Download {
        url: "https://huggingface.co/onnx-community/mediapipe_selfie_segmentation/resolve/be49485c8e027524be38591817fc5cd31bd9d00e/onnx/model.onnx",
        sha256: "3241ac4ad8aa35bdaf33946776db29f7c283a413aa0b0dacb9483594b4531aad",
        name: "selfie_square.onnx",
    },
    Download {
        url: "https://huggingface.co/onnx-community/mediapipe_selfie_segmentation_landscape/resolve/2497d5bec26c626c7b3c4edc6e1fefc21b64f6c3/onnx/model.onnx",
        sha256: "7a0adcfdb1715d3b0ff0f61486d8a181c33f4a208343a5abfccbc6540e872d24",
        name: "selfie_landscape.onnx",
    },
];

struct Runtime {
    archive: Download,
    /// Path of the library inside the archive.
    member: &'static str,
    /// File name the library gets next to the executables.
    library: &'static str,
}

/// ONNX Runtime per target (`target_os`, `target_arch`). 1.23.2 is the newest release that
/// still ships an Intel macOS build, which the universal macOS app needs.
fn runtime_for(os: &str, arch: &str) -> Option<Runtime> {
    let runtime = match (os, arch) {
        ("linux", "x86_64") => Runtime {
            archive: Download {
                url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-linux-x64-1.23.2.tgz",
                sha256: "1fa4dcaef22f6f7d5cd81b28c2800414350c10116f5fdd46a2160082551c5f9b",
                name: "onnxruntime-linux-x64-1.23.2.tgz",
            },
            member: "onnxruntime-linux-x64-1.23.2/lib/libonnxruntime.so.1.23.2",
            library: "libonnxruntime.so.1.23.2",
        },
        ("windows", "x86_64") => Runtime {
            archive: Download {
                url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-win-x64-1.23.2.zip",
                sha256: "0b38df9af21834e41e73d602d90db5cb06dbd1ca618948b8f1d66d607ac9f3cd",
                name: "onnxruntime-win-x64-1.23.2.zip",
            },
            member: "onnxruntime-win-x64-1.23.2/lib/onnxruntime.dll",
            library: "onnxruntime.dll",
        },
        ("macos", "aarch64") => Runtime {
            archive: Download {
                url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-osx-arm64-1.23.2.tgz",
                sha256: "b4d513ab2b26f088c66891dbbc1408166708773d7cc4163de7bdca0e9bbb7856",
                name: "onnxruntime-osx-arm64-1.23.2.tgz",
            },
            member: "./onnxruntime-osx-arm64-1.23.2/lib/libonnxruntime.1.23.2.dylib",
            library: "libonnxruntime.1.23.2.dylib",
        },
        ("macos", "x86_64") => Runtime {
            archive: Download {
                url: "https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-osx-x86_64-1.23.2.tgz",
                sha256: "d10359e16347b57d9959f7e80a225a5b4a66ed7d7e007274a15cae86836485a6",
                name: "onnxruntime-osx-x86_64-1.23.2.tgz",
            },
            member: "./onnxruntime-osx-x86_64-1.23.2/lib/libonnxruntime.1.23.2.dylib",
            library: "libonnxruntime.1.23.2.dylib",
        },
        _ => return None,
    };
    Some(runtime)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=MUMCORD_FETCH_CACHE");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let cache = cache_dir();
    if let Err(err) = fs::create_dir_all(&cache) {
        panic!(
            "cannot create the download cache {}: {err}",
            cache.display()
        );
    }

    for model in &MODELS {
        let path = fetch(&cache, model).unwrap_or_else(|err| panic!("{err}"));
        fs::copy(&path, out_dir.join(model.name))
            .unwrap_or_else(|err| panic!("cannot copy {}: {err}", model.name));
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

/// Fetches the runtime archive, extracts the library into the cache once, and copies it next
/// to the build's executables. Returns the cached library path.
fn fetch_runtime(cache: &Path, runtime: &Runtime, out_dir: &Path) -> Result<PathBuf, String> {
    let library = cache.join(format!(
        "{}-{}",
        &runtime.archive.sha256[..16],
        runtime.library
    ));
    if !library.is_file() {
        let archive = fetch(cache, &runtime.archive)?;
        let scratch = out_dir.join("ort-extract");
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).map_err(|err| format!("cannot create {err}"))?;
        extract(&archive, runtime.member, &scratch)?;
        let extracted = scratch.join(runtime.member);
        let partial = library.with_extension(format!("part{}", std::process::id()));
        fs::copy(&extracted, &partial)
            .and_then(|_| fs::rename(&partial, &library))
            .map_err(|err| format!("cannot store {}: {err}", library.display()))?;
        let _ = fs::remove_dir_all(&scratch);
    }
    // OUT_DIR is target/<profile>/build/<crate>-<hash>/out; executables live in target/<profile>.
    if let Some(exe_dir) = out_dir.ancestors().nth(3) {
        let target = exe_dir.join(runtime.library);
        if !target.is_file()
            || fs::metadata(&target).map(|m| m.len()).ok()
                != fs::metadata(&library).map(|m| m.len()).ok()
        {
            fs::copy(&library, &target)
                .map_err(|err| format!("cannot copy the library to {}: {err}", target.display()))?;
        }
    }
    Ok(library)
}

/// Extracts one member with the system `tar` (bsdtar on Windows and macOS reads zip too);
/// `unzip` covers zip files on hosts whose `tar` is GNU tar.
fn extract(archive: &Path, member: &str, into: &Path) -> Result<(), String> {
    let is_zip = archive.extension().is_some_and(|ext| ext == "zip");
    let tar = Command::new("tar")
        .arg(if is_zip { "-xf" } else { "-xzf" })
        .arg(archive)
        .arg("-C")
        .arg(into)
        .arg(member)
        .output();
    if tar.as_ref().is_ok_and(|out| out.status.success()) {
        return Ok(());
    }
    if is_zip {
        let unzip = Command::new("unzip")
            .args(["-q", "-o"])
            .arg(archive)
            .arg(member)
            .arg("-d")
            .arg(into)
            .status();
        if unzip.is_ok_and(|status| status.success()) {
            return Ok(());
        }
    }
    Err(format!(
        "cannot extract {member} from {}",
        archive.display()
    ))
}
