//! Links libvpx and generates its bindings.
//!
//! On Linux libvpx is found with pkg-config (the system library). Windows and macOS link a static
//! libvpx built by `scripts/build-libvpx.sh`: `LIBVPX_STATIC_DIR` points at its prefix with
//! `include/vpx/` and `lib/vpx.lib` (MSVC) or `lib/libvpx.a`.
//! The bindings are generated from the headers of exactly that library, because the encoder and
//! decoder ABI versions are checked at runtime and differ between libvpx releases.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=LIBVPX_STATIC_DIR");

    let include_paths =
        match env::var_os("LIBVPX_STATIC_DIR") {
            Some(dir) => link_static(Path::new(&dir)),
            None => pkg_config::Config::new()
                .atleast_version("1.8")
                .probe("vpx")
                .unwrap_or_else(|err| {
                    panic!(
                        "libvpx not found via pkg-config ({err}). Install the libvpx development \
                         package (Debian/Ubuntu: libvpx-dev, Fedora: libvpx-devel, Arch: libvpx), \
                         or set LIBVPX_STATIC_DIR to a static build (scripts/build-libvpx.sh)."
                    )
                })
                .include_paths,
        };

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .allowlist_function("vpx_.*")
        .allowlist_type("vpx_.*|vp8e_.*|vp9e_.*|vp8_.*")
        .allowlist_var("VPX_.*|VP8_.*|VP8E_.*|VP9E_.*")
        .prepend_enum_name(false)
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    for path in &include_paths {
        builder = builder.clang_arg(format!("-I{}", path.display()));
    }
    let bindings = builder.generate().unwrap_or_else(|err| {
        panic!("generating libvpx bindings failed (libclang missing?): {err}")
    });

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("vpx.rs");
    bindings
        .write_to_file(&out)
        .unwrap_or_else(|err| panic!("writing {}: {err}", out.display()));
}

/// Links the static libvpx in `dir` and returns its include path.
fn link_static(dir: &Path) -> Vec<PathBuf> {
    let include = dir.join("include");
    let lib_dir = dir.join("lib");
    // MSVC names static libraries `name.lib`, every other toolchain `libname.a`.
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    let lib = lib_dir.join(if msvc { "vpx.lib" } else { "libvpx.a" });
    for required in [&include.join("vpx").join("vpx_encoder.h"), &lib] {
        assert!(
            required.is_file(),
            "LIBVPX_STATIC_DIR={}: {} is missing (build it with scripts/build-libvpx.sh)",
            dir.display(),
            required.display()
        );
    }
    // A rebuilt library at the same path (CI cache refresh) must relink.
    println!("cargo:rerun-if-changed={}", lib.display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=vpx");
    vec![include]
}
