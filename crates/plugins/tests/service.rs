//! The plugin service end to end with real (tiny) plugins, a real store and packages signed
//! with keys generated per test run: install, restore after remove, permission revokes,
//! Turn off all, `--no-plugins`, developer folders, badge default and the integrity re-check;
//! plugin messaging with its rate rules, visibility and the voice-server-only rule.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use minisign::KeyPair;
use mumcord_plugins::Source;
use mumcord_plugins::registry::{Download, key_fingerprint_prefix};
use mumcord_plugins::service::{
    InstallRequest, InstallStage, PeerInfo, PluginInfo, PluginService, SelfInfo, ServiceConfig,
    ServiceError, ServiceEvent, Status, StoredLine,
};
use mumcord_plugins::verify::IndexEntry;
use mumcord_plugins::{Digest, HostEvent};
use mumcord_store::{NewPluginRepository, PluginVisibility, Store};
use semver::Version;
use tokio::sync::mpsc::UnboundedReceiver;
use zip::write::SimpleFileOptions;

const REPO: Source = Source::Repo { repository_id: 1 };
const OK_START: &str = "(i32.store8 (i32.const 64) (i32.const 0)) (i32.const 64)";
/// `start` returns the error "nope".
const FAILING_START: &str = "
    (i32.store8 (i32.const 64) (i32.const 1))
    (i32.store (i32.const 68) (i32.const 32))
    (i32.store (i32.const 72) (i32.const 4))
    (i32.const 64)";
const PERMISSIONS: &str = r#","permissions":{"required":["ui.panel"],"optional":[{"id":"events.voice"},{"id":"ui.member-badge"}]}"#;

fn component(start: &str) -> Vec<u8> {
    let core = include_str!("fixtures/plugin.wat")
        .replace("{{START}}", start)
        .replace("{{EVENT}}", "")
        .replace("{{STOP}}", "");
    encode(&core)
}

fn encode(core: &str) -> Vec<u8> {
    let mut module = wat::parse_str(core).expect("fixture is valid WAT");
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

fn manifest(id: &str, version: &str, extra: &str) -> String {
    format!(
        r#"{{"manifest":1,"id":"{id}","name":"Test","version":"{version}","api":"^1.0","authors":["Tests"],
           "description":"A test plugin","categories":["tools"],"license":"MIT"{extra}}}"#
    )
}

fn package(manifest: &str, wasm: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("manifest.json", manifest.as_bytes()),
        ("plugin.wasm", wasm),
        ("i18n/en.json", b"{}".as_slice()),
    ] {
        writer.start_file(name, options).expect("start file");
        writer.write_all(data).expect("write");
    }
    writer.finish().expect("finish").into_inner()
}

struct Signed {
    request: InstallRequest,
    download: Download,
}

/// A signed package plus the index entry promising it, as the registry would deliver it.
fn signed(id: &str, start: &str, extra: &str, optional: &[&str]) -> Signed {
    signed_wasm(id, &component(start), extra, optional)
}

fn signed_wasm(id: &str, wasm: &[u8], extra: &str, optional: &[&str]) -> Signed {
    let author = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let bytes = package(&manifest(id, "1.0.0", extra), wasm);
    let signature = minisign::sign(Some(&author.pk), &author.sk, &bytes[..], None, None)
        .expect("sign")
        .into_string();
    Signed {
        request: InstallRequest {
            source: REPO,
            entry: IndexEntry {
                id: id.into(),
                version: "1.0.0".into(),
                digest: Digest::of(&bytes),
                author_keys: vec![author.pk.to_base64()],
                permissions: vec![
                    "ui.panel".into(),
                    "events.voice".into(),
                    "ui.member-badge".into(),
                ],
            },
            optional: optional.iter().map(|p| (*p).to_owned()).collect(),
            visibility: PluginVisibility::Friends,
        },
        download: Download { bytes, signature },
    }
}

struct Fixture {
    root: tempfile::TempDir,
    store: Arc<Mutex<Store>>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("temp dir");
        let store = Store::open(root.path()).expect("store opens");
        Self {
            root,
            store: Arc::new(Mutex::new(store)),
        }
    }

    fn plugins_dir(&self) -> PathBuf {
        self.root.path().join("plugins")
    }

    fn service(&self) -> (PluginService, UnboundedReceiver<ServiceEvent>) {
        self.service_with(Version::new(0, 7, 0), false)
    }

    fn service_with(
        &self,
        app_version: Version,
        no_plugins: bool,
    ) -> (PluginService, UnboundedReceiver<ServiceEvent>) {
        let (service, _host_events, events) = self.service_with_host(app_version, no_plugins);
        (service, events)
    }

    fn service_with_host(
        &self,
        app_version: Version,
        no_plugins: bool,
    ) -> (
        PluginService,
        UnboundedReceiver<HostEvent>,
        UnboundedReceiver<ServiceEvent>,
    ) {
        let config = ServiceConfig {
            plugins_dir: self.plugins_dir(),
            cache_dir: None,
            app_version,
            no_plugins,
        };
        let (mut service, host_events, events) =
            PluginService::new(config, Arc::clone(&self.store), None).expect("service builds");
        service.start().expect("service starts");
        (service, host_events, events)
    }

    fn set_developer_mode(&self, on: bool) {
        let store = self.store.lock().expect("lock");
        let mut settings = store.settings().expect("settings");
        settings.developer_mode = on;
        store.save_settings(&settings).expect("save settings");
    }

    fn kv(&self, id: &str) -> Option<Vec<u8>> {
        self.store
            .lock()
            .expect("lock")
            .plugin_kv_get(id, "global", "k")
            .expect("kv read")
    }

    fn set_kv(&self, id: &str) {
        self.store
            .lock()
            .expect("lock")
            .plugin_kv_set(id, "global", "k", b"v", 1024, 0)
            .expect("kv write");
    }
}

fn info(service: &PluginService, id: &str) -> PluginInfo {
    service
        .list()
        .expect("list")
        .into_iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| panic!("{id} is listed"))
}

fn stages(events: &mut UnboundedReceiver<ServiceEvent>) -> Vec<InstallStage> {
    let mut stages = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let ServiceEvent::InstallProgress { stage, .. } = event {
            stages.push(stage);
        }
    }
    stages
}

#[test]
fn installs_starts_and_reports_progress() {
    let fx = Fixture::new();
    let (mut service, mut events) = fx.service();
    let poll = signed("poll", OK_START, PERMISSIONS, &["events.voice", "net.http"]);
    service
        .install(&poll.request, &poll.download)
        .expect("installs");

    let plugin = info(&service, "poll");
    assert_eq!(plugin.status, Status::Running);
    assert_eq!(plugin.source, REPO);
    assert_eq!(plugin.visibility, PluginVisibility::Friends);
    // Required always, the chosen optional one, nothing the manifest does not ask for.
    assert_eq!(plugin.granted, ["ui.panel", "events.voice"]);
    assert!(fx.plugins_dir().join("poll/1.0.0/plugin.wasm").exists());
    assert_eq!(
        stages(&mut events),
        [
            InstallStage::Verifying,
            InstallStage::Unpacking,
            InstallStage::Starting,
            InstallStage::Done
        ]
    );
    assert!(matches!(
        service.install(&poll.request, &poll.download),
        Err(ServiceError::AlreadyInstalled)
    ));
}

#[test]
fn a_tampered_download_or_failing_start_leaves_nothing_behind() {
    let fx = Fixture::new();
    let (mut service, mut events) = fx.service();
    let mut poll = signed("poll", OK_START, PERMISSIONS, &[]);
    poll.download.bytes.push(0);
    let error = service
        .install(&poll.request, &poll.download)
        .expect_err("refused");
    assert_eq!(error.to_string(), mumcord_plugins::verify::MISMATCH_TEXT);
    assert!(matches!(
        stages(&mut events).last(),
        Some(InstallStage::Failed { reason }) if reason == mumcord_plugins::verify::MISMATCH_TEXT
    ));

    let broken = signed("broken", FAILING_START, "", &[]);
    assert!(service.install(&broken.request, &broken.download).is_err());
    assert!(service.list().expect("list").is_empty());
    assert!(!fx.plugins_dir().join("broken").exists());
}

#[test]
fn remove_keeps_data_for_a_reinstall_unless_asked_to_delete_it() {
    let fx = Fixture::new();
    let (mut service, _events) = fx.service();
    let pets = signed("pets", OK_START, "", &[]);
    service
        .install(&pets.request, &pets.download)
        .expect("installs");
    fx.set_kv("pets");
    assert!(
        service
            .export_data("pets")
            .expect("exports")
            .contains("\"k\"")
    );

    service.remove("pets", false).expect("removes");
    assert!(service.list().expect("list").is_empty());
    assert!(!fx.plugins_dir().join("pets").exists());
    assert_eq!(fx.kv("pets").as_deref(), Some(b"v".as_slice()));

    service
        .install(&pets.request, &pets.download)
        .expect("reinstalls");
    assert_eq!(
        fx.kv("pets").as_deref(),
        Some(b"v".as_slice()),
        "pet is back"
    );

    service.remove("pets", true).expect("removes with data");
    assert_eq!(fx.kv("pets"), None);
    assert!(
        fx.store
            .lock()
            .expect("lock")
            .plugin("pets")
            .expect("read")
            .is_none()
    );
}

#[test]
fn reset_deletes_data_and_keeps_the_plugin_running() {
    let fx = Fixture::new();
    let (mut service, _events) = fx.service();
    let pets = signed("pets", OK_START, "", &[]);
    service
        .install(&pets.request, &pets.download)
        .expect("installs");
    fx.set_kv("pets");
    service.reset_data("pets").expect("resets");
    assert_eq!(fx.kv("pets"), None);
    assert_eq!(info(&service, "pets").status, Status::Running);
}

#[test]
fn revoking_a_required_permission_turns_the_plugin_off() {
    let fx = Fixture::new();
    let (mut service, _events) = fx.service();
    let poll = signed("poll", OK_START, PERMISSIONS, &["events.voice"]);
    service
        .install(&poll.request, &poll.download)
        .expect("installs");

    service
        .set_permissions("poll", &["ui.panel".into()])
        .expect("optional revoked");
    let plugin = info(&service, "poll");
    assert_eq!(plugin.status, Status::Running);
    assert_eq!(plugin.granted, ["ui.panel"]);

    service
        .set_permissions("poll", &[])
        .expect("required revoked");
    let plugin = info(&service, "poll");
    assert_eq!(plugin.status, Status::Off);
    assert_eq!(plugin.disabled_reason.as_deref(), Some("permission"));
    assert!(matches!(
        service.set_enabled("poll", true),
        Err(ServiceError::RequiredMissing)
    ));

    service
        .set_permissions("poll", &["ui.panel".into()])
        .expect("granted again");
    service.set_enabled("poll", true).expect("turns on");
    assert_eq!(info(&service, "poll").status, Status::Running);
    service.set_enabled("poll", false).expect("turns off");
    let plugin = info(&service, "poll");
    assert_eq!(plugin.status, Status::Off);
    assert_eq!(plugin.disabled_reason.as_deref(), Some("user"));
}

#[test]
fn turn_off_all_survives_a_restart_and_no_plugins_loads_nothing() {
    let fx = Fixture::new();
    let (mut service, _events) = fx.service();
    let poll = signed("poll", OK_START, "", &[]);
    service
        .install(&poll.request, &poll.download)
        .expect("installs");
    service.set_all_off(true).expect("all off");
    assert_eq!(info(&service, "poll").status, Status::AllOff);
    drop(service);

    let (mut service, _events) = fx.service();
    assert!(service.all_off().expect("setting"));
    assert_eq!(info(&service, "poll").status, Status::AllOff);
    assert!(matches!(
        service.set_enabled("poll", true),
        Err(ServiceError::AllOff)
    ));
    service.set_all_off(false).expect("on again");
    assert_eq!(info(&service, "poll").status, Status::Running);
    drop(service);

    let (mut service, _events) = fx.service_with(Version::new(0, 7, 0), true);
    assert!(service.no_plugins());
    assert_eq!(info(&service, "poll").status, Status::NoPlugins);
    assert!(service.dir("poll").is_none(), "nothing loaded");
    let other = signed("other", OK_START, "", &[]);
    assert!(matches!(
        service.install(&other.request, &other.download),
        Err(ServiceError::NoPlugins)
    ));
    drop(service);

    // --no-plugins changed nothing that is stored.
    let (service, _events) = fx.service();
    assert_eq!(info(&service, "poll").status, Status::Running);
}

#[test]
fn the_first_badge_plugin_installed_is_the_default_badge() {
    let fx = Fixture::new();
    let (mut service, _events) = fx.service();
    let first = signed("first", OK_START, PERMISSIONS, &["ui.member-badge"]);
    let second = signed("second", OK_START, PERMISSIONS, &["ui.member-badge"]);
    service
        .install(&first.request, &first.download)
        .expect("installs");
    service
        .install(&second.request, &second.download)
        .expect("installs");
    assert_eq!(
        service.badge_plugin().expect("badge").as_deref(),
        Some("first")
    );
    assert!(info(&service, "first").badge);

    service.set_badge_plugin(None).expect("none chosen");
    assert_eq!(service.badge_plugin().expect("badge"), None);
    service
        .set_badge_plugin(Some("second".into()))
        .expect("second chosen");
    service.remove("second", true).expect("removes");
    assert_eq!(service.badge_plugin().expect("badge"), None);
}

#[test]
fn changed_files_are_found_after_an_app_update_and_old_versions_cleaned() {
    let fx = Fixture::new();
    let (mut service, _events) = fx.service();
    let poll = signed("poll", OK_START, "", &[]);
    service
        .install(&poll.request, &poll.download)
        .expect("installs");
    drop(service);

    std::fs::write(fx.plugins_dir().join("poll/1.0.0/i18n/en.json"), "{ }").expect("tamper");
    let stale = fx.plugins_dir().join("poll/0.9.0");
    std::fs::create_dir_all(&stale).expect("old version");
    let orphan = fx.plugins_dir().join("gone/1.0.0");
    std::fs::create_dir_all(&orphan).expect("orphan");

    let (service, _events) = fx.service_with(Version::new(0, 8, 0), false);
    let plugin = info(&service, "poll");
    assert_eq!(plugin.status, Status::Off);
    assert_eq!(plugin.disabled_reason.as_deref(), Some("tampered"));
    assert!(!stale.exists());
    assert!(!fx.plugins_dir().join("gone").exists());
    assert!(fx.plugins_dir().join("poll/1.0.0").exists());
}

#[test]
fn a_plugin_that_cannot_load_keeps_its_translated_name_and_icon() {
    let fx = Fixture::new();
    let (mut service, _events) = fx.service();
    let dice = signed("dice", OK_START, "", &[]);
    service
        .install(&dice.request, &dice.download)
        .expect("installs");
    drop(service);

    let installed = fx.plugins_dir().join("dice/1.0.0");
    let extra = r#","icon":"assets/icon.svg""#;
    std::fs::write(
        installed.join("manifest.json"),
        manifest("dice", "1.0.0", extra),
    )
    .expect("manifest");
    std::fs::create_dir_all(installed.join("assets")).expect("assets");
    std::fs::write(installed.join("assets/icon.svg"), "<svg/>").expect("icon");
    std::fs::write(
        installed.join("i18n/de.json"),
        r#"{"manifest.name":"Würfel","manifest.description":"Würfelt"}"#,
    )
    .expect("translation");
    std::fs::remove_file(installed.join("plugin.wasm")).expect("break the plugin");

    let (mut service, _events) = fx.service();
    service.set_language("de");
    let plugin = info(&service, "dice");
    assert!(matches!(plugin.status, Status::NotLoaded { .. }));
    assert_eq!(plugin.name, "Würfel");
    assert_eq!(plugin.description, "Würfelt");
    assert_eq!(plugin.icon.as_deref(), Some("assets/icon.svg"));
    assert_eq!(service.files_dir("dice"), Some(installed));
    assert!(service.dir("dice").is_none(), "nothing loaded");
}

fn dev_folder(root: &Path, id: &str, extra: &str) -> PathBuf {
    let dir = root.join(format!("src-{id}"));
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(dir.join("manifest.json"), manifest(id, "0.1.0", extra)).expect("manifest");
    std::fs::write(dir.join("plugin.wasm"), component(OK_START)).expect("wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("i18n");
    dir
}

#[test]
fn developer_folders_follow_developer_mode() {
    let fx = Fixture::new();
    let (mut service, _events) = fx.service();
    let folder = dev_folder(fx.root.path(), "devtool", PERMISSIONS);
    assert!(matches!(
        service.load_dev(&folder),
        Err(ServiceError::DeveloperModeOff)
    ));

    fx.set_developer_mode(true);
    let id = service.load_dev(&folder).expect("loads");
    let plugin = info(&service, &id);
    assert_eq!(plugin.status, Status::Running);
    assert_eq!(plugin.source, Source::Dev);
    assert_eq!(
        plugin.developer_folder,
        Some(folder.canonicalize().expect("path"))
    );
    service.reload_dev(&id).expect("reloads");
    assert_eq!(info(&service, &id).status, Status::Running);

    let http = dev_folder(
        fx.root.path(),
        "fetcher",
        r#","permissions":{"required":["net.http"]},"net":{"hosts":["example.org"]}"#,
    );
    assert!(matches!(
        service.load_dev(&http),
        Err(ServiceError::Manifest(_))
    ));

    let installed = signed("taken", OK_START, "", &[]);
    service
        .install(&installed.request, &installed.download)
        .expect("installs");
    assert!(matches!(
        service.load_dev(&dev_folder(fx.root.path(), "taken", "")),
        Err(ServiceError::IdTaken(_))
    ));

    fx.set_developer_mode(false);
    service.developer_mode_changed(false).expect("unloads");
    assert!(service.list().expect("list").iter().all(|p| p.id != id));
    assert!(service.dir(&id).is_none());
}

/// Core module: `start` sends "hi" (kind 5) to the channel with `peers.send`. On an error it
/// refuses to start with the error's code as a digit.
const SENDER: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 40) "hi")
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (call $send (i32.const 0) (i32.const 0) (i32.const 5) (i32.const 40) (i32.const 2)
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
const MESSAGING: &str = r#","permissions":{"required":["peers.messages"]}"#;
/// `no-recipients` in the WIT error enum.
const NO_RECIPIENTS: &str = "3";

fn sender_folder(root: &Path, id: &str) -> PathBuf {
    let dir = root.join(format!("src-{id}"));
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(dir.join("manifest.json"), manifest(id, "0.1.0", MESSAGING)).expect("manifest");
    std::fs::write(dir.join("plugin.wasm"), encode(SENDER)).expect("wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("i18n");
    dir
}

fn bob() -> PeerInfo {
    PeerInfo {
        session: 7,
        name: "Bob".into(),
        cert_hash: Some("ab".repeat(20)),
        is_friend: false,
    }
}

#[test]
fn messages_go_to_peers_announcing_the_same_plugin() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, mut host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    service.peer_joined(1, bob());
    service.peer_data(
        1,
        &bob(),
        "mumcord.plugins",
        br#"{"v":1,"p":["sender@1~dev"]}"#,
    );

    let id = service
        .load_dev(&sender_folder(fx.root.path(), "sender"))
        .expect("loads");
    assert_eq!(info(&service, &id).status, Status::Running);
    let mut sent = None;
    while let Ok(event) = host_events.try_recv() {
        service.handle_host_event(event).expect("handled");
    }
    while let Ok(event) = events.try_recv() {
        if let ServiceEvent::PeerSend {
            server,
            plugin,
            outgoing,
        } = event
        {
            sent = Some((server, plugin, outgoing));
        }
    }
    let (server, plugin, outgoing) = sent.expect("a message for the engine");
    assert_eq!((server, plugin.as_str()), (1, "sender"));
    assert_eq!(outgoing.data_id, "mumcord.p.sender");
    assert_eq!(outgoing.receivers, [7]);
    assert_eq!(outgoing.bytes, [1, 1, 5, b'h', b'i']);
    assert_eq!(service.messaging_counters("sender").sent, 1);

    // Nobody runs "lonely": its send at start fails with no-recipients.
    let lonely = service
        .load_dev(&sender_folder(fx.root.path(), "lonely"))
        .expect("loads");
    assert!(matches!(
        info(&service, &lonely).status,
        Status::BackingOff { error } if error.ends_with(NO_RECIPIENTS)
    ));

    // The announce lists the messaging plugins and goes to capable peers, once per 10 s.
    let announces = service.poll_announces();
    assert_eq!(announces.len(), 1);
    assert_eq!(announces[0].receivers, [7]);
    let text = String::from_utf8(announces[0].bytes.clone()).expect("JSON");
    assert!(
        text.contains("sender@1~dev") && text.contains("lonely@1~dev"),
        "{text}"
    );
    assert!(service.poll_announces().is_empty());
    assert!(service.next_announce_at().is_none());

    // Incoming: accepted from the announcing member, refused from someone outside the channel.
    service.peer_data(1, &bob(), "mumcord.p.sender", &[1, 1, 9, 0]);
    let stranger = PeerInfo {
        session: 9,
        ..bob()
    };
    service.peer_data(1, &stranger, "mumcord.p.sender", &[1, 1, 9, 0]);
    let counters = service.messaging_counters("sender");
    assert_eq!((counters.received, counters.incoming_refused), (1, 1));
    service.message_expired(1, "sender");
    assert_eq!(service.messaging_counters("sender").expired, 1);
}

/// The first message a Friends-only plugin sends at start: its receivers, or `None` when the send
/// failed (the plugin then backs off with `no-recipients`).
fn friends_only_send(
    service: &mut PluginService,
    host_events: &mut UnboundedReceiver<HostEvent>,
    events: &mut UnboundedReceiver<ServiceEvent>,
    root: &Path,
    id: &str,
) -> Option<Vec<u32>> {
    let dir = sender_folder(root, id);
    let friends_only = format!(r#"{MESSAGING},"messaging":{{"default_visibility":"friends"}}"#);
    std::fs::write(
        dir.join("manifest.json"),
        manifest(id, "0.1.0", &friends_only),
    )
    .expect("manifest");
    let id = service.load_dev(&dir).expect("loads");
    while let Ok(event) = host_events.try_recv() {
        service.handle_host_event(event).expect("handled");
    }
    let mut receivers = None;
    while let Ok(event) = events.try_recv() {
        if let ServiceEvent::PeerSend { outgoing, .. } = event {
            receivers = Some(outgoing.receivers);
        }
    }
    if receivers.is_none() {
        assert!(matches!(
            info(service, &id).status,
            Status::BackingOff { error } if error.ends_with(NO_RECIPIENTS)
        ));
    }
    receivers
}

#[test]
fn befriending_someone_already_in_the_channel_lets_friends_only_messages_reach_them() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, mut host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    // Bob is in the channel before he becomes a friend.
    service.peer_joined(1, bob());
    service.peer_data(
        1,
        &bob(),
        "mumcord.plugins",
        br#"{"v":1,"p":["before@1~dev","after@1~dev"]}"#,
    );
    let root = fx.root.path();
    assert_eq!(
        friends_only_send(&mut service, &mut host_events, &mut events, root, "before"),
        None
    );

    let bob_hash = bob().cert_hash.expect("hash");
    service.friends_changed(&HashSet::from([bob_hash]));
    assert_eq!(
        friends_only_send(&mut service, &mut host_events, &mut events, root, "after"),
        Some(vec![7])
    );
}

#[test]
fn visibility_nobody_leaves_a_plugin_out_of_the_announce() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, _host_events, _events) = fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    let id = service
        .load_dev(&sender_folder(fx.root.path(), "quiet"))
        .expect("loads");
    service
        .set_visibility(&id, PluginVisibility::Nobody)
        .expect("visibility");
    let announces = service.poll_announces();
    let text = String::from_utf8(announces[0].bytes.clone()).expect("JSON");
    assert!(!text.contains("quiet"), "{text}");
}

/// Core module: `start` sends "hi" six times, starts a game burst, sends six more and asks for a
/// second burst. It refuses to start with "1" if the first burst is refused and "2" if the
/// second is granted.
const BURSTER: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  (import "mumcord:plugin/peers@1.0.0" "start-game-burst" (func $burst (param i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 40) "hi")
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func $spam (local $i i32)
    (loop $again
      (call $send (i32.const 0) (i32.const 0) (i32.const 5) (i32.const 40) (i32.const 2)
        (i32.const 100))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $again (i32.lt_u (local.get $i) (i32.const 6)))))
  (func $fail (param $code i32) (result i32)
    (i32.store8 (i32.const 48) (local.get $code))
    (i32.store8 (i32.const 64) (i32.const 1))
    (i32.store (i32.const 68) (i32.const 48))
    (i32.store (i32.const 72) (i32.const 1))
    (i32.const 64))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (call $spam)
    (call $burst (i32.const 120))
    (if (i32.load8_u (i32.const 120)) (then (return (call $fail (i32.const 49)))))
    (call $spam)
    (call $burst (i32.const 120))
    (if (i32.eqz (i32.load8_u (i32.const 120))) (then (return (call $fail (i32.const 50)))))
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param i32))
  (func (export "stop"))
)"#;

#[test]
fn a_spamming_plugin_is_held_to_its_rate_and_one_game_burst_a_minute() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, _host_events, _events) = fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    service.peer_joined(1, bob());
    service.peer_data(
        1,
        &bob(),
        "mumcord.plugins",
        br#"{"v":1,"p":["burster@1~dev"]}"#,
    );
    let dir = fx.root.path().join("src-burster");
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(
        dir.join("manifest.json"),
        manifest("burster", "0.1.0", MESSAGING),
    )
    .expect("manifest");
    std::fs::write(dir.join("plugin.wasm"), encode(BURSTER)).expect("wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("i18n");
    let id = service.load_dev(&dir).expect("loads");
    assert_eq!(info(&service, &id).status, Status::Running);

    // Its own bucket lets 3 through; the game burst adds its 4, but all plugins together have
    // only 5 at once (FR-6.36). Everything else is refused and counted, never queued.
    let counters = service.messaging_counters("burster");
    assert_eq!((counters.sent, counters.rate_limited), (5, 7));
}

/// `not-permitted` in the WIT error enum.
const NOT_PERMITTED: &str = "0";

#[test]
fn revoking_optional_peers_messages_stops_sends_and_the_announce_at_once() {
    let fx = Fixture::new();
    // Messaging tells sources apart by the repository key, so the repository must exist.
    let repository_key = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let repository = fx
        .store
        .lock()
        .expect("lock")
        .add_plugin_repository(
            &NewPluginRepository {
                url: "https://plugins.example.test".into(),
                name: "Test".into(),
                public_key: repository_key.pk.to_base64(),
            },
            0,
        )
        .expect("repository");
    assert_eq!(
        Source::Repo {
            repository_id: repository
        },
        REPO
    );
    let tag: String = key_fingerprint_prefix(&repository_key.pk.to_base64())
        .expect("fingerprint")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let (mut service, _host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    service.peer_joined(1, bob());
    let bob_runs_poll = format!(r#"{{"v":1,"p":["poll@1~{tag}"]}}"#);
    service.peer_data(1, &bob(), "mumcord.plugins", bob_runs_poll.as_bytes());
    let mut poll = signed_wasm(
        "poll",
        &encode(SENDER),
        r#","permissions":{"required":["ui.panel"],"optional":["peers.messages"]}"#,
        &["peers.messages"],
    );
    poll.request.entry.permissions = vec!["ui.panel".into(), "peers.messages".into()];
    poll.request.visibility = PluginVisibility::Everyone;
    service
        .install(&poll.request, &poll.download)
        .expect("installs");

    // Granted: the send at start reaches Bob, and the announce lists Poll.
    assert_eq!(info(&service, "poll").status, Status::Running);
    assert_eq!(service.messaging_counters("poll").sent, 1);
    while events.try_recv().is_ok() {}
    let announces = service.poll_announces();
    let text = String::from_utf8(announces[0].bytes.clone()).expect("JSON");
    assert!(text.contains("poll@1"), "{text}");

    // Revoked at runtime (AC-6.11): sending is not permitted and Poll is no longer announced.
    service
        .set_permissions("poll", &["ui.panel".into()])
        .expect("optional revoked");
    service.set_enabled("poll", false).expect("turns off");
    // Poll's `start` sends and gives up on the error.
    let error = service
        .set_enabled("poll", true)
        .expect_err("start fails")
        .to_string();
    assert!(error.ends_with(NOT_PERMITTED), "{error}");
    // Server 1 announced less than 10 s ago; a server that newly needs an announce shows it.
    assert!(
        service.next_announce_at().is_some(),
        "the change is announced"
    );
    service.shared_state(2, true);
    service.peer_capable(2, 8, true);
    let announces = service.poll_announces();
    assert_eq!(announces.len(), 1);
    let text = String::from_utf8(announces[0].bytes.clone()).expect("JSON");
    assert!(!text.contains("poll"), "{text}");
    while let Ok(event) = events.try_recv() {
        assert!(
            !matches!(event, ServiceEvent::PeerSend { .. }),
            "no mumcord.p.poll goes out"
        );
    }
}

#[test]
fn peer_messages_come_only_from_the_voice_server_with_a_flood_guard_per_server() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, _host_events, _events) = fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    for server in [1, 2] {
        service.shared_state(server, true);
        service.peer_capable(server, 7, true);
        service.peer_joined(server, bob());
        service.peer_data(
            server,
            &bob(),
            "mumcord.plugins",
            br#"{"v":1,"p":["sender@1~dev"]}"#,
        );
    }
    service
        .load_dev(&sender_folder(fx.root.path(), "sender"))
        .expect("loads");

    // Chat events come from every server (H3), plugin messages only from the voice server.
    service.peer_data(2, &bob(), "mumcord.p.sender", &[1, 1, 9, 0]);
    assert_eq!(service.messaging_counters("sender").received, 0);
    // Bob floods: five per second pass, the rest is dropped and counted.
    for _ in 0..6 {
        service.peer_data(1, &bob(), "mumcord.p.sender", &[1, 1, 9, 0]);
    }
    let counters = service.messaging_counters("sender");
    assert_eq!((counters.received, counters.incoming_rate_dropped), (5, 1));
    // Voice moves to server 2: Bob there has his own window.
    service.set_focused_server(Some(2));
    for _ in 0..5 {
        service.peer_data(2, &bob(), "mumcord.p.sender", &[1, 1, 9, 0]);
    }
    service.peer_data(1, &bob(), "mumcord.p.sender", &[1, 1, 9, 0]);
    let counters = service.messaging_counters("sender");
    assert_eq!((counters.received, counters.incoming_rate_dropped), (10, 1));
}

/// On a `chat-trigger` event (case 19), sends its `args` to the channel as a kind-5 plugin
/// message, so the test can see what arrived. The event payload starts at offset 8 (the
/// variant holds a u64); in `chat-trigger`, `args` is the string at payload offset 32.
const TRIGGER_ECHO: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    ;; Round up to the requested alignment.
    (local.set $p (i32.and
      (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
      (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param $event i32)
    (if (i32.eq (i32.load8_u (local.get $event)) (i32.const 19))
      (then
        (call $send (i32.const 0) (i32.const 0) (i32.const 5)
          (i32.load (i32.add (local.get $event) (i32.const 40)))
          (i32.load (i32.add (local.get $event) (i32.const 44)))
          (i32.const 100)))))
  (func (export "stop"))
)"#;

fn trigger_folder(root: &Path, id: &str, extra: &str) -> PathBuf {
    let dir = root.join(format!("src-{id}"));
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(dir.join("manifest.json"), manifest(id, "0.1.0", extra)).expect("manifest");
    std::fs::write(dir.join("plugin.wasm"), encode(TRIGGER_ECHO)).expect("wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("i18n");
    dir
}

/// Plugin and payload of every plugin message sent within `wait`.
fn echoed(
    service: &mut PluginService,
    host_events: &mut UnboundedReceiver<HostEvent>,
    events: &mut UnboundedReceiver<ServiceEvent>,
    wait: std::time::Duration,
) -> Vec<(String, Vec<u8>)> {
    let until = std::time::Instant::now() + wait;
    let mut out = Vec::new();
    loop {
        while let Ok(event) = host_events.try_recv() {
            service.handle_host_event(event).expect("handled");
        }
        while let Ok(event) = events.try_recv() {
            if let ServiceEvent::PeerSend {
                plugin, outgoing, ..
            } = event
            {
                out.push((plugin, outgoing.bytes[3..].to_vec()));
            }
        }
        if std::time::Instant::now() >= until {
            return out;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn chat_triggers_reach_only_the_owner_and_only_from_my_channel() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, mut host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    service.peer_joined(1, bob());
    service.peer_data(
        1,
        &bob(),
        "mumcord.plugins",
        br#"{"v":1,"p":["poll@1~dev","late@1~dev","deaf@1~dev"]}"#,
    );
    service.self_changed(
        1,
        SelfInfo {
            me: PeerInfo {
                session: 1,
                name: "Me".into(),
                cert_hash: None,
                is_friend: false,
            },
            channel: Some((3, "Lobby".into())),
            muted: false,
            deafened: false,
        },
    );
    let triggers =
        r#","permissions":{"required":["peers.messages","chat.triggers"]},"triggers":["!poll"]"#;
    let poll = service
        .load_dev(&trigger_folder(fx.root.path(), "poll", triggers))
        .expect("loads");
    // Declares the same word later: the first plugin keeps it.
    let _late = service
        .load_dev(&trigger_folder(fx.root.path(), "late", triggers))
        .expect("loads");
    // Without the permission (and so without triggers) nothing reaches it.
    let _deaf = service
        .load_dev(&trigger_folder(
            fx.root.path(),
            "deaf",
            r#","permissions":{"required":["peers.messages"]}"#,
        ))
        .expect("loads");
    for id in [&poll, &_late, &_deaf] {
        assert_eq!(info(&service, id).status, Status::Running, "{id}");
    }

    // Another channel, another server or an unknown word: nothing.
    service.chat_trigger(1, &bob(), 4, "!poll", "1", None);
    service.chat_trigger(2, &bob(), 3, "!poll", "1", None);
    service.chat_trigger(1, &bob(), 3, "!dice", "1", None);
    // In my channel: the owner gets it; a third line within a second is dropped.
    service.chat_trigger(1, &bob(), 3, "!poll", "2", None);
    service.chat_trigger(1, &bob(), 3, "!poll", "3", None);
    service.chat_trigger(1, &bob(), 3, "!poll", "4", None);

    let wait = std::time::Duration::from_millis(500);
    assert_eq!(
        echoed(&mut service, &mut host_events, &mut events, wait),
        [(poll.clone(), b"2".to_vec()), (poll, b"3".to_vec())]
    );
}

/// On a `chat-match` event (case 20), sends its `text` to the channel as a plugin message of
/// kind 5 + `pattern-index`. In `chat-match`, `pattern-index` is the byte at payload offset 24
/// and `text` the string at payload offset 28 (the payload starts at offset 8).
const MATCH_ECHO: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (i32.and
      (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
      (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param $event i32)
    (if (i32.eq (i32.load8_u (local.get $event)) (i32.const 20))
      (then
        (call $send (i32.const 0) (i32.const 0)
          (i32.add (i32.const 5) (i32.load8_u (i32.add (local.get $event) (i32.const 32))))
          (i32.load (i32.add (local.get $event) (i32.const 36)))
          (i32.load (i32.add (local.get $event) (i32.const 40)))
          (i32.const 100)))))
  (func (export "stop"))
)"#;

fn match_folder(root: &Path, id: &str, extra: &str) -> PathBuf {
    let dir = root.join(format!("src-{id}"));
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(dir.join("manifest.json"), manifest(id, "0.1.0", extra)).expect("manifest");
    std::fs::write(dir.join("plugin.wasm"), encode(MATCH_ECHO)).expect("wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("i18n");
    dir
}

#[test]
fn chat_matches_reach_every_matching_plugin_and_only_from_my_channel() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, mut host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    service.peer_joined(1, bob());
    service.peer_data(
        1,
        &bob(),
        "mumcord.plugins",
        br#"{"v":1,"p":["fits@1~dev","cmds@1~dev","deaf@1~dev"]}"#,
    );
    service.self_changed(
        1,
        SelfInfo {
            me: PeerInfo {
                session: 1,
                name: "Me".into(),
                cert_hash: None,
                is_friend: false,
            },
            channel: Some((3, "Lobby".into())),
            muted: false,
            deafened: false,
        },
    );
    assert!(
        service.chat_pattern_set().is_none(),
        "no plugin listens yet"
    );
    let patterns = |list: &str| {
        format!(
            r#","permissions":{{"required":["peers.messages","chat.patterns"]}},"chat_patterns":{list}"#
        )
    };
    let fits = service
        .load_dev(&match_folder(
            fx.root.path(),
            "fits",
            &patterns(
                r#"[{"pattern":"^!fit","description":"Fit commands"},
                    {"pattern":"^\\[[^,\\n]+,[^\\]\\n]*\\]","description":"EVE fits"}]"#,
            ),
        ))
        .expect("loads");
    let cmds = service
        .load_dev(&match_folder(
            fx.root.path(),
            "cmds",
            &patterns(r#"[{"pattern":"^!","description":"Commands"}]"#),
        ))
        .expect("loads");
    // Without the permission (and so without patterns) nothing reaches it.
    let deaf = service
        .load_dev(&match_folder(
            fx.root.path(),
            "deaf",
            r#","permissions":{"required":["peers.messages"]}"#,
        ))
        .expect("loads");
    for id in [&fits, &cmds, &deaf] {
        assert_eq!(info(&service, id).status, Status::Running, "{id}");
    }
    let set = service.chat_pattern_set().expect("patterns");
    assert!(set.is_match("[Rifter, PvP]\nx") && set.is_match("!x") && !set.is_match("hi"));

    // Another channel, another server or no pattern matches: nothing.
    service.chat_match(1, &bob(), 4, "[Rifter, a]", None);
    service.chat_match(2, &bob(), 3, "[Rifter, a]", None);
    service.chat_match(1, &bob(), 3, "hello", None);
    // In my channel: every matching plugin, with its first matching pattern.
    service.chat_match(1, &bob(), 3, "[Rifter, a]", None);
    service.chat_match(1, &bob(), 3, "!fit b", None);
    service.chat_match(1, &bob(), 3, "!x c", None);
    // A fourth match within 10 seconds is dropped.
    service.chat_match(1, &bob(), 3, "!fit d", None);

    let wait = std::time::Duration::from_millis(500);
    let mut got: Vec<(String, u8, Vec<u8>)> =
        sent(&mut service, &mut host_events, &mut events, wait);
    got.sort();
    assert_eq!(
        got,
        [
            (cmds.clone(), 5, b"!fit b".to_vec()),
            (cmds, 5, b"!x c".to_vec()),
            (fits.clone(), 5, b"!fit b".to_vec()),
            (fits, 6, b"[Rifter, a]".to_vec()),
        ]
    );
}

/// Like [`MATCH_ECHO`], but imports `chat.is-replay` (so it takes replays): on a `chat-match`
/// it hides the message behind it (`chat.current-message` + `chat.hide`) and sends the text as
/// kind 5 (live) or 6 (replay).
const REPLAY_ECHO: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  (import "mumcord:plugin/chat@1.0.0" "current-message" (func $current (param i32)))
  (import "mumcord:plugin/chat@1.0.0" "is-replay" (func $replay (result i32)))
  (import "mumcord:plugin/chat@1.0.0" "hide" (func $hide (param i64 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (i32.and
      (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
      (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param $event i32)
    (if (i32.eq (i32.load8_u (local.get $event)) (i32.const 20))
      (then
        (call $current (i32.const 256))
        (if (i32.eq (i32.load8_u (i32.const 256)) (i32.const 1))
          (then (call $hide (i64.load (i32.const 264)) (i32.const 288))))
        (call $send (i32.const 0) (i32.const 0)
          (i32.add (i32.const 5) (call $replay))
          (i32.load (i32.add (local.get $event) (i32.const 36)))
          (i32.load (i32.add (local.get $event) (i32.const 40)))
          (i32.const 100)))))
  (func (export "stop"))
)"#;

/// Plugin messages (plugin, kind, payload) and hidden messages (plugin, message) within `wait`.
type Seen = (Vec<(String, u8, Vec<u8>)>, Vec<(String, u64)>);

fn sent_and_hidden(
    service: &mut PluginService,
    host_events: &mut UnboundedReceiver<HostEvent>,
    events: &mut UnboundedReceiver<ServiceEvent>,
    wait: std::time::Duration,
) -> Seen {
    let until = std::time::Instant::now() + wait;
    let (mut sent, mut hidden) = (Vec::new(), Vec::new());
    loop {
        while let Ok(event) = host_events.try_recv() {
            service.handle_host_event(event).expect("handled");
        }
        while let Ok(event) = events.try_recv() {
            match event {
                ServiceEvent::PeerSend {
                    plugin, outgoing, ..
                } => sent.push((plugin, outgoing.bytes[2], outgoing.bytes[3..].to_vec())),
                ServiceEvent::Supersede {
                    plugin,
                    card: None,
                    message,
                } => hidden.push((plugin, message)),
                _ => {}
            }
        }
        if std::time::Instant::now() >= until {
            sent.sort();
            hidden.sort();
            return (sent, hidden);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn plugins_hide_only_their_messages_and_replays_reach_only_replay_aware_plugins() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, mut host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    service.peer_joined(1, bob());
    service.peer_data(
        1,
        &bob(),
        "mumcord.plugins",
        br#"{"v":1,"p":["fits@1~dev","old@1~dev"]}"#,
    );
    service.self_changed(
        1,
        SelfInfo {
            me: PeerInfo {
                session: 1,
                name: "Me".into(),
                cert_hash: None,
                is_friend: false,
            },
            channel: Some((3, "Lobby".into())),
            muted: false,
            deafened: false,
        },
    );
    let extra = r#","permissions":{"required":["peers.messages","chat.patterns"]},"chat_patterns":[{"pattern":"^\\[","description":"Fits"}]"#;
    let fits_dir = match_folder(fx.root.path(), "fits", extra);
    std::fs::write(fits_dir.join("plugin.wasm"), encode(REPLAY_ECHO)).expect("wasm");
    let fits = service.load_dev(&fits_dir).expect("loads");
    // Built before replays: never gets one.
    let old = service
        .load_dev(&match_folder(fx.root.path(), "old", extra))
        .expect("loads");
    let wait = std::time::Duration::from_millis(500);

    // Live: both get it; the replay-aware plugin hides the message it was handed.
    service.chat_match(1, &bob(), 3, "[Rifter, a]", Some(41));
    let (sent, hidden) = sent_and_hidden(&mut service, &mut host_events, &mut events, wait);
    assert_eq!(
        sent,
        [
            (fits.clone(), 5, b"[Rifter, a]".to_vec()),
            (old.clone(), 5, b"[Rifter, a]".to_vec()),
        ]
    );
    assert_eq!(hidden, [(fits.clone(), 41)]);

    // Replay: only the replay-aware plugin, flagged as replay, and never message 41 again.
    let line = |message, text: &str| StoredLine {
        message,
        sender: PeerInfo {
            session: 0,
            ..bob()
        },
        text: text.into(),
        trigger: None,
    };
    let lines = [
        line(41, "[Rifter, a]"),
        line(42, "[Rifter, b]"),
        line(43, "hi"),
    ];
    service.replay_chat(1, 4, &lines);
    service.replay_chat(1, 3, &lines);
    let (sent, hidden) = sent_and_hidden(&mut service, &mut host_events, &mut events, wait);
    assert_eq!(sent, [(fits.clone(), 6, b"[Rifter, b]".to_vec())]);
    assert_eq!(hidden, [(fits.clone(), 42)]);

    // A second replay of the same history sends nothing.
    service.replay_chat(1, 3, &lines);
    let (sent, hidden) = sent_and_hidden(&mut service, &mut host_events, &mut events, wait);
    assert!(sent.is_empty() && hidden.is_empty(), "{sent:?} {hidden:?}");
}

/// Plugin, message kind and payload of every plugin message sent within `wait`.
fn sent(
    service: &mut PluginService,
    host_events: &mut UnboundedReceiver<HostEvent>,
    events: &mut UnboundedReceiver<ServiceEvent>,
    wait: std::time::Duration,
) -> Vec<(String, u8, Vec<u8>)> {
    let until = std::time::Instant::now() + wait;
    let mut out = Vec::new();
    loop {
        while let Ok(event) = host_events.try_recv() {
            service.handle_host_event(event).expect("handled");
        }
        while let Ok(event) = events.try_recv() {
            if let ServiceEvent::PeerSend {
                plugin, outgoing, ..
            } = event
            {
                out.push((plugin, outgoing.bytes[2], outgoing.bytes[3..].to_vec()));
            }
        }
        if std::time::Instant::now() >= until {
            return out;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn me_in(channel: u32) -> SelfInfo {
    SelfInfo {
        me: PeerInfo {
            session: 1,
            name: "Me".into(),
            cert_hash: None,
            is_friend: false,
        },
        channel: Some((channel, "Lobby".into())),
        muted: false,
        deafened: false,
    }
}

#[test]
fn chat_matches_come_from_every_connected_server() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, mut host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    // Voice on server 1 (the echo goes out there), also connected to server 2.
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    service.peer_joined(1, bob());
    service.peer_data(
        1,
        &bob(),
        "mumcord.plugins",
        br#"{"v":1,"p":["cmds@1~dev"]}"#,
    );
    service.self_changed(1, me_in(3));
    service.self_changed(2, me_in(5));
    let cmds = service
        .load_dev(&match_folder(
            fx.root.path(),
            "cmds",
            r#","permissions":{"required":["peers.messages","chat.patterns"]},"chat_patterns":[{"pattern":"^!","description":"Commands"}]"#,
        ))
        .expect("loads");
    assert_eq!(info(&service, &cmds).status, Status::Running);

    // Server 2: only the user's channel there (5), not the voice server's channel number.
    service.chat_match(2, &bob(), 3, "!wrong", None);
    service.chat_match(2, &bob(), 5, "!a", None);
    service.chat_match(1, &bob(), 3, "!b", None);
    // Not connected to server 3: nothing.
    service.chat_match(3, &bob(), 5, "!none", None);

    let wait = std::time::Duration::from_millis(500);
    let mut got: Vec<Vec<u8>> = sent(&mut service, &mut host_events, &mut events, wait)
        .into_iter()
        .map(|(_, _, text)| text)
        .collect();
    got.sort();
    assert_eq!(got, [b"!a".to_vec(), b"!b".to_vec()]);
}

/// What the safety fixture builder puts in place of `{{PRELUDE}}` (memory, allocator, `stop`,
/// `$start_ok`, `$log_info`), so the real fixture core runs here without the builder.
const FIXTURE_PRELUDE: &str = r#"
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (func (export "cabi_realloc") (param $old i32) (param $old_size i32) (param $align i32) (param $size i32) (result i32)
    (local $p i32)
    (local.set $p
      (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
        (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
    (global.set $heap (i32.add (local.get $p) (local.get $size)))
    (local.get $p))
  (func (export "stop"))
  (func $start_ok (result i32)
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func $log_info (param $ptr i32) (param $len i32)
    (call $log (i32.const 1) (local.get $ptr) (local.get $len)))
"#;

/// AC-6.7: the Dice-like safety fixture declares everything Dice declares (incl. `chat.send`) and
/// posts to chat at start, which is no user gesture: it gets `not-permitted` and nothing is
/// posted, although it is connected and focused.
#[test]
fn a_dice_like_plugin_cannot_post_to_chat_without_a_gesture() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, mut host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);

    let declared: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/safety/fx-dice-like/manifest.json"))
            .expect("fixture manifest");
    let extra: String = ["permissions", "messaging", "commands"]
        .iter()
        .map(|key| format!(r#","{key}":{}"#, declared[key]))
        .collect();
    let core =
        include_str!("fixtures/safety/cores/chat-send.wat").replace("{{PRELUDE}}", FIXTURE_PRELUDE);
    let dir = fx.root.path().join("src-fx-dice-like");
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(
        dir.join("manifest.json"),
        manifest("fx-dice-like", "0.1.0", &extra),
    )
    .expect("manifest");
    std::fs::write(dir.join("plugin.wasm"), encode(&core)).expect("wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("i18n");
    let id = service.load_dev(&dir).expect("loads");
    assert_eq!(info(&service, &id).status, Status::Running);

    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut posted = Vec::new();
    let outcome = loop {
        while let Ok(event) = host_events.try_recv() {
            service.handle_host_event(event).expect("handled");
        }
        while let Ok(event) = events.try_recv() {
            if let ServiceEvent::ChatPost { line, .. } = event {
                posted.push(line);
            }
        }
        let lines = service.log_lines(&id);
        if let Some((_, line)) = lines.iter().find(|(_, l)| l.starts_with("chat-send")) {
            break line.clone();
        }
        assert!(
            std::time::Instant::now() < until,
            "no chat-send outcome: {lines:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(outcome, "chat-send-not-permitted");
    assert!(posted.is_empty(), "{posted:?}");
}

/// On a `peer-ready` event (case 21), sends the peer's name to the channel as a plugin message
/// of kind 5. In `peer`, `name` is the string at payload offset 4 (the payload starts at
/// offset 8).
const READY_ECHO: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (i32.and
      (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
      (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param $event i32)
    (if (i32.eq (i32.load8_u (local.get $event)) (i32.const 21))
      (then
        (call $send (i32.const 0) (i32.const 0) (i32.const 5)
          (i32.load (i32.add (local.get $event) (i32.const 12)))
          (i32.load (i32.add (local.get $event) (i32.const 16)))
          (i32.const 100)))))
  (func (export "stop"))
)"#;

fn ready_folder(root: &Path, id: &str) -> PathBuf {
    let dir = root.join(format!("src-{id}"));
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(dir.join("manifest.json"), manifest(id, "0.1.0", MESSAGING)).expect("manifest");
    std::fs::write(dir.join("plugin.wasm"), encode(READY_ECHO)).expect("wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("i18n");
    dir
}

#[test]
fn a_peer_newly_announcing_a_plugin_makes_it_ready_once() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, mut host_events, mut events) =
        fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    service.peer_joined(1, bob());
    let poll = service
        .load_dev(&ready_folder(fx.root.path(), "poll"))
        .expect("loads");
    let _dice = service
        .load_dev(&ready_folder(fx.root.path(), "dice"))
        .expect("loads");
    let wait = std::time::Duration::from_millis(300);
    assert!(echoed(&mut service, &mut host_events, &mut events, wait).is_empty());

    // Bob installs Poll: only Poll hears that he is ready, and only the first time.
    let runs_poll = br#"{"v":1,"p":["poll@1~dev"]}"#;
    service.peer_data(1, &bob(), "mumcord.plugins", runs_poll);
    service.peer_data(1, &bob(), "mumcord.plugins", runs_poll);
    // Someone outside the channel announcing Poll is nobody Poll can reach.
    let eve = PeerInfo {
        session: 9,
        name: "Eve".into(),
        ..bob()
    };
    service.peer_data(1, &eve, "mumcord.plugins", runs_poll);
    assert_eq!(
        echoed(&mut service, &mut host_events, &mut events, wait),
        [(poll, b"Bob".to_vec())]
    );
}

#[test]
fn installing_a_plugin_announces_at_once_despite_the_interval() {
    let fx = Fixture::new();
    fx.set_developer_mode(true);
    let (mut service, _host_events, _events) = fx.service_with_host(Version::new(0, 7, 0), false);
    service.set_focused_server(Some(1));
    service.shared_state(1, true);
    service.peer_capable(1, 7, true);
    let first = service
        .load_dev(&ready_folder(fx.root.path(), "first"))
        .expect("loads");
    assert_eq!(service.poll_announces().len(), 1);

    service
        .load_dev(&ready_folder(fx.root.path(), "second"))
        .expect("loads");
    let announces = service.poll_announces();
    assert_eq!(announces.len(), 1, "a new plugin skips the 10 s interval");
    let text = String::from_utf8(announces[0].bytes.clone()).expect("JSON");
    assert!(text.contains("second@1~dev"), "{text}");

    // Hiding a plugin is no gain: it waits for the interval.
    service
        .set_visibility(&first, PluginVisibility::Nobody)
        .expect("visibility");
    assert!(service.poll_announces().is_empty());
    assert!(service.next_announce_at().is_some());
}
