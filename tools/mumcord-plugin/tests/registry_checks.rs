//! The client's background check (`run_check`) against the real `serve-registry` fixture
//! registry (AC-6.19, AC-6.20): automatic updates, updates that ask, pinning, rollback of a
//! version that fails to start, and the daily revocation check with updates switched off.
//!
//! Both tests trust one test key through `MUMCORD_PLUGIN_REGISTRY_KEY`, set exactly once before
//! anything else in this binary reads the environment (see [`shared`]).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use mumcord_plugins::Source;
use mumcord_plugins::registry::{Client, IndexKind, UrlPolicy};
use mumcord_plugins::service::{
    InstallRequest, PluginInfo, PluginService, Schedule, ServiceConfig, Status, run_check,
};
use mumcord_store::{PluginUpdates, PluginVisibility, Store};
use semver::Version;
use zip::write::SimpleFileOptions;

const OK_START: &str = "(i32.store8 (i32.const 64) (i32.const 0)) (i32.const 64)";
/// Returns `err("nope")` from `start`.
const FAILING_START: &str = "
    (i32.store8 (i32.const 64) (i32.const 1))
    (i32.store (i32.const 68) (i32.const 32))
    (i32.store (i32.const 72) (i32.const 4))
    (i32.const 64)";
const PERMISSIONS: &[&str] = &["ui.panel", "events.voice"];
/// `chat.send` becomes required (the first entry is the required one).
const MORE_PERMISSIONS: &[&str] = &["chat.send", "ui.panel", "events.voice"];

struct Shared {
    _dir: tempfile::TempDir,
    key: PathBuf,
}

/// Generates the registry test key and points the client at it. `OnceLock` makes every other
/// test thread wait until the environment is written, so nothing reads it concurrently.
fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| {
        let dir = tempfile::tempdir().expect("temp dir");
        let out = cli(&[
            "keygen",
            "--unencrypted",
            "--out",
            dir.path().to_str().expect("utf-8"),
            "--name",
            "registry",
        ]);
        assert!(out.status.success(), "keygen failed");
        let public = std::fs::read_to_string(dir.path().join("registry.pub")).expect("public key");
        let public = public
            .lines()
            .rfind(|l| !l.trim().is_empty())
            .expect("key line")
            .trim()
            .to_owned();
        // SAFETY: runs once, before this binary starts runtimes or reads the environment; the
        // other test waits in `get_or_init` meanwhile.
        unsafe {
            std::env::set_var("MUMCORD_PLUGIN_REGISTRY_KEY", public);
            std::env::set_var("MUMCORD_PLUGIN_TEST_FORCE_DAILY_CHECK", "1");
        }
        Shared {
            key: dir.path().join("registry.key"),
            _dir: dir,
        }
    })
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mumcord-plugin"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn component(start: &str) -> Vec<u8> {
    let core = include_str!("../../../crates/plugins/tests/fixtures/plugin.wat")
        .replace("{{START}}", start)
        .replace("{{EVENT}}", "")
        .replace("{{STOP}}", "");
    let mut module = wat::parse_str(core).expect("fixture is valid WAT");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/plugins/wit"))
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

/// An unsigned package; `serve-registry` signs it as its author. The first permission is
/// required, the others optional.
fn package(id: &str, version: &str, start: &str, permissions: &[&str]) -> Vec<u8> {
    let optional: Vec<_> = permissions[1..]
        .iter()
        .map(|p| serde_json::json!({ "id": p }))
        .collect();
    let manifest = serde_json::json!({
        "manifest": 1, "id": id, "name": "Test", "version": version, "api": "^1.0",
        "authors": ["Tests"], "description": "A test plugin", "categories": ["tools"],
        "license": "MIT",
        "permissions": {"required": [permissions[0]], "optional": optional},
    })
    .to_string();
    let wasm = component(start);
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("manifest.json", manifest.as_bytes()),
        ("plugin.wasm", wasm.as_slice()),
        ("i18n/en.json", b"{}".as_slice()),
        ("README.md", b"# Test".as_slice()),
        ("LICENSE", b"MIT".as_slice()),
    ] {
        writer.start_file(name, options).expect("start file");
        writer.write_all(data).expect("write");
    }
    writer.finish().expect("finish").into_inner()
}

/// A running `serve-registry` (the official registry), stopped on drop.
struct Registry {
    child: Child,
    packages: tempfile::TempDir,
    /// `http://127.0.0.1:<port>/v1/`
    base: String,
}

impl Registry {
    fn start() -> Self {
        let packages = tempfile::tempdir().expect("temp dir");
        let mut child = Command::new(env!("CARGO_BIN_EXE_mumcord-plugin"))
            .args(["serve-registry", "--packages"])
            .arg(packages.path())
            .arg("--key")
            .arg(&shared().key)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("serve-registry starts");
        let stdout = child.stdout.take().expect("stdout");
        let base = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .find_map(|l| l.strip_prefix("official registry base ").map(str::to_owned))
            .expect("serve-registry prints its address");
        Self {
            child,
            packages,
            base,
        }
    }

    /// Adds a package and publishes a newer index.
    fn publish(&self, id: &str, version: &str, start: &str, permissions: &[&str]) {
        std::fs::write(
            self.packages
                .path()
                .join(format!("{id}-{version}.mcplugin")),
            package(id, version, start, permissions),
        )
        .expect("write package");
        self.control("reload");
    }

    fn scenario(&self, scenarios: &str) {
        self.control(&format!("scenario?set={scenarios}"));
    }

    fn control(&self, path: &str) {
        let address = self
            .base
            .trim_start_matches("http://")
            .trim_end_matches("/v1/");
        let mut stream = TcpStream::connect(address).expect("connect");
        write!(stream, "POST /_ctl/{path} HTTP/1.1\r\nHost: x\r\n\r\n").expect("request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        assert!(response.starts_with("HTTP/1.1 200"), "{path}: {response}");
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        // The process may already be gone; nothing else to clean up then.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A client profile (store + plugins folder) pointed at the fixture registry.
struct App {
    root: tempfile::TempDir,
    store: Arc<Mutex<Store>>,
    service: Arc<Mutex<PluginService>>,
    client: Client,
}

impl App {
    fn new(registry: &Registry, configure: impl FnOnce(&mut mumcord_store::Settings)) -> Self {
        let root = tempfile::tempdir().expect("temp dir");
        let store = Store::open(root.path()).expect("store opens");
        let mut settings = store.settings().expect("settings");
        settings.plugins_mirror_url = Some(registry.base.clone());
        configure(&mut settings);
        store.save_settings(&settings).expect("save");
        let store = Arc::new(Mutex::new(store));
        let config = ServiceConfig {
            plugins_dir: root.path().join("plugins"),
            cache_dir: None,
            app_version: Version::new(0, 7, 0),
            no_plugins: false,
        };
        let (mut service, _host_events, _events) =
            PluginService::new(config, Arc::clone(&store), None).expect("service builds");
        service.start().expect("service starts");
        Self {
            root,
            store,
            service: Arc::new(Mutex::new(service)),
            client: Client::new(UrlPolicy::new(true)).expect("client"),
        }
    }

    /// Installs the latest version of `id` from the official index, as the store does.
    async fn install(&self, registry: &Registry, id: &str) {
        let index = self
            .client
            .download(&format!("{}index.json", registry.base), 1 << 20)
            .await
            .expect("index");
        let index = mumcord_plugins::registry::Index::parse(&index.bytes, &IndexKind::Official)
            .expect("index parses");
        let plugin = index.plugin(id).expect("listed");
        let latest = plugin.latest();
        let download = self
            .client
            .download(&latest.url, latest.size)
            .await
            .expect("download");
        let request = InstallRequest {
            source: Source::Official,
            entry: plugin.entry(latest),
            optional: vec!["events.voice".into()],
            visibility: PluginVisibility::Friends,
        };
        self.service
            .lock()
            .expect("lock")
            .install(&request, &download)
            .expect("installs");
        self.store
            .lock()
            .expect("lock")
            .plugin_kv_set(id, "global", "k", b"v", 1024, 0)
            .expect("kv write");
    }

    /// One background check as at app start (a fresh schedule: updates are due).
    async fn check(&self) {
        run_check(
            &self.client,
            &self.service,
            &mut Schedule::new(),
            &self.root.path().join("registry"),
        )
        .await
        .expect("check runs");
    }

    fn info(&self, id: &str) -> PluginInfo {
        self.service
            .lock()
            .expect("lock")
            .list()
            .expect("list")
            .into_iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("{id} is listed"))
    }

    fn kv(&self, id: &str) -> Option<Vec<u8>> {
        self.store
            .lock()
            .expect("lock")
            .plugin_kv_get(id, "global", "k")
            .expect("kv read")
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// AC-6.19 through the fixture registry with the default update setting.
#[test]
fn registry_updates_install_ask_respect_pins_and_roll_back() {
    shared();
    runtime().block_on(async {
        let registry = Registry::start();
        registry.publish("poll", "1.0.0", OK_START, PERMISSIONS);
        registry.publish("dice", "1.0.0", OK_START, PERMISSIONS);
        let app = App::new(&registry, |s| {
            assert_eq!(s.plugins_updates, PluginUpdates::Auto, "the default");
        });
        app.install(&registry, "poll").await;
        app.install(&registry, "dice").await;
        app.service
            .lock()
            .expect("lock")
            .set_pinned("dice", true)
            .expect("pins");

        // Poll 1.1 (same permissions) installs by itself; pinned Dice is not offered 1.1.
        registry.publish("poll", "1.1.0", OK_START, PERMISSIONS);
        registry.publish("dice", "1.1.0", OK_START, PERMISSIONS);
        app.check().await;
        let poll = app.info("poll");
        assert_eq!(
            (poll.version.as_str(), poll.status),
            ("1.1.0", Status::Running)
        );
        assert_eq!(app.kv("poll").as_deref(), Some(b"v".as_slice()));
        assert_eq!(app.info("dice").version, "1.0.0");
        let updates = app.service.lock().expect("lock").updates();
        assert!(updates.iter().all(|u| u.id != "dice"), "{updates:?}");

        // Poll 1.2 asks for a new permission: it waits in Updates and asks.
        registry.publish("poll", "1.2.0", OK_START, MORE_PERMISSIONS);
        app.check().await;
        assert_eq!(app.info("poll").version, "1.1.0");
        let updates = app.service.lock().expect("lock").updates();
        let offer = updates.iter().find(|u| u.id == "poll").expect("offered");
        assert_eq!(offer.to, "1.2.0");
        assert!(offer.approval.is_some(), "asks before installing");

        // Poll 1.3 has 1.1's permissions again but fails to start: rolled back with its reason.
        registry.publish("poll", "1.3.0", FAILING_START, PERMISSIONS);
        app.check().await;
        let poll = app.info("poll");
        assert_eq!(
            (poll.version.as_str(), poll.status),
            ("1.1.0", Status::Running)
        );
        assert_eq!(app.kv("poll").as_deref(), Some(b"v".as_slice()));
        let updates = app.service.lock().expect("lock").updates();
        let failed = updates
            .iter()
            .find(|u| u.id == "poll")
            .and_then(|u| u.failed.clone())
            .expect("the reason is shown");
        assert!(failed.contains("nope"), "{failed}");
    });
}

/// AC-6.20: updates Off, the daily check was done today; the forced check (test hook) still
/// fetches `revoked.json` and turns Echolings off with the reason, keeping its data.
#[test]
fn a_revocation_turns_a_plugin_off_with_updates_off() {
    shared();
    runtime().block_on(async {
        let registry = Registry::start();
        registry.publish("echolings", "1.0.0", OK_START, PERMISSIONS);
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_millis(),
        )
        .expect("millis");
        let app = App::new(&registry, |s| {
            s.plugins_updates = PluginUpdates::Off;
            s.plugins_revoked_checked_at = Some(now);
        });
        app.install(&registry, "echolings").await;
        assert_eq!(app.info("echolings").status, Status::Running);

        registry.publish("echolings", "1.1.0", OK_START, PERMISSIONS);
        registry.scenario("revoke:echolings");
        app.check().await;

        let plugin = app.info("echolings");
        assert_eq!(plugin.status, Status::Off);
        assert!(!plugin.enabled);
        assert_eq!(plugin.version, "1.0.0", "updates are off");
        assert_eq!(
            plugin.revocation.expect("notice").text(),
            "Turned off by Mumcord: Revoked for testing"
        );
        assert_eq!(app.kv("echolings").as_deref(), Some(b"v".as_slice()));
    });
}
