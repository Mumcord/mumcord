//! `storage` through the real sandbox: a tiny plugin writes a value at start and the test
//! checks the database, so the WIT wiring (scope, key, value, error codes) is covered end to end.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mumcord_plugins::api::storage::Storage;
use mumcord_plugins::{CallError, HostContext, HostEvent, PluginHost};
use mumcord_store::{PluginRecord, PluginVisibility, Store};
use semver::Version;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// Core module: `start` calls `storage.set(scope, "k", "v1")`. On an error it refuses to
/// start with the error's code as a digit, so the test sees exactly what the plugin got.
const PLUGIN: &str = r#"
(module
  (import "mumcord:plugin/storage@1.0.0" "set" (func $set (param i32 i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 32) "k")
  (data (i32.const 40) "v1")
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (call $set (i32.const {{SCOPE}}) (i32.const 32) (i32.const 1) (i32.const 40) (i32.const 2)
      (i32.const 100))
    (if (i32.load8_u (i32.const 100))
      (then
        (i32.store8 (i32.const 48) (i32.add (i32.const 48) (i32.load8_u (i32.const 101))))
        (i32.store8 (i32.const 64) (i32.const 1))
        (i32.store (i32.const 68) (i32.const 48))
        (i32.store (i32.const 72) (i32.const 1))
        (return (i32.const 64))))
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param i32))
  (func (export "stop"))
)"#;
const IDENTITY: &str = "0";
const GLOBAL: &str = "1";
/// `invalid` in the WIT error enum.
const INVALID: &str = "6";

fn component(scope: &str) -> Vec<u8> {
    let mut module = wat::parse_str(PLUGIN.replace("{{SCOPE}}", scope)).expect("valid WAT");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("wit"))
        .expect("WIT parses");
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

fn plugin_dir(root: &Path, id: &str, scope: &str) -> std::path::PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    let manifest = format!(
        r#"{{"manifest":1,"id":"{id}","name":"Test","version":"1.0.0","api":"^1.0","authors":["Tests"],
           "description":"A test plugin","categories":["tools"],"license":"MIT"}}"#
    );
    std::fs::write(dir.join("manifest.json"), manifest).expect("write manifest");
    std::fs::write(dir.join("plugin.wasm"), component(scope)).expect("write wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("write i18n");
    dir
}

fn record(id: &str) -> PluginRecord {
    PluginRecord {
        id: id.into(),
        version: "1.0.0".into(),
        previous_version: None,
        source: "official".into(),
        sha256: None,
        author_key: None,
        enabled: true,
        disabled_reason: None,
        visibility: PluginVisibility::Friends,
        granted: vec![],
        pinned: false,
        notifications: true,
        removed_at: None,
        installed_at: 0,
    }
}

struct Setup {
    _root: tempfile::TempDir,
    store: Arc<Mutex<Store>>,
    host: PluginHost,
    events: UnboundedReceiver<HostEvent>,
}

fn setup(id: &str) -> Setup {
    let root = tempfile::tempdir().expect("temp dir");
    let store = Store::open(root.path()).expect("store");
    store.save_plugin(&record(id)).expect("save");
    let store = Arc::new(Mutex::new(store));
    let ctx = HostContext {
        storage: Storage::new(Arc::clone(&store)),
        secrets: None,
    };
    let (outbox, events) = unbounded_channel();
    let host = PluginHost::new(Version::new(0, 7, 0), None, Arc::new(ctx), outbox)
        .expect("runtime starts");
    Setup {
        _root: root,
        store,
        host,
        events,
    }
}

fn refused_with(events: &mut UnboundedReceiver<HostEvent>) -> CallError {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match events.try_recv() {
            Ok(HostEvent::Failed { error, .. }) => return error,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("no failure event");
}

#[test]
fn a_plugin_stores_a_value_in_the_global_scope() {
    let mut s = setup("keeper");
    let dir = plugin_dir(s._root.path(), "keeper", GLOBAL);
    s.host.load(&dir, None).expect("loads");
    s.host.enable("keeper").expect("starts");
    let value = s
        .store
        .lock()
        .expect("lock")
        .plugin_kv_get("keeper", "global", "k")
        .expect("read");
    assert_eq!(value, Some(b"v1".to_vec()));
}

#[test]
fn the_identity_scope_without_an_identity_is_invalid() {
    let mut s = setup("keeper");
    let dir = plugin_dir(s._root.path(), "keeper", IDENTITY);
    s.host.load(&dir, None).expect("loads");
    assert!(s.host.enable("keeper").is_err());
    assert_eq!(
        refused_with(&mut s.events),
        CallError::Refused(INVALID.into())
    );
}
