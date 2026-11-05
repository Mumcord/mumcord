//! `build`: compiles a plugin project to `wasm32-unknown-unknown`, turns the core module into a
//! component and packs a `.mcplugin`. Plugins target `wasm32-unknown-unknown` instead of
//! `wasm32-wasip2` because the wasip2 std pulls in WASI imports the runtime refuses (lead decision
//! after B25a, recorded as a spec addendum).
//!
//! The manifest's `api` is written from the WIT the component imports (FR-9.6, FR-9.21): the SDK
//! feature picks the API, so authors never type it and it cannot drift from the build.

use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest::Manifest;
use mumcord_plugins::package::{Digest, MANIFEST_FILE, OFFICIAL_FILES, WASM_FILE};
use mumcord_plugins::version::ApiMinor;
use zip::write::SimpleFileOptions;

use crate::check;
use crate::registry::package_file_name;

pub const TARGET: &str = "wasm32-unknown-unknown";

/// Folders that are copied into the package as they are (FR-6.10, `sounds` FR-9.65).
const PACKAGE_DIRS: &[&str] = &["i18n", "assets", "web", "sounds"];

pub struct Built {
    pub path: PathBuf,
    pub sha256: String,
}

pub fn run(dir: &Path, reproducible: bool, out: Option<PathBuf>) -> Result<Built> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("{} is not a folder", dir.display()))?;
    let source_manifest = fs::read(dir.join(MANIFEST_FILE))
        .with_context(|| format!("{} has no {MANIFEST_FILE}", dir.display()))?;
    // Syntax errors show up before the slow compile; `api` may still be missing here.
    serde_json::from_slice::<serde_json::Value>(&source_manifest)
        .context("manifest.json is invalid")?;

    let module = compile(&dir, reproducible)?;
    let component = componentize(&module)?;
    let imports = check::component_imports(&component)?;
    let api = check::imports_api(&imports).context(
        "plugin.wasm imports no Mumcord host API; was it built with mumcord-plugin-sdk?",
    )?;
    let manifest_bytes = with_api(&source_manifest, api)?;
    let manifest = Manifest::parse(&manifest_bytes).context("manifest.json is invalid")?;

    let package = pack(&dir, &manifest_bytes, &component)?;
    let out = out.unwrap_or_else(|| dir.join("dist"));
    fs::create_dir_all(&out)?;
    let path = out.join(package_file_name(
        &manifest.id,
        &manifest.version.to_string(),
    ));
    fs::write(&path, &package)?;
    Ok(Built {
        path,
        sha256: Digest::of(&package).sha256_hex(),
    })
}

fn compile(dir: &Path, reproducible: bool) -> Result<Vec<u8>> {
    let target_dir = dir.join("target");
    let mut cargo = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cargo
        .current_dir(dir)
        .args([
            "build",
            "--release",
            "--lib",
            "--target",
            TARGET,
            "--target-dir",
        ])
        .arg(&target_dir);
    if reproducible {
        if !dir.join("Cargo.lock").exists() {
            bail!(
                "a reproducible build needs Cargo.lock; run `cargo generate-lockfile` and commit it"
            );
        }
        cargo.arg("--locked");
        // Paths of the build machine end up in panic messages; mapping them to fixed names
        // makes two machines produce the same bytes.
        cargo.env("CARGO_ENCODED_RUSTFLAGS", remap_flags(dir));
        cargo.env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1");
        cargo.env("CARGO_PROFILE_RELEASE_DEBUG", "false");
        cargo.env("CARGO_PROFILE_RELEASE_STRIP", "debuginfo");
        cargo.env("CARGO_PROFILE_RELEASE_INCREMENTAL", "false");
    }
    let status = cargo.status().context("could not run cargo")?;
    if !status.success() {
        bail!(
            "cargo build failed (is the {TARGET} target installed? `rustup target add {TARGET}`)"
        );
    }
    let wasm = target_dir
        .join(TARGET)
        .join("release")
        .join(format!("{}.wasm", lib_name(dir)?));
    fs::read(&wasm).with_context(|| format!("cargo produced no {}", wasm.display()))
}

fn remap_flags(dir: &Path) -> OsString {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
    let mut flags = OsString::from("--remap-path-prefix=");
    flags.push(dir);
    flags.push("=/plugin");
    if let Some(home) = cargo_home {
        flags.push("\x1f--remap-path-prefix=");
        flags.push(home);
        flags.push("=/cargo");
    }
    flags
}

/// The file name cargo gives the library: `[lib] name`, else the package name with `-` → `_`.
fn lib_name(dir: &Path) -> Result<String> {
    let text =
        fs::read_to_string(dir.join("Cargo.toml")).context("the project has no Cargo.toml")?;
    let toml: toml::Table = text.parse().context("Cargo.toml is invalid")?;
    let name = toml
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .or_else(|| toml.get("package").and_then(|p| p.get("name")))
        .and_then(|n| n.as_str())
        .context("Cargo.toml has no package name")?;
    Ok(name.replace('-', "_"))
}

/// The SDK embeds the WIT world in the module, so no WIT files are needed here.
pub fn componentize(module: &[u8]) -> Result<Vec<u8>> {
    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(module)
        .context("the module was not built with mumcord-plugin-sdk")?
        .encode()
        .context("could not turn the module into a component")
}

/// The manifest with `"api": "^MAJOR.MINOR"` for the API the component imports. A manifest that
/// already says so keeps its exact bytes; otherwise the value is set and the author told.
fn with_api(manifest: &[u8], api: ApiMinor) -> Result<Vec<u8>> {
    let mut value: serde_json::Value =
        serde_json::from_slice(manifest).context("manifest.json is invalid")?;
    let object = value
        .as_object_mut()
        .context("manifest.json is not a JSON object")?;
    let range = format!("^{api}");
    match object.get("api").and_then(serde_json::Value::as_str) {
        Some(current) if lower_bound(current) == Some(api) => return Ok(manifest.to_vec()),
        Some(current) => {
            println!("note: api {current} replaced by {range}, the API plugin.wasm is built for");
        }
        None => {}
    }
    object.insert("api".to_owned(), range.into());
    let mut bytes = serde_json::to_vec_pretty(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// `^1.1` / `^1.1.0` → 1.1; anything else (e.g. the invalid `^1`) → None.
fn lower_bound(range: &str) -> Option<ApiMinor> {
    let rest = range.strip_prefix('^')?;
    let version = semver::VersionReq::parse(range).ok()?;
    let comparator = version.comparators.first()?;
    (rest.contains('.')).then_some(ApiMinor {
        major: comparator.major,
        minor: comparator.minor?,
    })
}

/// Packs the project files with fixed timestamps, permissions and order, so equal inputs give
/// equal bytes. `manifest` is the manifest with `api` written in.
pub fn pack(dir: &Path, manifest: &[u8], component: &[u8]) -> Result<Vec<u8>> {
    let mut files: Vec<(String, Vec<u8>)> = vec![
        (MANIFEST_FILE.to_owned(), manifest.to_vec()),
        (WASM_FILE.to_owned(), component.to_vec()),
    ];
    for name in OFFICIAL_FILES {
        let path = dir.join(name);
        if path.is_file() {
            files.push(((*name).to_owned(), fs::read(path)?));
        }
    }
    for folder in PACKAGE_DIRS {
        collect(&dir.join(folder), folder, &mut files)?;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in files {
        zip.start_file(name, options)?;
        zip.write_all(&body)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn collect(path: &Path, name: &str, files: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name = file_name
            .to_str()
            .with_context(|| format!("{} has a file name that is not UTF-8", path.display()))?;
        let child = format!("{name}/{file_name}");
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("{child} is a symbolic link; packages cannot contain links");
        } else if kind.is_dir() {
            collect(&entry.path(), &child, files)?;
        } else {
            files.push((child, fs::read(entry.path())?));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::tests::manifest;

    #[test]
    fn packing_is_deterministic_and_complete() {
        let dir = tempfile::tempdir().expect("test step");
        let root = dir.path();
        fs::write(root.join(MANIFEST_FILE), manifest("demo", "0.1.0", &[])).expect("test step");
        fs::create_dir_all(root.join("i18n")).expect("test step");
        fs::write(root.join("i18n/en.json"), "{}").expect("test step");
        fs::create_dir_all(root.join("assets/icons")).expect("test step");
        fs::write(root.join("assets/icons/a.svg"), "<svg/>").expect("test step");
        fs::write(root.join("README.md"), "# Demo").expect("test step");
        fs::write(root.join("notes.txt"), "not packed").expect("test step");
        fs::create_dir_all(root.join("sounds")).expect("test step");
        fs::write(root.join("sounds/ding.ogg"), "OggS").expect("test step");
        let manifest = fs::read(root.join(MANIFEST_FILE)).expect("test step");

        let first = pack(root, &manifest, b"wasm").expect("test step");
        let second = pack(root, &manifest, b"wasm").expect("test step");
        assert_eq!(first, second);

        let archive = zip::ZipArchive::new(Cursor::new(first)).expect("test step");
        let names: Vec<String> = archive
            .file_names()
            .map(|n| n.expect("utf-8").into_owned())
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names.len(), 6);
        assert_eq!(
            sorted,
            [
                "README.md",
                "assets/icons/a.svg",
                "i18n/en.json",
                "manifest.json",
                "plugin.wasm",
                "sounds/ding.ogg"
            ]
        );
    }

    #[test]
    fn api_is_written_from_the_imports() {
        let api_of = |bytes: &[u8]| {
            let value: serde_json::Value = serde_json::from_slice(bytes).expect("json");
            value["api"].as_str().map(str::to_owned)
        };
        let mut missing: serde_json::Value =
            serde_json::from_str(&manifest("demo", "0.1.0", &[])).expect("json");
        missing.as_object_mut().expect("object").remove("api");
        let missing = serde_json::to_vec(&missing).expect("json");
        let written = with_api(&missing, ApiMinor::V1_1).expect("test step");
        assert_eq!(api_of(&written).as_deref(), Some("^1.1"));
        Manifest::parse(&written).expect("a valid manifest");

        let matching = manifest("demo", "0.1.0", &[]).replace("^1.0", "^1.0.0");
        let kept = with_api(matching.as_bytes(), ApiMinor::V1_0).expect("test step");
        assert_eq!(kept, matching.as_bytes(), "a matching api keeps the bytes");

        let stale = manifest("demo", "0.1.0", &[]);
        let fixed = with_api(stale.as_bytes(), ApiMinor::V1_1).expect("test step");
        assert_eq!(api_of(&fixed).as_deref(), Some("^1.1"));
        let invalid = stale.replace("^1.0", "^1");
        let fixed = with_api(invalid.as_bytes(), ApiMinor::V1_0).expect("test step");
        assert_eq!(api_of(&fixed).as_deref(), Some("^1.0"));
    }

    #[test]
    fn lib_name_follows_cargo() {
        let dir = tempfile::tempdir().expect("test step");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-plugin\"\n",
        )
        .expect("test step");
        assert_eq!(lib_name(dir.path()).expect("test step"), "my_plugin");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"a\"\n[lib]\nname = \"b_c\"\n",
        )
        .expect("test step");
        assert_eq!(lib_name(dir.path()).expect("test step"), "b_c");
    }
}
