//! Builds the safety fixture plugins (AC-6.6 ... AC-6.19) from WAT sources.
//!
//! Layout of the source tree (`fixtures/safety`):
//! - `cores/<name>.wat`: core module of a plugin, or `<name>.component.wat` for a hand-written
//!   component. Placeholders are resolved from `crates/plugins/wit`, so the byte offsets of the
//!   canonical ABI never get hard-coded (see `Layout`).
//! - `<fixture>/manifest.json`: only what is special about the fixture; `common_manifest` fills
//!   in the rest in one place.
//! - `<fixture>/core.txt`: name of the core to use; `<fixture>/web/`: copied as-is if present.
//!
//! Output: `<out>/<fixture>/` (a folder the developer loader accepts) and `<out>/<fixture>.mcplugin`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};
use wit_parser::{Resolve, SizeAlign, TypeDefKind, TypeId};

/// Memory management and the shared helpers every core module gets in place of `{{PRELUDE}}`.
/// Requires the module to import `log.write` as `$log`.
const PRELUDE: &str = r#"
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))

  ;; Bump allocator: the host copies the start info into the plugin with it.
  (func (export "cabi_realloc") (param $old i32) (param $old_size i32) (param $align i32) (param $size i32) (result i32)
    (local $p i32)
    (local.set $p
      (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
        (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
    (global.set $heap (i32.add (local.get $p) (local.get $size)))
    (local.get $p))

  (func (export "stop"))

  ;; start result `ok`: discriminant 0 in the return area at 64.
  (func $start_ok (result i32)
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))

  (func $log_info (param $ptr i32) (param $len i32)
    (call $log (i32.const 1) (local.get $ptr) (local.get $len)))
"#;

/// Canonical-ABI offsets the WAT sources ask for with `{{NAME}}`.
struct Layout {
    resolve: Resolve,
    sizes: SizeAlign,
}

impl Layout {
    fn load(wit_dir: &Path) -> Result<Self> {
        let mut resolve = Resolve::default();
        resolve.push_dir(wit_dir).context("WIT package parses")?;
        let mut sizes = SizeAlign::default();
        sizes.fill(&resolve)?;
        Ok(Self { resolve, sizes })
    }

    fn type_in(&self, interface: &str, name: &str) -> Result<TypeId> {
        let (_, iface) = self
            .resolve
            .interfaces
            .iter()
            .find(|(_, i)| i.name.as_deref() == Some(interface))
            .ok_or_else(|| anyhow!("interface {interface} missing"))?;
        iface
            .types
            .get(name)
            .copied()
            .ok_or_else(|| anyhow!("type {interface}.{name} missing"))
    }

    fn case_index(&self, interface: &str, variant: &str, case: &str) -> Result<usize> {
        let id = self.type_in(interface, variant)?;
        let TypeDefKind::Variant(v) = &self.resolve.types[id].kind else {
            bail!("{interface}.{variant} is not a variant");
        };
        v.cases
            .iter()
            .position(|c| c.name == case)
            .ok_or_else(|| anyhow!("{interface}.{variant} has no case {case}"))
    }

    fn payload_offset(&self, interface: &str, variant: &str) -> Result<usize> {
        let id = self.type_in(interface, variant)?;
        let TypeDefKind::Variant(v) = &self.resolve.types[id].kind else {
            bail!("{interface}.{variant} is not a variant");
        };
        Ok(self
            .sizes
            .payload_offset(v.tag(), v.cases.iter().map(|c| c.ty.as_ref()))
            .size_wasm32())
    }

    /// Replaces every `{{...}}` placeholder in a WAT source.
    fn resolve(&self, source: &str) -> Result<String> {
        let mut out = String::new();
        let mut rest = source;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let end = rest[start..]
                .find("}}")
                .ok_or_else(|| anyhow!("unterminated placeholder"))?
                + start;
            let key = &rest[start + 2..end];
            out.push_str(&self.value(key)?);
            rest = &rest[end + 2..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn value(&self, key: &str) -> Result<String> {
        if key == "PRELUDE" {
            return Ok(PRELUDE.to_owned());
        }
        if let Some(case) = key.strip_prefix("EVENT_CASE:") {
            return Ok(self.case_index("events", "event", case)?.to_string());
        }
        if let Some(case) = key.strip_prefix("SLOT_CASE:") {
            return Ok(self.case_index("ui", "slot", case)?.to_string());
        }
        if let Some(case) = key.strip_prefix("COMPONENT_CASE:") {
            return Ok(self.case_index("ui", "component", case)?.to_string());
        }
        match key {
            "EVENT_PAYLOAD" => Ok(self.payload_offset("events", "event")?.to_string()),
            "COMPONENT_PAYLOAD" => Ok(self.payload_offset("ui", "component")?.to_string()),
            other => bail!("unknown placeholder {{{{{other}}}}}"),
        }
    }
}

/// Fields every fixture shares. A manifest-format change only touches this function.
fn common_manifest(name: &str) -> Map<String, Value> {
    let Value::Object(map) = json!({
        "manifest": 1,
        "api": "^1.0",
        "version": "1.0.0",
        "license": "MIT",
        "entry": "plugin.wasm",
        "author": { "name": "Mumcord test fixtures" },
        "authors": [{ "name": "Mumcord test fixtures" }],
        "category": "tools",
        "categories": ["tools"],
        "description": format!("Safety fixture {name}"),
        "i18n": { "default": "en", "available": ["en"] },
    }) else {
        unreachable!("literal is an object")
    };
    map
}

fn encode_component(wit_dir: &Path, core: &str) -> Result<Vec<u8>> {
    let mut module = wat::parse_str(core).context("core module is valid WAT")?;
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_dir(wit_dir)?;
    let world = resolve.select_world(&[package], Some("plugin"))?;
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
        false,
    )?;
    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)?
        .encode()
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn files_below(dir: &Path, prefix: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let relative = prefix.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            files_below(&entry.path(), &relative, out)?;
        } else {
            out.push(relative);
        }
    }
    Ok(())
}

fn pack(folder: &Path, package: &Path) -> Result<()> {
    let mut files = Vec::new();
    files_below(folder, Path::new(""), &mut files)?;
    files.sort();
    let mut zip = zip::ZipWriter::new(fs::File::create(package)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for relative in files {
        zip.start_file(relative.to_string_lossy().replace('\\', "/"), options)?;
        zip.write_all(&fs::read(folder.join(&relative))?)?;
    }
    zip.finish()?;
    Ok(())
}

fn build_fixture(
    layout: &Layout,
    wit_dir: &Path,
    root: &Path,
    dir: &Path,
    out: &Path,
) -> Result<()> {
    let fixture = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("fixture dir name"))?
        .to_owned();
    let core_name = fs::read_to_string(dir.join("core.txt"))?.trim().to_owned();
    let wasm = match fs::read_to_string(root.join(format!("cores/{core_name}.wat"))) {
        Ok(source) => encode_component(wit_dir, &layout.resolve(&source)?)?,
        Err(_) => {
            let source = fs::read_to_string(root.join(format!("cores/{core_name}.component.wat")))
                .with_context(|| format!("core {core_name} exists"))?;
            wat::parse_str(source).context("component is valid WAT")?
        }
    };

    let Value::Object(special) = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)?
    else {
        bail!("{fixture}: manifest.json must be an object");
    };
    let mut manifest = common_manifest(&fixture);
    manifest.extend(special);
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&fixture)
        .to_owned();
    manifest.insert("name".into(), Value::String(name.clone()));
    let description = manifest["description"]
        .as_str()
        .unwrap_or_default()
        .to_owned();

    let folder = out.join(&fixture);
    if folder.exists() {
        fs::remove_dir_all(&folder)?;
    }
    fs::create_dir_all(folder.join("i18n"))?;
    fs::write(folder.join("plugin.wasm"), wasm)?;
    fs::write(
        folder.join("manifest.json"),
        serde_json::to_vec_pretty(&Value::Object(manifest))?,
    )?;
    fs::write(
        folder.join("i18n/en.json"),
        serde_json::to_vec_pretty(
            &json!({ "manifest.name": name, "manifest.description": description }),
        )?,
    )?;
    if dir.join("web").is_dir() {
        copy_dir(&dir.join("web"), &folder.join("web"))?;
    }
    pack(&folder, &out.join(format!("{fixture}.mcplugin")))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, wit_dir, out] = args.as_slice() else {
        bail!("usage: build-safety-fixtures <fixtures/safety> <crates/plugins/wit> <out-dir>");
    };
    let (root, wit_dir, out) = (Path::new(root), Path::new(wit_dir), Path::new(out));
    let layout = Layout::load(wit_dir)?;
    fs::create_dir_all(out)?;

    let mut dirs: Vec<PathBuf> = fs::read_dir(root)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("core.txt").is_file())
        .collect();
    dirs.sort();
    for dir in &dirs {
        build_fixture(&layout, wit_dir, root, dir, out)
            .with_context(|| format!("fixture {}", dir.display()))?;
    }
    println!("built {} fixtures into {}", dirs.len(), out.display());
    Ok(())
}
