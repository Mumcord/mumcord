//! Links libvpx (VP9), SVT-AV1 (AV1 encode) and dav1d (AV1 decode) and generates their bindings.
//!
//! Each library is either a static build from its `scripts/build-*.sh` script, pointed at by an
//! environment variable (CI and release builds), or the system library found with pkg-config
//! (Linux development):
//!
//! | Library | Variable            | Static prefix contents                                      |
//! |---------|---------------------|-------------------------------------------------------------|
//! | libvpx  | `LIBVPX_STATIC_DIR` | `include/vpx/`, `lib/vpx.lib` (MSVC) or `lib/libvpx.a`      |
//! | SVT-AV1 | `SVTAV1_STATIC_DIR` | `include/svt-av1/`, `lib/SvtAv1Enc.lib` or `lib/libSvtAv1Enc.a` |
//! | dav1d   | `DAV1D_STATIC_DIR`  | `include/dav1d/`, `lib/dav1d.lib` or `lib/libdav1d.a`       |
//!
//! The bindings are generated from the headers of exactly the library that is linked, because
//! the libvpx ABI versions are checked at runtime and SVT-AV1's configuration struct changes
//! between releases.
//!
//! The direct hardware encoders need `HWENC_SDK_DIR` (Windows and Linux, from
//! `scripts/build-hwenc-sdk.sh`): bindings for the NVENC, AMF, oneVPL and (Linux) VA-API headers,
//! plus the static oneVPL dispatcher and the C++ runtime it needs. The GPU drivers themselves are
//! loaded at runtime, so the app starts without them. Without the variable the hardware encoders
//! are compiled out (`cfg(hwenc)` is unset) and screen sharing uses the software encoders only.

use std::env;
use std::path::{Path, PathBuf};

struct Library {
    name: &'static str,
    env: &'static str,
    /// The library name for `rustc-link-lib` and its archive file name on MSVC and elsewhere.
    link: &'static str,
    msvc_archive: &'static str,
    archive: &'static str,
    /// A header that must exist below `include/` in a static prefix.
    header: &'static str,
    pkg_config: &'static str,
    min_version: &'static str,
    packages: &'static str,
    script: &'static str,
}

const VPX: Library = Library {
    name: "libvpx",
    env: "LIBVPX_STATIC_DIR",
    link: "vpx",
    msvc_archive: "vpx.lib",
    archive: "libvpx.a",
    header: "vpx/vpx_encoder.h",
    pkg_config: "vpx",
    min_version: "1.8",
    packages: "Debian/Ubuntu: libvpx-dev, Fedora: libvpx-devel, Arch: libvpx",
    script: "scripts/build-libvpx.sh",
};

// SVT-AV1 4 is the first release with the per-frame rate, frame-rate and layer-count events the
// encoder relies on for live changes.
const SVT_AV1: Library = Library {
    name: "SVT-AV1",
    env: "SVTAV1_STATIC_DIR",
    link: "SvtAv1Enc",
    msvc_archive: "SvtAv1Enc.lib",
    archive: "libSvtAv1Enc.a",
    header: "svt-av1/EbSvtAv1Enc.h",
    pkg_config: "SvtAv1Enc",
    min_version: "4.0",
    packages: "Fedora: svt-av1-devel, Arch: svt-av1",
    script: "scripts/build-svt-av1.sh",
};

const DAV1D: Library = Library {
    name: "dav1d",
    env: "DAV1D_STATIC_DIR",
    link: "dav1d",
    msvc_archive: "dav1d.lib",
    archive: "libdav1d.a",
    header: "dav1d/dav1d.h",
    pkg_config: "dav1d",
    min_version: "1.0",
    packages: "Debian/Ubuntu: libdav1d-dev, Fedora: libdav1d-devel, Arch: dav1d",
    script: "scripts/build-dav1d.sh",
};

fn main() {
    let vpx = find(&VPX);
    generate(
        &vpx,
        bindgen::Builder::default()
            .header("wrapper.h")
            .allowlist_function("vpx_.*")
            .allowlist_type("vpx_.*|vp8e_.*|vp9e_.*|vp8_.*")
            .allowlist_var("VPX_.*|VP8_.*|VP8E_.*|VP9E_.*"),
        "vpx.rs",
    );
    println!("cargo:rerun-if-changed=wrapper.h");

    let svt = find(&SVT_AV1);
    generate(
        &svt,
        bindgen::Builder::default()
            // stdio.h for vsnprintf, which formats SVT-AV1's log messages (see av1_svt.rs).
            .header_contents(
                "svt.h",
                "#include <stdio.h>\n#include <svt-av1/EbSvtAv1Enc.h>\n",
            )
            .allowlist_function("svt_av1_.*|vsnprintf")
            .allowlist_type("Eb.*|SvtAv1.*|PrivDataType|PredStructure")
            .allowlist_var("EB_.*"),
        "svt.rs",
    );
    // Modern MSVC defines vsnprintf inline in its headers; this library exports it as a function.
    if env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc") {
        println!("cargo:rustc-link-lib=dylib=legacy_stdio_definitions");
    }

    let dav1d = find(&DAV1D);
    generate(
        &dav1d,
        bindgen::Builder::default()
            // errno.h for EAGAIN, which dav1d returns negated and whose value differs per OS.
            .header_contents("dav1d.h", "#include <errno.h>\n#include <dav1d/dav1d.h>\n")
            .allowlist_function("dav1d_.*")
            .allowlist_type("Dav1d.*")
            .allowlist_var("DAV1D_.*|EAGAIN"),
        "dav1d.rs",
    );

    hardware_encoders();
}

/// Generates the hardware encoder bindings and links the oneVPL dispatcher when `HWENC_SDK_DIR`
/// is set; see the module documentation.
fn hardware_encoders() {
    println!("cargo::rustc-check-cfg=cfg(hwenc)");
    println!("cargo:rerun-if-env-changed=HWENC_SDK_DIR");
    let Some(dir) = env::var_os("HWENC_SDK_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let os = env::var("CARGO_CFG_TARGET_OS").expect("cargo sets CARGO_CFG_TARGET_OS");
    assert!(
        os == "windows" || os == "linux",
        "HWENC_SDK_DIR is set, but {os} has no supported hardware encoder"
    );
    let include = dir.join("include");
    let lib_dir = dir.join("lib");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    let vpl = lib_dir.join(if msvc { "vpl.lib" } else { "libvpl.a" });
    let mut required = vec![
        include.join("ffnvcodec/nvEncodeAPI.h"),
        include.join("AMF/components/VideoEncoderAV1.h"),
        include.join("vpl/mfx.h"),
        vpl.clone(),
    ];
    if os == "linux" {
        required.push(include.join("va/va_enc_av1.h"));
    }
    for path in &required {
        assert!(
            path.is_file(),
            "HWENC_SDK_DIR={}: {} is missing (build it with scripts/build-hwenc-sdk.sh)",
            dir.display(),
            path.display()
        );
    }

    println!("cargo:rerun-if-changed={}", vpl.display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=vpl");
    // The dispatcher is C++ and loads Intel's runtime itself. MSVC: the archive was built against
    // the dynamic C runtime (/MD), like Rust's default, so it needs the matching C++ library.
    if msvc {
        println!("cargo:rustc-link-lib=dylib=msvcprt");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=dl");
    }
    println!("cargo:rustc-cfg=hwenc");

    let include = [include];
    generate(
        &include,
        bindgen::Builder::default()
            .header_contents("nvenc.h", "#include <ffnvcodec/nvEncodeAPI.h>\n")
            // The driver library is loaded at runtime: its two entry points become a loader struct.
            .dynamic_library_name("NvEncodeApi")
            .allowlist_function("NvEncodeAPICreateInstance|NvEncodeAPIGetMaxSupportedVersion")
            .allowlist_type("_?NV_ENC.*|_?NVENC.*|_?NV_ENCODE.*|PNV.*|GUID|_GUID")
            .allowlist_var("NV_ENC.*|NVENC.*|NV_MAX.*"),
        "nvenc.rs",
    );
    generate(
        &include,
        bindgen::Builder::default()
            .header_contents(
                "amf.h",
                "#include <AMF/core/Factory.h>\n#include <AMF/components/VideoEncoderAV1.h>\n",
            )
            .allowlist_type("AMF.*|amf_.*")
            .allowlist_var("AMF_.*"),
        "amf.rs",
    );
    generate(
        &include,
        bindgen::Builder::default()
            .header_contents("vpl.h", "#include <vpl/mfx.h>\n")
            .allowlist_function("MFX.*")
            .allowlist_type("mfx.*|MFX.*")
            .allowlist_var("MFX_.*"),
        "vpl.rs",
    );
    if os == "linux" {
        // libva and libva-drm are loaded at runtime, each into its own loader struct.
        generate(
            &include,
            bindgen::Builder::default()
                .header_contents(
                    "va.h",
                    "#include <va/va.h>\n#include <va/va_enc_av1.h>\n#include <va/va_enc_vp9.h>\n",
                )
                .dynamic_library_name("LibVa")
                .allowlist_function("va.*")
                .allowlist_type("VA.*|_VA.*")
                .allowlist_var("VA_.*"),
            "va.rs",
        );
        generate(
            &include,
            bindgen::Builder::default()
                .header_contents("va_drm.h", "#include <va/va_drm.h>\n")
                .dynamic_library_name("LibVaDrm")
                .allowlist_function("vaGetDisplayDRM")
                .allowlist_recursively(false)
                .raw_line("use super::va::VADisplay;"),
            "va_drm.rs",
        );
    }
}

/// Links `lib` (static prefix or pkg-config) and returns its include paths.
fn find(lib: &Library) -> Vec<PathBuf> {
    println!("cargo:rerun-if-env-changed={}", lib.env);
    if let Some(dir) = env::var_os(lib.env) {
        return link_static(lib, Path::new(&dir));
    }
    pkg_config::Config::new()
        .atleast_version(lib.min_version)
        .probe(lib.pkg_config)
        .unwrap_or_else(|err| {
            panic!(
                "{} {}+ not found via pkg-config ({err}). Install the development package ({}), \
                 or set {} to a static build ({}).",
                lib.name, lib.min_version, lib.packages, lib.env, lib.script
            )
        })
        .include_paths
}

/// Links the static library in `dir` and returns its include path.
fn link_static(lib: &Library, dir: &Path) -> Vec<PathBuf> {
    let include = dir.join("include");
    let lib_dir = dir.join("lib");
    // MSVC names static libraries `name.lib`, every other toolchain `libname.a`.
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    let archive = lib_dir.join(if msvc { lib.msvc_archive } else { lib.archive });
    for required in [&include.join(lib.header), &archive] {
        assert!(
            required.is_file(),
            "{}={}: {} is missing (build it with {})",
            lib.env,
            dir.display(),
            required.display(),
            lib.script
        );
    }
    // A rebuilt library at the same path (CI cache refresh) must relink.
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static={}", lib.link);
    vec![include]
}

fn generate(include_paths: &[PathBuf], builder: bindgen::Builder, file: &str) {
    let mut builder = builder
        .prepend_enum_name(false)
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    for path in include_paths {
        builder = builder.clang_arg(format!("-I{}", path.display()));
    }
    let bindings = builder
        .generate()
        .unwrap_or_else(|err| panic!("generating {file} failed (libclang missing?): {err}"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join(file);
    bindings
        .write_to_file(&out)
        .unwrap_or_else(|err| panic!("writing {}: {err}", out.display()));
}
