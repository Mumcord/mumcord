//! Linking by API minor (FR-9.5 … FR-9.8, AC-9.25 unit part): a plugin is linked against
//! exactly the minor its manifest declares, and a 1.0 plugin keeps working as on 0.6.x next to
//! 1.1 and 1.2 plugins. The plugins are built from `fixtures/plugin.wat` against `wit/1.0.0`,
//! `wit/1.1.0` or `wit/1.2.0`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mumcord_plugins::version::{ApiMinor, imports_api};
use mumcord_plugins::wit::events::Event;
use mumcord_plugins::{CompileError, HostContext, LoadError, PluginHost};
use semver::Version;
use tokio::sync::mpsc::unbounded_channel;

/// Logs the `start-info.api-version` it got, then returns `ok`.
const LOG_API_VERSION: &str = "
    (call $log (i32.const 1) (local.get $api_ptr) (local.get $api_len))
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64)";
/// Logs 'E' for every event.
const LOG_EVENT: &str = "
    (i32.store8 (i32.const 100) (i32.const 69))
    (call $log (i32.const 1) (i32.const 100) (i32.const 1))";

fn wit(version: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("wit")
        .join(version)
}

/// The fixture built against `wit/<version>`, importing that version's interfaces.
fn component(version: &str) -> Vec<u8> {
    let core = include_str!("fixtures/plugin.wat")
        .replace("@1.0.0", &format!("@{version}"))
        .replace("{{START}}", LOG_API_VERSION)
        .replace("{{EVENT}}", LOG_EVENT)
        .replace("{{STOP}}", "");
    let mut module = wat::parse_str(core).expect("fixture is valid WAT");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve.push_dir(wit(version)).expect("WIT parses");
    let world = resolve
        .select_world(&[package], Some("plugin"))
        .expect("world exists");
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
        false,
    )
    .expect("metadata embeds");
    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .expect("module is accepted")
        .encode()
        .expect("component encodes")
}

fn plugin_dir(root: &Path, id: &str, wasm: &[u8], api: &str) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    let manifest = format!(
        r#"{{"manifest":1,"id":"{id}","name":"Test","version":"1.0.0","api":"{api}","authors":["Tests"],
           "description":"A test plugin","categories":["tools"],"license":"MIT"}}"#
    );
    std::fs::write(dir.join("manifest.json"), manifest).expect("write manifest");
    std::fs::write(dir.join("plugin.wasm"), wasm).expect("write wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("write i18n");
    dir
}

fn host() -> PluginHost {
    let (outbox, _events) = unbounded_channel();
    PluginHost::new(
        Version::new(0, 7, 0),
        None,
        Arc::new(HostContext::default()),
        outbox,
    )
    .expect("runtime starts")
}

fn logged(host: &PluginHost, id: &str, lines: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let now: Vec<String> = host
            .log_lines(id)
            .into_iter()
            .map(|(_, line)| line)
            .collect();
        if now.len() >= lines || Instant::now() > deadline {
            return now;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn components_tell_which_minor_they_import() {
    let engine =
        wasmtime::Engine::new(wasmtime::Config::new().wasm_component_model(true)).expect("engine");
    for (version, api) in [
        ("1.0.0", ApiMinor::V1_0),
        ("1.1.0", ApiMinor::V1_1),
        ("1.2.0", ApiMinor::V1_2),
    ] {
        let component =
            wasmtime::component::Component::new(&engine, component(version)).expect("compiles");
        assert_eq!(imports_api(&component), Some(api));
    }
}

/// A 1.0 plugin and a 1.1 plugin run side by side, each linked against its own minor, and both
/// get the events the host produces.
#[test]
fn each_plugin_is_linked_against_its_declared_minor() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    for (id, version, api) in [
        ("old", "1.0.0", "^1.0"),
        ("new", "1.1.0", "^1.1"),
        ("newest", "1.2.0", "^1.2"),
    ] {
        host.load(&plugin_dir(root.path(), id, &component(version), api), None)
            .expect("loads");
        host.enable(id).expect("starts");
        assert!(host.send(id, Event::Timer(7)));
    }
    assert_eq!(logged(&host, "old", 2), ["1.0.0", "E"]);
    assert_eq!(logged(&host, "new", 2), ["1.1.0", "E"]);
    assert_eq!(logged(&host, "newest", 2), ["1.2.0", "E"]);
}

/// FR-9.8: the manifest's minor and the component's imports must agree, so a plugin that says
/// 1.0 can never reach a 1.1 import, and a 1.0 build cannot claim 1.1.
#[test]
fn a_component_built_for_another_minor_is_refused() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    for (id, version, api) in [
        ("claims-old", "1.1.0", "^1.0"),
        ("claims-new", "1.0.0", "^1.1"),
        ("claims-1-1", "1.2.0", "^1.1"),
        ("claims-1-2", "1.1.0", "^1.2"),
    ] {
        let error = host
            .load(&plugin_dir(root.path(), id, &component(version), api), None)
            .expect_err("refused");
        assert!(
            matches!(&error, LoadError::Compile(CompileError::Mismatch(m)) if m.contains("API")),
            "{error:?}"
        );
    }
}
