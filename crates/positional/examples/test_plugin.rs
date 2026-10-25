//! A Mumble game plugin for tests (AC-1.32): implements the plugin's side of the C interface
//! and "finds its game" when a program with the configured name runs, then returns fixed
//! positions. Tests steer it through the extra `mumcord_test_*` functions; every copy of the
//! library at another path is a separate plugin with its own state.
//!
//! The E2E tests set `MUMCORD_TEST_PLUGIN_GAME` instead (see [`texts`]).
//!
//! Built by `cargo test` as `examples/<dll prefix>test_plugin.<dll extension>`.

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use mumcord_positional::plugins::abi::{
    self, ErrorCode, PDEC_ERROR_PERM, PDEC_ERROR_TEMP, PDEC_OK, PluginId, StringWrapper, Version,
};

/// Behaves like a working plugin.
pub const MODE_NORMAL: u32 = 0;
/// `mumble_init` fails.
pub const MODE_INIT_FAILS: u32 = 1;
/// `mumble_initPositionalData` says "never".
pub const MODE_UNSUPPORTED: u32 = 2;
/// Reports plugin API 2.0.0.
pub const MODE_API_2: u32 = 3;

/// Counters readable through `mumcord_test_count`.
pub const COUNT_INIT: u32 = 0;
pub const COUNT_SHUTDOWN: u32 = 1;
pub const COUNT_SEARCH: u32 = 2;
pub const COUNT_FETCH: u32 = 3;
pub const COUNT_SHUTDOWN_POSITIONAL: u32 = 4;

static MODE: AtomicU32 = AtomicU32::new(MODE_NORMAL);
static RUNNING: AtomicBool = AtomicBool::new(true);
static API_REGISTERED: AtomicBool = AtomicBool::new(false);
static COUNTS: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];

struct Texts {
    name: CString,
    game: CString,
    context: CString,
    identity: CString,
}

static TEXTS: Mutex<Option<Texts>> = Mutex::new(None);

/// Name the plugin reports when the E2E tests configure it through the environment.
const ENV_NAME: &CStr = c"Mumcord Test Game";

/// The configured texts. Unconfigured, `MUMCORD_TEST_PLUGIN_GAME` names the program to look
/// for: the E2E tests load the plugin into the real app, where they cannot call
/// `mumcord_test_configure`.
fn texts() -> MutexGuard<'static, Option<Texts>> {
    let mut texts = TEXTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if texts.is_none()
        && let Some(game) = std::env::var("MUMCORD_TEST_PLUGIN_GAME")
            .ok()
            .and_then(|g| CString::new(g).ok())
    {
        *texts = Some(Texts {
            name: ENV_NAME.to_owned(),
            game,
            context: c"test-context".to_owned(),
            identity: c"tester".to_owned(),
        });
    }
    texts
}

fn count(which: u32) {
    COUNTS[which as usize].fetch_add(1, Ordering::Relaxed);
}

/// Sets the behaviour, the plugin name and the program it looks for.
///
/// # Safety
/// `name` and `game` are NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mumcord_test_configure(
    mode: u32,
    name: *const c_char,
    game: *const c_char,
) {
    MODE.store(mode, Ordering::Relaxed);
    // SAFETY: see the function contract.
    let (name, game) = unsafe {
        (
            CStr::from_ptr(name).to_owned(),
            CStr::from_ptr(game).to_owned(),
        )
    };
    *texts() = Some(Texts {
        name,
        game,
        context: c"test-context".to_owned(),
        identity: c"tester".to_owned(),
    });
}

/// Simulates the game starting (`true`) or ending (`false`).
#[unsafe(no_mangle)]
pub extern "C" fn mumcord_test_set_running(running: bool) {
    RUNNING.store(running, Ordering::Relaxed);
}

/// How often a plugin function was called (see the `COUNT_*` constants).
#[unsafe(no_mangle)]
pub extern "C" fn mumcord_test_count(which: u32) -> u32 {
    COUNTS[which as usize].load(Ordering::Relaxed)
}

/// Whether the host handed over its API functions.
#[unsafe(no_mangle)]
pub extern "C" fn mumcord_test_api_registered() -> bool {
    API_REGISTERED.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_init(_id: PluginId) -> ErrorCode {
    count(COUNT_INIT);
    if MODE.load(Ordering::Relaxed) == MODE_INIT_FAILS {
        abi::EC_GENERIC_ERROR
    } else {
        abi::STATUS_OK
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_shutdown() {
    count(COUNT_SHUTDOWN);
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_getName() -> StringWrapper {
    let texts = texts();
    let (data, size) = texts.as_ref().map_or((std::ptr::null(), 0), |t| {
        (t.name.as_ptr(), t.name.as_bytes().len())
    });
    StringWrapper {
        data,
        size,
        needs_releasing: false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_getAPIVersion() -> Version {
    if MODE.load(Ordering::Relaxed) == MODE_API_2 {
        Version {
            major: 2,
            minor: 0,
            patch: 0,
        }
    } else {
        abi::MIN_API_VERSION
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_getVersion() -> Version {
    Version {
        major: 1,
        minor: 2,
        patch: 3,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_getFeatures() -> u32 {
    abi::FEATURE_POSITIONAL
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_registerAPIFunctions(api: *mut c_void) {
    if api.is_null() {
        return;
    }
    // Like a plugin that logs: the first entry (`freeMemory`) must answer, with an error.
    // SAFETY: the host passes a table of `extern "C" fn` pointers.
    let first = unsafe { *api.cast::<extern "C" fn(PluginId, *const c_void) -> ErrorCode>() };
    API_REGISTERED.store(
        first(0, std::ptr::null()) != abi::STATUS_OK,
        Ordering::Relaxed,
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_releaseResource(_pointer: *const c_void) {}

/// # Safety
/// `names` and `pids` have `count` entries; the names are NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mumble_initPositionalData(
    names: *const *const c_char,
    pids: *const u64,
    count_: usize,
) -> u8 {
    count(COUNT_SEARCH);
    if MODE.load(Ordering::Relaxed) == MODE_UNSUPPORTED {
        return PDEC_ERROR_PERM;
    }
    if !RUNNING.load(Ordering::Relaxed) || count_ == 0 || names.is_null() || pids.is_null() {
        return PDEC_ERROR_TEMP;
    }
    let texts = texts();
    let Some(game) = texts.as_ref().map(|t| t.game.as_c_str()) else {
        return PDEC_ERROR_TEMP;
    };
    // SAFETY: see the function contract.
    let names = unsafe { std::slice::from_raw_parts(names, count_) };
    // SAFETY: as above.
    let found = names.iter().any(|&n| unsafe { CStr::from_ptr(n) } == game);
    if found { PDEC_OK } else { PDEC_ERROR_TEMP }
}

fn write(out: *mut f32, v: [f32; 3]) {
    // SAFETY: the host passes three writable floats per vector.
    unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, 3) };
}

/// # Safety
/// Six writable three-float vectors and two writable string pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mumble_fetchPositionalData(
    avatar_pos: *mut f32,
    avatar_dir: *mut f32,
    avatar_axis: *mut f32,
    camera_pos: *mut f32,
    camera_dir: *mut f32,
    camera_axis: *mut f32,
    context: *mut *const c_char,
    identity: *mut *const c_char,
) -> bool {
    count(COUNT_FETCH);
    if !RUNNING.load(Ordering::Relaxed) {
        return false;
    }
    write(avatar_pos, [1.0, 2.0, 3.0]);
    write(avatar_dir, [0.0, 0.0, 1.0]);
    write(avatar_axis, [0.0, 1.0, 0.0]);
    write(camera_pos, [1.0, 3.5, 2.0]);
    write(camera_dir, [1.0, 0.0, 0.0]);
    write(camera_axis, [0.0, 1.0, 0.0]);
    let texts = texts();
    if let Some(t) = texts.as_ref() {
        // SAFETY: writable out-pointers; the strings live in a static until reconfigured.
        unsafe {
            *context = t.context.as_ptr();
            *identity = t.identity.as_ptr();
        }
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn mumble_shutdownPositionalData() {
    count(COUNT_SHUTDOWN_POSITIONAL);
}
