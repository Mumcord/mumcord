//! `check`: runs the install checks of the client on a package (FR-6.10 … FR-6.12) plus a look at
//! the component's imports, so authors see refusals before publishing.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest::Source;
use mumcord_plugins::package::{Package, WASM_FILE};
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
    component_imports(component)?;
    if let Some(key) = key {
        let public = sign::read_public(key)?;
        let signature_path = sign::signature_path(path);
        let signature = fs::read_to_string(&signature_path)
            .with_context(|| format!("could not read {}", signature_path.display()))?;
        sign::verify(&public, &bytes, &signature).context("author signature check failed")?;
    }
    let manifest = &package.manifest;
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
            WorldItem::Interface { id, .. } => resolve.id_of(*id),
            _ => None,
        }
        .unwrap_or_else(|| resolve.name_world_key(key));
        if !import_allowed(&name) {
            bail!("plugin.wasm imports {name}; Mumcord provides only {API_PREFIX}*@{API_MAJOR}.x");
        }
        names.push(name);
    }
    Ok(names)
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
