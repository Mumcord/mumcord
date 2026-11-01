//! The plugin service end to end with real (tiny) plugins, a real store and packages signed
//! with keys generated per test run: install, restore after remove, permission revokes,
//! Turn off all, `--no-plugins`, developer folders, badge default and the integrity re-check.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use minisign::KeyPair;
use mumcord_plugins::Digest;
use mumcord_plugins::Source;
use mumcord_plugins::registry::Download;
use mumcord_plugins::service::{
    InstallRequest, InstallStage, PluginInfo, PluginService, ServiceConfig, ServiceError,
    ServiceEvent, Status,
};
use mumcord_plugins::verify::IndexEntry;
use mumcord_store::{PluginVisibility, Store};
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
    let author = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let bytes = package(&manifest(id, "1.0.0", extra), &component(start));
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
        let config = ServiceConfig {
            plugins_dir: self.plugins_dir(),
            cache_dir: None,
            app_version,
            no_plugins,
        };
        let (mut service, _host_events, events) =
            PluginService::new(config, Arc::clone(&self.store), None).expect("service builds");
        service.start().expect("service starts");
        (service, events)
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
