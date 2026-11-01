//! The sandbox with real (tiny) plugins: built from `fixtures/plugin.wat` with the start and stop
//! bodies each test needs, no network and no toolchain beyond this crate's dev-dependencies.

use std::path::Path;
use std::time::{Duration, Instant};

use mumcord_plugins::{
    CallError, CompileError, HostError, Incompatibility, Level, LoadError, PluginHost, PluginState,
};
use semver::Version;

/// Logs the API version and, after an update, the previous version; then returns `ok`.
const WELL_BEHAVED: &str = "
    (call $log (i32.const 1) (local.get $api_ptr) (local.get $api_len))
    (if (local.get $has_prev)
      (then (call $log (i32.const 1) (local.get $prev_ptr) (local.get $prev_len))))
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64)";
const ENDLESS_LOOP: &str = "(loop $forever (br $forever)) (unreachable)";
/// Grows memory 1 MiB at a time until the limit stops it.
const MEMORY_HOG: &str =
    "(loop $more (drop (memory.grow (i32.const 16))) (br $more)) (unreachable)";

fn component(start: &str, stop: &str) -> Vec<u8> {
    let core = include_str!("fixtures/plugin.wat")
        .replace("{{START}}", start)
        .replace("{{STOP}}", stop);
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

fn manifest(id: &str, extra: &str) -> String {
    format!(
        r#"{{"manifest":1,"id":"{id}","name":"Test","version":"1.0.0","api":"^1.0","authors":["Tests"],
           "description":"A test plugin","categories":["tools"],"license":"MIT"{extra}}}"#
    )
}

fn plugin_dir(root: &Path, id: &str, wasm: &[u8], extra_manifest: &str) -> std::path::PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(dir.join("manifest.json"), manifest(id, extra_manifest))
        .expect("write manifest");
    std::fs::write(dir.join("plugin.wasm"), wasm).expect("write wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("write i18n");
    dir
}

fn host() -> PluginHost {
    PluginHost::new(Version::new(0, 7, 0)).expect("runtime starts")
}

fn failed_with(host: &PluginHost, id: &str) -> CallError {
    match host.state(id) {
        Some(PluginState::Failed { error, .. }) => error.clone(),
        other => panic!("expected a failed plugin, got {other:?}"),
    }
}

#[test]
fn runs_a_plugin_through_its_lifecycle() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    let dir = plugin_dir(root.path(), "good", &component(WELL_BEHAVED, ""), "");

    host.load(&dir, Some(Version::new(0, 9, 0))).expect("loads");
    assert_eq!(host.state("good"), Some(&PluginState::Disabled));
    host.enable("good").expect("starts");
    assert_eq!(host.state("good"), Some(&PluginState::Enabled));
    let lines = host.log_lines("good");
    assert_eq!(
        lines,
        vec![
            (Level::Info, "1.0.0".to_owned()),
            (Level::Info, "0.9.0".to_owned())
        ]
    );

    host.disable("good").expect("stops");
    assert_eq!(host.state("good"), Some(&PluginState::Disabled));
    host.enable("good").expect("starts again");
    host.unload("good").expect("unloads");
    assert_eq!(host.state("good"), None);
    assert_eq!(
        host.enable("good"),
        Err(HostError::NotLoaded("good".into()))
    );
}

#[test]
fn endless_loop_is_stopped_and_the_host_keeps_working() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    host.load(
        &plugin_dir(root.path(), "spinner", &component(ENDLESS_LOOP, ""), ""),
        None,
    )
    .expect("loads");
    host.load(
        &plugin_dir(root.path(), "good", &component(WELL_BEHAVED, ""), ""),
        None,
    )
    .expect("loads");

    let started = Instant::now();
    assert!(host.enable("spinner").is_err());
    // `start` has 500 ms; generous slack for a loaded CI machine.
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(failed_with(&host, "spinner"), CallError::Timeout);

    host.enable("good").expect("other plugins still run");
}

#[test]
fn endless_loop_in_a_normal_call_is_stopped_after_its_short_budget() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    host.load(
        &plugin_dir(
            root.path(),
            "stuck",
            &component(WELL_BEHAVED, ENDLESS_LOOP),
            "",
        ),
        None,
    )
    .expect("loads");
    host.enable("stuck").expect("starts");

    let started = Instant::now();
    let result = host.disable("stuck");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(
        result,
        Err(HostError::Call {
            id: "stuck".into(),
            error: CallError::Timeout
        })
    );
    assert_eq!(host.state("stuck"), Some(&PluginState::Disabled));
}

#[test]
fn memory_is_limited() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    host.load(
        &plugin_dir(root.path(), "hog", &component(MEMORY_HOG, ""), ""),
        None,
    )
    .expect("loads");
    assert!(host.enable("hog").is_err());
    assert_eq!(failed_with(&host, "hog"), CallError::OutOfMemory);

    // A lower limit from the manifest applies too.
    let small = r#","limits":{"memory_mib":2}"#;
    let grow_4_mib = "(if (i32.eq (memory.grow (i32.const 64)) (i32.const -1)) (then (unreachable))) (i32.const 64)";
    host.load(
        &plugin_dir(root.path(), "small", &component(grow_4_mib, ""), small),
        None,
    )
    .expect("loads");
    assert!(host.enable("small").is_err());
    assert_eq!(failed_with(&host, "small"), CallError::OutOfMemory);
}

#[test]
fn trap_stops_the_plugin_with_back_off_and_stays_off_after_three() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    host.load(
        &plugin_dir(root.path(), "crashy", &component("(unreachable)", ""), ""),
        None,
    )
    .expect("loads");

    for attempt in 1..=3 {
        assert!(host.enable("crashy").is_err());
        let Some(PluginState::Failed {
            error: CallError::Trap(_),
            retry_at,
        }) = host.state("crashy")
        else {
            panic!("expected a trap, got {:?}", host.state("crashy"));
        };
        assert_eq!(retry_at.is_some(), attempt < 3, "attempt {attempt}");
    }
    // "Turn on again" starts counting from zero.
    assert!(host.turn_on_again("crashy").is_err());
    assert!(matches!(
        host.state("crashy"),
        Some(PluginState::Failed {
            retry_at: Some(_),
            ..
        })
    ));
}

#[test]
fn start_can_refuse() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    let refuse =
        "(i32.store8 (i32.const 64) (i32.const 1)) (i32.store (i32.const 68) (i32.const 32))
                  (i32.store (i32.const 72) (i32.const 4)) (i32.const 64)";
    host.load(
        &plugin_dir(root.path(), "picky", &component(refuse, ""), ""),
        None,
    )
    .expect("loads");
    assert!(host.enable("picky").is_err());
    assert_eq!(
        failed_with(&host, "picky"),
        CallError::Refused("nope".into())
    );
}

#[test]
fn wasi_and_unknown_imports_fail_to_load_with_a_clear_message() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    let wasi = wat::parse_str(r#"(component (import "wasi:filesystem/types@0.2.0" (instance)))"#)
        .expect("valid");
    let error = host
        .load(&plugin_dir(root.path(), "wasi", &wasi, ""), None)
        .expect_err("refused");
    assert!(
        matches!(&error, LoadError::Compile(CompileError::UnknownImport(name)) if name == "wasi:filesystem/types@0.2.0")
    );
    assert!(error.to_string().contains("no WASI"), "{error}");

    let plain_func =
        wat::parse_str(r#"(component (import "steal-cookies" (func)))"#).expect("valid");
    let error = host
        .load(&plugin_dir(root.path(), "sneaky", &plain_func, ""), None)
        .expect_err("refused");
    assert!(matches!(
        error,
        LoadError::Compile(CompileError::UnknownImport(_))
    ));

    let core_module = wat::parse_str("(module)").expect("valid");
    let error = host
        .load(&plugin_dir(root.path(), "core", &core_module, ""), None)
        .expect_err("refused");
    assert!(matches!(
        error,
        LoadError::Compile(CompileError::Invalid(_))
    ));
}

#[test]
fn api_version_decides_whether_a_plugin_loads() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut host = host();
    let wasm = component(WELL_BEHAVED, "");
    let newer = plugin_dir(root.path(), "newer", &wasm, "").join("manifest.json");
    std::fs::write(&newer, manifest("newer", "").replace("^1.0", "^1.5")).expect("write");
    let error = host
        .load(newer.parent().expect("dir"), None)
        .expect_err("refused");
    assert!(matches!(
        error,
        LoadError::Incompatible(Incompatibility::NeedsNewerMumcord)
    ));
    assert_eq!(error.to_string(), "Needs a newer Mumcord");

    let app_range = plugin_dir(root.path(), "fixed", &wasm, r#","mumcord":">=0.8.0""#);
    assert!(matches!(
        host.load(&app_range, None),
        Err(LoadError::Incompatible(Incompatibility::NeedsNewerMumcord))
    ));

    host.load(&plugin_dir(root.path(), "current", &wasm, ""), None)
        .expect("^1.0 loads");
    assert!(matches!(
        host.load(&root.path().join("current"), None),
        Err(LoadError::AlreadyLoaded(_))
    ));
}
