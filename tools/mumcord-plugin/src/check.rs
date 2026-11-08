//! `check`: runs the install checks of the client on a package (FR-6.10 … FR-6.12; permissions,
//! `min_api`, origins and sounds per FR-9.23 … FR-9.26, FR-9.65) plus a look at the component's
//! imports, so authors see refusals before publishing: `api` must name the API the component
//! imports (FR-9.21), and a deprecated API gets a warning (FR-9.11).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest::Source;
use mumcord_plugins::package::{Package, WASM_FILE};
use mumcord_plugins::version::{self, ApiMinor};
use wit_parser::{WorldItem, decoding::DecodedWasm};

use crate::sign;

/// Host API major the client provides; imports of another major are refused at load.
const API_MAJOR: u64 = 1;
const API_PREFIX: &str = "mumcord:plugin/";

pub fn run(path: &Path, official: bool, key: Option<&str>) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let source = if official {
        Source::Official
    } else {
        // Third-party rules; the repository id does not change any check.
        Source::Repo { repository_id: 0 }
    };
    let package = Package::read(&bytes, source).context("the package would be refused")?;
    let component = package
        .file(WASM_FILE)
        .context("the package has no plugin.wasm")?;
    let imports = component_imports(component)?;
    let manifest = &package.manifest;
    let api = version::api_of(manifest);
    api_matches(api, imports_api(&imports))?;
    if let Some(warning) = deprecation_warning(api, version::deprecated(api)) {
        println!("{warning}");
    }
    // A 1.2 package with such files was refused above; older ones only get a warning (FR-13.29).
    if let Some(file) = package.reserved_web_files().next() {
        println!("{}", reserved_warning(file));
    }
    if let Some(key) = key {
        let public = sign::read_public(key)?;
        let signature_path = sign::signature_path(path);
        let signature = fs::read_to_string(&signature_path)
            .with_context(|| format!("could not read {}", signature_path.display()))?;
        sign::verify(&public, &bytes, &signature).context("author signature check failed")?;
    }
    println!(
        "ok {} {} sha256 {}",
        manifest.id,
        manifest.version,
        package.digest.sha256_hex()
    );
    Ok(())
}

/// Refuses components that import anything but the Mumcord host API (no WASI, FR-6.4).
pub fn component_imports(component: &[u8]) -> Result<Vec<String>> {
    let decoded =
        wit_parser::decoding::decode(component).context("plugin.wasm is not a component")?;
    let DecodedWasm::Component(resolve, world) = decoded else {
        bail!("plugin.wasm is a WIT package, not a component");
    };
    let mut names = Vec::new();
    for (key, item) in &resolve.worlds[world].imports {
        let name = match item {
            // Types the exports use (e.g. start-info) show up as world-level type imports.
            WorldItem::Type { .. } => continue,
            WorldItem::Interface { id, .. } => resolve.id_of(*id),
            WorldItem::Function(_) => None,
        }
        .unwrap_or_else(|| resolve.name_world_key(key));
        if !import_allowed(&name) {
            bail!("plugin.wasm imports {name}; Mumcord provides only {API_PREFIX}*@{API_MAJOR}.x");
        }
        names.push(name);
    }
    Ok(names)
}

/// The API a component is built against: the newest `mumcord:plugin` version among its imports,
/// the same rule the client links by (`version::imports_api`).
pub fn imports_api(imports: &[String]) -> Option<ApiMinor> {
    imports
        .iter()
        .filter_map(|name| name.strip_prefix(API_PREFIX)?.split_once('@'))
        .filter_map(|(_, version)| semver::Version::parse(version).ok())
        .map(|v| ApiMinor {
            major: v.major,
            minor: v.minor,
        })
        .max()
}

/// The client refuses to load a package whose `api` differs from its imports (FR-9.8, FR-9.21).
fn api_matches(api: ApiMinor, imported: Option<ApiMinor>) -> Result<()> {
    match imported {
        Some(imported) if imported == api => Ok(()),
        Some(imported) => bail!(
            "manifest.json says api ^{api}, but plugin.wasm is built against API {imported}; \
             rebuild with `mumcord-plugin build`, which writes api for you"
        ),
        // A component without host imports links against any minor.
        None => Ok(()),
    }
}

fn reserved_warning(file: &str) -> String {
    format!(
        "warning: {file} is under the reserved folder web/_mumcord; Mumcord never serves it \
         (an API 1.2 package with such files is refused)"
    )
}

fn deprecation_warning(api: ApiMinor, deprecated: bool) -> Option<String> {
    deprecated.then(|| {
        format!(
            "warning: API {api} is deprecated and goes away with the next major API; \
             move to API {} (FR-9.11)",
            version::API_VERSION
        )
    })
}

fn import_allowed(name: &str) -> bool {
    let Some((interface, version)) = name.split_once('@') else {
        return false;
    };
    interface.starts_with(API_PREFIX)
        && semver::Version::parse(version).is_ok_and(|v| v.major == API_MAJOR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::tests::package;

    #[test]
    fn only_host_api_imports_are_allowed() {
        assert!(import_allowed("mumcord:plugin/storage@1.0.0"));
        assert!(import_allowed("mumcord:plugin/ui@1.3.0"));
        assert!(!import_allowed("mumcord:plugin/ui@2.0.0"));
        assert!(!import_allowed("mumcord:plugin/ui"));
        assert!(!import_allowed("wasi:cli/environment@0.2.0"));
    }

    #[test]
    fn api_comes_from_the_newest_host_import() {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            imports_api(&names(&[
                "mumcord:plugin/storage@1.0.0",
                "mumcord:plugin/stage@1.1.0"
            ])),
            Some(ApiMinor::V1_1)
        );
        assert_eq!(
            imports_api(&names(&["mumcord:plugin/ui@1.0.2"])),
            Some(ApiMinor::V1_0)
        );
        assert_eq!(imports_api(&[]), None);
    }

    #[test]
    fn api_must_match_the_imports() {
        assert!(api_matches(ApiMinor::V1_1, Some(ApiMinor::V1_1)).is_ok());
        assert!(api_matches(ApiMinor::V1_0, None).is_ok());
        let error = api_matches(ApiMinor::V1_0, Some(ApiMinor::V1_1)).expect_err("mismatch");
        assert!(
            error.to_string().contains("built against API 1.1"),
            "{error}"
        );
        assert!(api_matches(ApiMinor::V1_1, Some(ApiMinor::V1_0)).is_err());
    }

    #[test]
    fn reserved_folder_warning_names_the_file() {
        let warning = reserved_warning("web/_MUMCORD/x.css");
        assert!(warning.contains("web/_MUMCORD/x.css"), "{warning}");
        assert!(warning.starts_with("warning:"), "{warning}");
    }

    #[test]
    fn warns_only_on_a_deprecated_api() {
        assert_eq!(deprecation_warning(ApiMinor::V1_0, false), None);
        let warning = deprecation_warning(ApiMinor::V1_0, true).expect("warning");
        assert!(
            warning.starts_with("warning: API 1.0 is deprecated"),
            "{warning}"
        );
    }

    /// The committed API 1.0 fixture (F0), built with the 0.6.2 SDK.
    fn api10_fixture() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../e2e/fixtures/plugins/test-api10/test-api10.mcplugin");
        fs::read(path).expect("committed fixture")
    }

    /// The same package with another `api` in its manifest.
    fn with_api(package: &[u8], api: &str) -> Vec<u8> {
        use std::io::{Cursor, Read, Write};
        let mut archive = zip::ZipArchive::new(Cursor::new(package)).expect("zip");
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).expect("entry");
            let name = file.name().expect("utf-8").into_owned();
            let mut body = Vec::new();
            file.read_to_end(&mut body).expect("read");
            if name == "manifest.json" {
                let mut manifest: serde_json::Value = serde_json::from_slice(&body).expect("json");
                manifest["api"] = api.into();
                body = serde_json::to_vec(&manifest).expect("json");
            }
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .expect("write");
            out.write_all(&body).expect("write");
        }
        out.finish().expect("zip").into_inner()
    }

    #[test]
    fn a_real_component_passes_and_a_wrong_api_fails() {
        let dir = tempfile::tempdir().expect("test step");
        let path = dir.path().join("x.mcplugin");
        let package = api10_fixture();
        fs::write(&path, &package).expect("test step");
        run(&path, false, None).expect("the 1.0 fixture passes");

        fs::write(&path, with_api(&package, "^1.1")).expect("test step");
        let error = run(&path, false, None).expect_err("api differs from the imports");
        assert!(
            format!("{error:#}").contains("built against API 1.0"),
            "{error:#}"
        );
    }

    #[test]
    fn refuses_bad_packages_with_the_client_rules() {
        let dir = tempfile::tempdir().expect("test step");
        let path = dir.path().join("x.mcplugin");
        fs::write(&path, package("net-tool", "1.0.0", &["secrets"])).expect("test step");
        let error = run(&path, false, None).expect_err("official-only permission");
        assert!(
            format!("{error:#}").contains("only available to official plugins"),
            "{error:#}"
        );

        fs::write(&path, package("demo", "1.0.0", &[])).expect("test step");
        let error = run(&path, false, None).expect_err("not a component");
        assert!(
            format!("{error:#}").contains("not a component"),
            "{error:#}"
        );
    }
}
