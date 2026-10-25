//! The game plugin host with the test plugin (`examples/test_plugin.rs`) simulating a game:
//! the "game" is this test program, found by name in the running programs (FR-1.42b, AC-1.32).

use std::env::consts::{DLL_EXTENSION, DLL_PREFIX};
use std::ffi::{CString, c_char};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use libloading::Library;
use mumcord_positional::plugins::{DEMAND_TIMEOUT, SEARCH_INTERVAL};
use mumcord_positional::{Fetch, PluginHost, PluginInfo, PluginProblem, PositionSource};

// Mirrors of the test plugin's constants.
const MODE_NORMAL: u32 = 0;
const MODE_INIT_FAILS: u32 = 1;
const MODE_UNSUPPORTED: u32 = 2;
const MODE_API_2: u32 = 3;
const COUNT_INIT: u32 = 0;
const COUNT_SHUTDOWN: u32 = 1;
const COUNT_SEARCH: u32 = 2;
const COUNT_FETCH: u32 = 3;
const COUNT_SHUTDOWN_POSITIONAL: u32 = 4;

const WAIT: Duration = Duration::from_secs(8);

/// The built test plugin.
fn fixture() -> PathBuf {
    let exe = std::env::current_exe().expect("test exe");
    let file = exe
        .parent()
        .and_then(Path::parent)
        .expect("target dir")
        .join("examples")
        .join(format!("{DLL_PREFIX}test_plugin.{DLL_EXTENSION}"));
    assert!(
        file.exists(),
        "{} missing: run `cargo test -p mumcord-positional`, which builds the test plugin",
        file.display()
    );
    file
}

fn plugin_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("plugin-host")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("plugin dir");
    dir
}

/// The name this test program has in the running programs.
fn this_program() -> String {
    let exe = std::env::current_exe().expect("test exe");
    exe.file_name()
        .and_then(|n| n.to_str())
        .expect("exe name")
        .to_owned()
}

/// A copy of the test plugin installed as plugin `id`; the copy is a plugin of its own.
struct TestPlugin {
    lib: Library,
}

impl TestPlugin {
    fn install(dir: &Path, id: &str, mode: u32, name: &str) -> Self {
        let path = dir.join(format!("{DLL_PREFIX}{id}.{DLL_EXTENSION}"));
        std::fs::copy(fixture(), &path).expect("install plugin");
        // SAFETY: the test plugin has no initialisers with preconditions.
        let lib = unsafe { Library::new(&path) }.expect("load test plugin");
        let plugin = Self { lib };
        let name = CString::new(name).expect("name");
        let game = CString::new(this_program()).expect("game");
        // SAFETY: the test plugin's own signature; both strings are NUL-terminated.
        unsafe {
            let configure = plugin
                .lib
                .get::<unsafe extern "C" fn(u32, *const c_char, *const c_char)>(
                    b"mumcord_test_configure\0",
                )
                .expect("configure");
            configure(mode, name.as_ptr(), game.as_ptr());
        }
        plugin
    }

    fn count(&self, which: u32) -> u32 {
        // SAFETY: the test plugin's own signature.
        unsafe {
            let f = self
                .lib
                .get::<extern "C" fn(u32) -> u32>(b"mumcord_test_count\0")
                .expect("count");
            f(which)
        }
    }

    fn set_running(&self, running: bool) {
        // SAFETY: the test plugin's own signature.
        unsafe {
            let f = self
                .lib
                .get::<extern "C" fn(bool)>(b"mumcord_test_set_running\0")
                .expect("set_running");
            f(running);
        }
    }

    fn api_registered(&self) -> bool {
        // SAFETY: the test plugin's own signature.
        unsafe {
            let f = self
                .lib
                .get::<extern "C" fn() -> bool>(b"mumcord_test_api_registered\0")
                .expect("api_registered");
            f()
        }
    }
}

fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if let Some(v) = f() {
            return Some(v);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

fn poll(source: &mut Box<dyn PositionSource>) -> Option<Fetch> {
    wait_for(|| source.fetch(Instant::now()))
}

fn info(host: &PluginHost, id: &str) -> PluginInfo {
    host.list()
        .expect("list")
        .into_iter()
        .find(|p| p.id == id)
        .expect("plugin listed")
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn enabled_plugin_delivers_positions_of_a_running_game() {
    let dir = plugin_dir("delivers");
    let plugin = TestPlugin::install(&dir, "fakegame", MODE_NORMAL, "Fake Game");
    std::fs::write(dir.join("notes.txt"), "not a plugin").expect("notes");
    let host = PluginHost::start(vec![dir]).expect("host");

    let listed = host.list().expect("list");
    assert_eq!(
        listed,
        [PluginInfo {
            id: "fakegame".into(),
            name: "Fake Game".into(),
            version: Some("1.2.3".into()),
            enabled: false,
            active: false,
            problem: None,
        }]
    );
    assert!(plugin.api_registered(), "API table handed over and answers");
    assert_eq!(plugin.count(COUNT_INIT), 0, "listing does not initialise");

    host.set_enabled(&ids(&["fakegame"])).expect("enable");
    let mut source = host.sources(&ids(&["fakegame"])).remove(0);
    let fetch = poll(&mut source).expect("positions arrive");
    assert_eq!(fetch.source, "Fake Game");
    assert_eq!(fetch.game, None);
    assert_eq!(fetch.data.avatar.position, [1.0, 2.0, 3.0]);
    assert_eq!(fetch.data.avatar.front, [0.0, 0.0, 1.0]);
    assert_eq!(fetch.data.camera.position, [1.0, 3.5, 2.0]);
    assert_eq!(fetch.data.camera.top, [0.0, 1.0, 0.0]);
    assert_eq!(fetch.data.context, b"Fake Game\0test-context");
    assert_eq!(fetch.data.identity, "tester");

    let shown = info(&host, "fakegame");
    assert!(shown.enabled && shown.active, "{shown:?}");
}

#[test]
fn the_game_is_not_read_while_nobody_asks_for_positions() {
    let dir = plugin_dir("demand");
    let plugin = TestPlugin::install(&dir, "fakegame", MODE_NORMAL, "Fake Game");
    let host = PluginHost::start(vec![dir]).expect("host");
    host.set_enabled(&ids(&["fakegame"])).expect("enable");
    assert!(wait_for(|| (plugin.count(COUNT_INIT) == 1).then_some(())).is_some());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(plugin.count(COUNT_SEARCH), 0, "no search without demand");

    let mut source = host.sources(&ids(&["fakegame"])).remove(0);
    poll(&mut source).expect("positions arrive");
    // Positional audio switched off: the engine stops asking.
    std::thread::sleep(DEMAND_TIMEOUT + Duration::from_millis(300));
    assert_eq!(plugin.count(COUNT_SHUTDOWN_POSITIONAL), 1);
    let fetches = plugin.count(COUNT_FETCH);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(plugin.count(COUNT_FETCH), fetches, "no more fetches");
}

#[test]
fn a_game_that_ends_and_restarts_is_picked_up_again() {
    let dir = plugin_dir("restart");
    let plugin = TestPlugin::install(&dir, "fakegame", MODE_NORMAL, "Fake Game");
    let host = PluginHost::start(vec![dir]).expect("host");
    host.set_enabled(&ids(&["fakegame"])).expect("enable");
    let mut source = host.sources(&ids(&["fakegame"])).remove(0);
    poll(&mut source).expect("positions arrive");

    plugin.set_running(false);
    assert!(
        wait_for(|| source.fetch(Instant::now()).is_none().then_some(())).is_some(),
        "positions stop"
    );
    assert_eq!(plugin.count(COUNT_SHUTDOWN_POSITIONAL), 1);
    assert!(!info(&host, "fakegame").active);

    plugin.set_running(true);
    let started = Instant::now();
    poll(&mut source).expect("positions again");
    assert!(started.elapsed() < SEARCH_INTERVAL + Duration::from_secs(2));
}

#[test]
fn disabling_shuts_the_plugin_down() {
    let dir = plugin_dir("disable");
    let plugin = TestPlugin::install(&dir, "fakegame", MODE_NORMAL, "Fake Game");
    let host = PluginHost::start(vec![dir]).expect("host");
    host.set_enabled(&ids(&["fakegame"])).expect("enable");
    let mut source = host.sources(&ids(&["fakegame"])).remove(0);
    poll(&mut source).expect("positions arrive");

    host.set_enabled(&[]).expect("disable");
    let shown = info(&host, "fakegame");
    assert!(!shown.enabled && !shown.active, "{shown:?}");
    assert_eq!(plugin.count(COUNT_SHUTDOWN_POSITIONAL), 1);
    assert_eq!(plugin.count(COUNT_SHUTDOWN), 1);
    assert_eq!(source.fetch(Instant::now()), None);
}

#[test]
fn failing_plugins_do_not_affect_the_others() {
    let dir = plugin_dir("failures");
    let good = TestPlugin::install(&dir, "good", MODE_NORMAL, "Good Game");
    let _newer = TestPlugin::install(&dir, "newer", MODE_API_2, "Newer API");
    let broken = TestPlugin::install(&dir, "broken", MODE_INIT_FAILS, "Broken");
    let other = TestPlugin::install(&dir, "other", MODE_UNSUPPORTED, "Other OS");
    std::fs::write(
        dir.join(format!("{DLL_PREFIX}junk.{DLL_EXTENSION}")),
        "not a library",
    )
    .expect("junk");
    let host = PluginHost::start(vec![dir]).expect("host");

    let listed = host.list().expect("list");
    let listed_ids: Vec<&str> = listed.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(listed_ids, ["broken", "good", "newer", "other"]);
    assert_eq!(
        info(&host, "newer").problem,
        Some(PluginProblem::Incompatible)
    );

    let all = ids(&["broken", "good", "missing", "newer", "other"]);
    host.set_enabled(&all).expect("enable");
    let mut sources = host.sources(&all);
    // Like the engine with positional audio on, but asking every source each time.
    let mut ask_all = || {
        let now = Instant::now();
        let fetched: Vec<Fetch> = sources.iter_mut().filter_map(|s| s.fetch(now)).collect();
        fetched
    };
    let fetched = wait_for(|| {
        let fetched = ask_all();
        (!fetched.is_empty() && other.count(COUNT_SEARCH) > 0).then_some(fetched)
    })
    .expect("the good plugin delivers and the others were tried");
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].source, "Good Game");

    assert_eq!(
        info(&host, "broken").problem,
        Some(PluginProblem::InitFailed)
    );
    assert_eq!(broken.count(COUNT_SEARCH), 0);
    let unsupported = info(&host, "other");
    assert_eq!(unsupported.problem, Some(PluginProblem::Unsupported));
    assert!(!unsupported.active);
    let searches = other.count(COUNT_SEARCH);
    let until = Instant::now() + SEARCH_INTERVAL + Duration::from_millis(200);
    while Instant::now() < until {
        assert_eq!(ask_all().len(), 1);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(other.count(COUNT_SEARCH), searches, "never asked again");
    assert!(info(&host, "good").active);
    assert!(good.count(COUNT_FETCH) > 0);
}
