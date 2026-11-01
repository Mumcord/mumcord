//! `ui`, `peers` and `chat` through the real sandbox: a tiny plugin sends a peer message and
//! clears its panel at start, and a recording `PluginIo` shows what reached the service side.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_plugins::wit::peers::Target;
use mumcord_plugins::wit::types::{BotInfo, Error, Peer};
use mumcord_plugins::wit::ui::{Node, Slot, UserMenuFilter};
use mumcord_plugins::{CallError, HostContext, HostEvent, PluginHost, PluginIo};
use semver::Version;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// Core module: `start` calls `peers.send(channel, 7, "hi")`; on an error it refuses to start
/// with the error's code as a digit. Then it calls `ui.clear(panel)`.
const PLUGIN: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send"
    (func $send (param i32 i32 i32 i32 i32 i32)))
  (import "mumcord:plugin/ui@1.0.0" "clear" (func $clear (param i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 40) "hi")
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (call $send (i32.const 0) (i32.const 0) (i32.const 7) (i32.const 40) (i32.const 2)
      (i32.const 100))
    (if (i32.load8_u (i32.const 100))
      (then
        (i32.store8 (i32.const 48) (i32.add (i32.const 48) (i32.load8_u (i32.const 101))))
        (i32.store8 (i32.const 64) (i32.const 1))
        (i32.store (i32.const 68) (i32.const 48))
        (i32.store (i32.const 72) (i32.const 1))
        (return (i32.const 64))))
    (call $clear (i32.const 0) (i32.const 0) (i32.const 0))
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param i32))
  (func (export "stop"))
)"#;
/// `not-permitted` in the WIT error enum.
const NOT_PERMITTED: &str = "0";

#[derive(Debug, Default)]
struct Recorder {
    calls: Mutex<Vec<String>>,
}

impl Recorder {
    fn record(&self, call: String) {
        self.calls.lock().expect("lock").push(call);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("lock").clone()
    }
}

impl PluginIo for Recorder {
    fn set_tree(&self, plugin: &str, _slot: Slot, nodes: Vec<Node>) -> Result<(), Error> {
        self.record(format!("{plugin} set_tree {}", nodes.len()));
        Ok(())
    }
    fn clear(&self, plugin: &str, slot: Slot) {
        self.record(format!("{plugin} clear {slot:?}"));
    }
    fn open_dialog(&self, _plugin: &str, _title: String) -> Result<(), Error> {
        Ok(())
    }
    fn close_dialog(&self, _plugin: &str) {}
    fn set_user_menu_entry(&self, _plugin: &str, _label: Option<String>) -> Result<(), Error> {
        Ok(())
    }
    fn set_user_menu_entry_for(
        &self,
        _plugin: &str,
        _label: String,
        _filter: UserMenuFilter,
    ) -> Result<(), Error> {
        Ok(())
    }
    fn peer_send(&self, plugin: &str, to: Target, kind: u8, payload: Vec<u8>) -> Result<(), Error> {
        let payload = String::from_utf8_lossy(&payload);
        self.record(format!("{plugin} send {to:?} {kind} {payload}"));
        Ok(())
    }
    fn start_game_burst(&self, _plugin: &str) -> Result<(), Error> {
        Ok(())
    }
    fn presence(&self, _plugin: &str) -> Result<Vec<Peer>, Error> {
        Ok(Vec::new())
    }
    fn bot(&self, _plugin: &str, _session: u32) -> Result<Option<BotInfo>, Error> {
        Ok(None)
    }
    fn chat_send(&self, _plugin: &str, _text: String) -> Result<(), Error> {
        Ok(())
    }
    fn line_hash(&self, _plugin: &str, _text: &str) -> u32 {
        0
    }
    fn supersede(
        &self,
        _plugin: &str,
        _card: String,
        _sender: u32,
        _hash: u32,
    ) -> Result<(), Error> {
        Ok(())
    }
}

fn component() -> Vec<u8> {
    let mut module = wat::parse_str(PLUGIN).expect("valid WAT");
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

fn plugin_dir(root: &Path) -> PathBuf {
    let dir = root.join("talker");
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    let manifest = r#"{"manifest":1,"id":"talker","name":"Test","version":"1.0.0","api":"^1.0",
        "authors":["Tests"],"description":"A test plugin","categories":["tools"],"license":"MIT"}"#;
    std::fs::write(dir.join("manifest.json"), manifest).expect("write manifest");
    std::fs::write(dir.join("plugin.wasm"), component()).expect("write wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("write i18n");
    dir
}

struct Setup {
    _root: tempfile::TempDir,
    host: PluginHost,
    events: UnboundedReceiver<HostEvent>,
}

fn setup(io: Option<Arc<dyn PluginIo>>, granted: &[&str]) -> Setup {
    let root = tempfile::tempdir().expect("temp dir");
    let ctx = Arc::new(HostContext {
        io,
        ..HostContext::default()
    });
    ctx.grants.set("talker", granted.iter().copied());
    let (outbox, events) = unbounded_channel();
    let mut host = PluginHost::new(Version::new(0, 7, 0), None, Arc::clone(&ctx), outbox)
        .expect("runtime starts");
    host.load(&plugin_dir(root.path()), None).expect("loads");
    Setup {
        _root: root,
        host,
        events,
    }
}

fn refused_with(events: &mut UnboundedReceiver<HostEvent>) -> CallError {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match events.try_recv() {
            Ok(HostEvent::Failed { error, .. }) => return error,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("no failure event");
}

#[test]
fn granted_calls_reach_the_io() {
    let recorder = Arc::new(Recorder::default());
    let mut s = setup(
        Some(Arc::clone(&recorder) as Arc<dyn PluginIo>),
        &["peers.messages", "ui.panel"],
    );
    s.host.enable("talker").expect("starts");
    assert_eq!(
        recorder.calls(),
        [
            "talker send Target::Channel 7 hi",
            "talker clear Slot::Panel"
        ]
    );
}

#[test]
fn a_clear_without_the_slot_permission_is_dropped() {
    let recorder = Arc::new(Recorder::default());
    let mut s = setup(
        Some(Arc::clone(&recorder) as Arc<dyn PluginIo>),
        &["peers.messages"],
    );
    s.host.enable("talker").expect("starts");
    assert_eq!(recorder.calls(), ["talker send Target::Channel 7 hi"]);
}

#[test]
fn a_send_without_the_permission_is_refused() {
    let recorder = Arc::new(Recorder::default());
    let mut s = setup(Some(Arc::clone(&recorder) as Arc<dyn PluginIo>), &[]);
    assert!(s.host.enable("talker").is_err());
    assert_eq!(
        refused_with(&mut s.events),
        CallError::Refused(NOT_PERMITTED.into())
    );
    assert!(recorder.calls().is_empty());
}

#[test]
fn without_an_io_a_send_is_not_permitted() {
    let mut s = setup(None, &["peers.messages"]);
    assert!(s.host.enable("talker").is_err());
    assert_eq!(
        refused_with(&mut s.events),
        CallError::Refused(NOT_PERMITTED.into())
    );
}
