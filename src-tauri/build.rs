fn main() {
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
