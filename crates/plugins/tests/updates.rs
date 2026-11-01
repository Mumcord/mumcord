//! Plugin updates and the kill switch with real (tiny) plugins, a real store and packages
//! signed with keys generated per test run (FR-6.66–6.70, AC-6.19, AC-6.20).

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use minisign::KeyPair;
use mumcord_plugins::registry::{Download, Index, IndexKind, RevocationList, Severity};
use mumcord_plugins::service::{
    Approval, InstallRequest, PluginInfo, PluginService, ServiceConfig, ServiceError, ServiceEvent,
    Status,
};
use mumcord_plugins::verify::IndexEntry;
use mumcord_plugins::{Digest, Source};
use mumcord_store::{PluginUpdates, PluginVisibility, Store};
use semver::Version;
use serde_json::json;
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
const PERMISSIONS: &[&str] = &["ui.panel", "events.voice"];
const MORE_PERMISSIONS: &[&str] = &["ui.panel", "events.voice", "chat.send"];

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

/// `ui.panel` is required, everything else optional.
fn manifest(id: &str, version: &str, permissions: &[&str]) -> String {
    let optional: Vec<_> = permissions[1..]
        .iter()
        .map(|p| json!({ "id": p }))
        .collect();
    let permissions = json!({"required": [permissions[0]], "optional": optional});
    format!(
        r#"{{"manifest":1,"id":"{id}","name":"Test","version":"{version}","api":"^1.0","authors":["Tests"],
           "description":"A test plugin","categories":["tools"],"license":"MIT","permissions":{permissions}}}"#
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

/// One signed package version as a repository publishes it.
struct Release {
    id: String,
    version: String,
    permissions: Vec<String>,
    author: String,
    download: Download,
}

impl Release {
    fn new(id: &str, version: &str, start: &str, permissions: &[&str], key: &KeyPair) -> Self {
        let bytes = package(&manifest(id, version, permissions), &component(start));
        let signature = minisign::sign(Some(&key.pk), &key.sk, &bytes[..], None, None)
            .expect("sign")
            .into_string();
        Self {
            id: id.into(),
            version: version.into(),
            permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
            author: key.pk.to_base64(),
            download: Download { bytes, signature },
        }
    }

    fn sha256(&self) -> String {
        Digest::of(&self.download.bytes).sha256_hex()
    }

    fn request(&self, optional: &[&str]) -> InstallRequest {
        InstallRequest {
            source: REPO,
            entry: IndexEntry {
                id: self.id.clone(),
                version: self.version.clone(),
                digest: Digest::of(&self.download.bytes),
                author_keys: vec![self.author.clone()],
                permissions: self.permissions.clone(),
            },
            optional: optional.iter().map(|p| (*p).to_owned()).collect(),
            visibility: PluginVisibility::Friends,
        }
    }
}

/// A signed-and-checked index listing `releases` (all of one plugin per id).
fn index(releases: &[&Release]) -> Vec<(Source, Index)> {
    let mut plugins: Vec<serde_json::Value> = Vec::new();
    for release in releases {
        let version = json!({
            "version": release.version, "api": 1,
            "url": format!("https://example.org/{}-{}.mcplugin", release.id, release.version),
            "size": release.download.bytes.len(), "sha256": release.sha256(),
            "permissions": release.permissions,
        });
        match plugins.iter_mut().find(|p| p["id"] == release.id) {
            Some(plugin) => {
                plugin["versions"]
                    .as_array_mut()
                    .expect("versions")
                    .push(version);
                let keys = plugin["author_keys"].as_array_mut().expect("keys");
                if !keys.contains(&json!(release.author)) {
                    keys.push(json!(release.author));
                }
            }
            None => plugins.push(json!({
                "id": release.id, "name": {"en": "Test"}, "summary": {"en": "A test"},
                "author_keys": [release.author], "reviewed": true, "updated": "2026-10-01",
                "versions": [version],
            })),
        }
    }
    let bytes = serde_json::to_vec(&json!({"format": 1, "sequence": 1, "plugins": plugins}))
        .expect("index json");
    vec![(
        REPO,
        Index::parse(&bytes, &IndexKind::Official).expect("index parses"),
    )]
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

    fn service(&self) -> (PluginService, UnboundedReceiver<ServiceEvent>) {
        let config = ServiceConfig {
            plugins_dir: self.root.path().join("plugins"),
            cache_dir: None,
            app_version: Version::new(0, 7, 0),
            no_plugins: false,
        };
        let (mut service, _host_events, events) =
            PluginService::new(config, Arc::clone(&self.store), None).expect("service builds");
        service.start().expect("service starts");
        (service, events)
    }

    fn set_kv(&self, id: &str) {
        self.store
            .lock()
            .expect("lock")
            .plugin_kv_set(id, "global", "k", b"v", 1024, 0)
            .expect("kv write");
    }

    fn kv(&self, id: &str) -> Option<Vec<u8>> {
        self.store
            .lock()
            .expect("lock")
            .plugin_kv_get(id, "global", "k")
            .expect("kv read")
    }

    fn version_dir(&self, id: &str, version: &str) -> std::path::PathBuf {
        self.root.path().join("plugins").join(id).join(version)
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

fn announced(events: &mut UnboundedReceiver<ServiceEvent>) -> Vec<(String, String)> {
    let mut found = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let ServiceEvent::UpdateAvailable { id, version, .. } = event {
            found.push((id, version));
        }
    }
    found
}

fn installed(fx: &Fixture, release: &Release) -> (PluginService, UnboundedReceiver<ServiceEvent>) {
    let (mut service, events) = fx.service();
    service
        .install(&release.request(&["events.voice"]), &release.download)
        .expect("installs");
    fx.set_kv(&release.id);
    (service, events)
}

#[test]
fn same_permissions_and_key_update_by_themselves_and_keep_data() {
    let fx = Fixture::new();
    let key = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let v1 = Release::new("poll", "1.0.0", OK_START, PERMISSIONS, &key);
    let v11 = Release::new("poll", "1.1.0", OK_START, PERMISSIONS, &key);
    let (mut service, mut events) = installed(&fx, &v1);

    let automatic = service
        .plan_updates(&index(&[&v1, &v11]), PluginUpdates::Auto)
        .expect("plan");
    assert_eq!(automatic.len(), 1);
    assert_eq!(automatic[0].to, "1.1.0");
    assert!(
        announced(&mut events).is_empty(),
        "automatic updates need no notification"
    );
    service
        .apply_update("poll", &v11.download, None)
        .expect("updates");

    let plugin = info(&service, "poll");
    assert_eq!(plugin.version, "1.1.0");
    assert_eq!(plugin.status, Status::Running);
    assert_eq!(plugin.granted, ["ui.panel", "events.voice"]);
    assert_eq!(fx.kv("poll").as_deref(), Some(b"v".as_slice()));
    assert!(service.updates().is_empty());

    // Notify only: listed and announced once, never installed by itself.
    let v12 = Release::new("poll", "1.2.0", OK_START, PERMISSIONS, &key);
    for _ in 0..2 {
        let automatic = service
            .plan_updates(&index(&[&v11, &v12]), PluginUpdates::Notify)
            .expect("plan");
        assert!(automatic.is_empty());
    }
    assert_eq!(service.updates().len(), 1);
    assert_eq!(announced(&mut events), [("poll".into(), "1.2.0".into())]);
    // Off: nothing on offer.
    service
        .plan_updates(&index(&[&v11, &v12]), PluginUpdates::Off)
        .expect("plan");
    assert!(service.updates().is_empty());
}

#[test]
fn new_permissions_or_a_new_key_wait_for_approval() {
    let fx = Fixture::new();
    let key = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let v1 = Release::new("poll", "1.0.0", OK_START, PERMISSIONS, &key);
    let v12 = Release::new("poll", "1.2.0", OK_START, MORE_PERMISSIONS, &key);
    let (mut service, mut events) = installed(&fx, &v1);

    let automatic = service
        .plan_updates(&index(&[&v1, &v12]), PluginUpdates::Auto)
        .expect("plan");
    assert!(automatic.is_empty());
    let update = &service.updates()[0];
    assert_eq!(
        update.approval,
        Some(Approval::NewPermissions(vec!["chat.send".into()]))
    );
    assert_eq!(announced(&mut events), [("poll".into(), "1.2.0".into())]);

    service
        .apply_update("poll", &v12.download, Some(&["chat.send".into()]))
        .expect("approved update installs");
    let plugin = info(&service, "poll");
    assert_eq!(plugin.version, "1.2.0");
    assert_eq!(plugin.granted, ["ui.panel", "chat.send"]);
    assert_eq!(fx.kv("poll").as_deref(), Some(b"v".as_slice()));

    // A different author key: the index lists both keys, but the installed one did not sign it.
    let other = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let v13 = Release::new("poll", "1.3.0", OK_START, MORE_PERMISSIONS, &other);
    service
        .plan_updates(&index(&[&v12, &v13]), PluginUpdates::Auto)
        .expect("plan");
    assert!(matches!(
        service.apply_update("poll", &v13.download, None),
        Err(ServiceError::NeedsApproval)
    ));
    assert_eq!(service.updates()[0].approval, Some(Approval::NewAuthorKey));
    assert_eq!(info(&service, "poll").version, "1.2.0");
}

#[test]
fn a_pinned_plugin_is_not_offered_updates() {
    let fx = Fixture::new();
    let key = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let v1 = Release::new("dice", "1.0.0", OK_START, PERMISSIONS, &key);
    let v11 = Release::new("dice", "1.1.0", OK_START, PERMISSIONS, &key);
    let (mut service, _events) = installed(&fx, &v1);

    service
        .plan_updates(&index(&[&v1, &v11]), PluginUpdates::Notify)
        .expect("plan");
    service.set_pinned("dice", true).expect("pins");
    assert!(service.updates().is_empty());
    let automatic = service
        .plan_updates(&index(&[&v1, &v11]), PluginUpdates::Auto)
        .expect("plan");
    assert!(automatic.is_empty());
    assert!(service.updates().is_empty());
    assert!(info(&service, "dice").pinned);
}

#[test]
fn a_version_that_fails_to_start_is_rolled_back_with_its_reason() {
    let fx = Fixture::new();
    let key = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let v1 = Release::new("poll", "1.0.0", OK_START, PERMISSIONS, &key);
    let broken = Release::new("poll", "1.1.0", FAILING_START, PERMISSIONS, &key);
    let (mut service, _events) = installed(&fx, &v1);

    service
        .plan_updates(&index(&[&v1, &broken]), PluginUpdates::Auto)
        .expect("plan");
    assert!(
        service
            .apply_update("poll", &broken.download, None)
            .is_err()
    );

    let plugin = info(&service, "poll");
    assert_eq!(plugin.version, "1.0.0");
    assert_eq!(plugin.status, Status::Running);
    assert_eq!(fx.kv("poll").as_deref(), Some(b"v".as_slice()));
    assert!(!fx.version_dir("poll", "1.1.0").exists());
    let reason = service.updates()[0].failed.clone().expect("reason shown");
    assert!(reason.contains("nope"), "{reason}");

    // Not retried by itself.
    let automatic = service
        .plan_updates(&index(&[&v1, &broken]), PluginUpdates::Auto)
        .expect("plan");
    assert!(automatic.is_empty());
    assert!(service.updates()[0].failed.is_some());
}

fn revocations(entries: serde_json::Value) -> RevocationList {
    RevocationList::parse(
        &serde_json::to_vec(&json!({"format": 1, "sequence": 9, "revoked": entries}))
            .expect("json"),
    )
    .expect("list parses")
}

#[test]
fn a_disable_revocation_turns_the_plugin_off_and_keeps_its_data() {
    let fx = Fixture::new();
    let key = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let v1 = Release::new("echolings", "1.0.0", OK_START, PERMISSIONS, &key);
    let (mut service, _events) = installed(&fx, &v1);

    service
        .apply_revocations(revocations(json!([
            {"id": "echolings", "versions": ["1.0.0"], "severity": "disable",
             "reason": {"en": "Leaks data", "de": "Gibt Daten preis"}, "since": "2026-10-01"}
        ])))
        .expect("applies");

    let plugin = info(&service, "echolings");
    assert_eq!(plugin.status, Status::Off);
    assert!(!plugin.enabled);
    assert_eq!(plugin.disabled_reason.as_deref(), Some("revoked"));
    let notice = plugin.revocation.expect("notice");
    assert_eq!(notice.severity, Severity::Disable);
    assert_eq!(notice.text(), "Turned off by Mumcord: Leaks data");
    assert_eq!(fx.kv("echolings").as_deref(), Some(b"v".as_slice()));
    assert!(matches!(
        service.set_enabled("echolings", true),
        Err(ServiceError::Revoked(_))
    ));

    // The revoked version is not offered; a fixed one is, and may run again.
    let v11 = Release::new("echolings", "1.1.0", OK_START, PERMISSIONS, &key);
    service
        .plan_updates(&index(&[&v1, &v11]), PluginUpdates::Auto)
        .expect("plan");
    service
        .apply_update("echolings", &v11.download, None)
        .expect("updates");
    let plugin = info(&service, "echolings");
    assert_eq!(plugin.revocation, None);
    assert_eq!(plugin.disabled_reason, None);
    service.set_enabled("echolings", true).expect("turns on");
    assert_eq!(info(&service, "echolings").status, Status::Running);
}

#[test]
fn a_warn_revocation_only_shows_and_revoked_versions_are_not_offered() {
    let fx = Fixture::new();
    let key = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let v1 = Release::new("dice", "1.0.0", OK_START, PERMISSIONS, &key);
    let v11 = Release::new("dice", "1.1.0", OK_START, PERMISSIONS, &key);
    let (mut service, _events) = installed(&fx, &v1);

    service
        .apply_revocations(revocations(json!([
            {"id": "dice", "versions": ["1.0.0"], "severity": "warn",
             "reason": {"en": "Outdated"}, "since": "2026-10-01"},
            {"id": "dice", "sha256": [v11.sha256()], "severity": "disable",
             "reason": {"en": "Broken"}, "since": "2026-10-02"}
        ])))
        .expect("applies");

    let plugin = info(&service, "dice");
    assert_eq!(plugin.status, Status::Running);
    assert_eq!(
        plugin.revocation.map(|n| (n.severity, n.text())),
        Some((Severity::Warn, "Outdated".into()))
    );
    service
        .plan_updates(&index(&[&v1, &v11]), PluginUpdates::Auto)
        .expect("plan");
    assert!(service.updates().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offline_check_applies_the_bundled_list_and_retries_later() {
    use mumcord_plugins::registry::{Client, UrlPolicy};
    use mumcord_plugins::service::{Schedule, run_check};

    let fx = Fixture::new();
    {
        let store = fx.store.lock().expect("lock");
        let mut settings = store.settings().expect("settings");
        settings.plugins_updates = PluginUpdates::Notify;
        // Nothing listens here: every fetch is offline.
        settings.plugins_mirror_url = Some("http://127.0.0.1:9/".into());
        store.save_settings(&settings).expect("save");
    }
    let (service, _events) = fx.service();
    let service = Arc::new(Mutex::new(service));
    let client = Client::new(UrlPolicy::new(true)).expect("client");
    let mut schedule = Schedule::new();
    run_check(
        &client,
        &service,
        &mut schedule,
        &fx.root.path().join("registry"),
    )
    .await
    .expect("check runs");

    let store = fx.store.lock().expect("lock");
    let settings = store.settings().expect("settings");
    assert_eq!(
        settings.plugins_revoked_checked_at, None,
        "an offline fetch is retried"
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let due = schedule.due(i64::try_from(now).expect("millis"), &settings);
    assert!(due.revocations && !due.updates);
}
