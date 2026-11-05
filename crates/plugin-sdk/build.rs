//! Links the prebuilt Bovnar library (`bovnar-wasm/libbovnar_wasm.a`) into plugins built for
//! `wasm32-unknown-unknown` with the 1.1 API, so plugin authors need no C compiler. Natively
//! the pinned `bovnar` crate compiles the same C sources instead.

fn main() {
    println!("cargo:rerun-if-changed=bovnar-wasm/libbovnar_wasm.a");
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32");
    // Cargo keeps the dot of the feature name "1.1" in the variable name.
    let api_1_1 = std::env::var_os("CARGO_FEATURE_1.1").is_some();
    if wasm && api_1_1 {
        let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo"))
            .join("bovnar-wasm");
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-lib=static=bovnar_wasm");
    }
}
