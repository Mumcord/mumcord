//! One loaded plugin library and safe wrappers around its C functions. Only the plugin host
//! thread calls into it.

use std::ffi::{CStr, CString, c_char};
use std::path::Path;

use libloading::Library;

use super::abi::{self, Version};
use crate::Pose;

/// Longest name or version string accepted from a plugin.
const MAX_STRING: usize = 1024;

/// Why a file is not usable as a positional game plugin.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LoadError {
    #[error("the library cannot be loaded: {0}")]
    Open(#[from] libloading::Error),
    #[error("not a Mumble plugin (missing {0})")]
    NotAPlugin(&'static str),
    #[error("plugin API {0} is not supported")]
    Incompatible(Version),
    #[error("the plugin delivers no positions")]
    NotPositional,
}

/// Result of `mumble_initPositionalData`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Search {
    Found,
    NotNow,
    Never,
}

/// What `mumble_fetchPositionalData` returned.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct RawPositions {
    pub avatar: Pose,
    pub camera: Pose,
    pub context: String,
    pub identity: String,
}

/// The functions of a plugin, copied out of the library. Valid while `PluginLibrary::_lib`
/// is loaded.
struct Fns {
    init: abi::InitFn,
    shutdown: abi::ShutdownFn,
    init_positional: abi::InitPositionalDataFn,
    fetch_positional: abi::FetchPositionalDataFn,
    shutdown_positional: abi::ShutdownPositionalDataFn,
}

/// A loaded positional game plugin.
pub(crate) struct PluginLibrary {
    pub name: String,
    pub version: Option<String>,
    fns: Fns,
    // Never unloaded while the functions may be called: dropped last, after `fns`.
    _lib: Library,
}

/// Looks up a function. `T` must be the function's real type (see [`abi`]).
fn symbol<T: Copy>(lib: &Library, name: &'static str) -> Option<T> {
    let mut cname = name.as_bytes().to_vec();
    cname.push(0);
    // SAFETY: the caller names the type the Mumble plugin interface declares for `name`.
    unsafe { lib.get::<T>(&cname) }.ok().map(|s| *s)
}

fn required<T: Copy>(lib: &Library, name: &'static str) -> Result<T, LoadError> {
    symbol(lib, name).ok_or(LoadError::NotAPlugin(name))
}

/// Reads a string a plugin returned and releases it if the plugin asks for that.
fn take_string(wrapper: abi::StringWrapper, release: Option<abi::ReleaseResourceFn>) -> String {
    if wrapper.data.is_null() {
        return String::new();
    }
    // SAFETY: the plugin interface promises `size` readable bytes at `data`; capped against
    // nonsense sizes.
    let bytes = unsafe {
        std::slice::from_raw_parts(wrapper.data.cast::<u8>(), wrapper.size.min(MAX_STRING))
    };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
    if wrapper.needs_releasing
        && let Some(release) = release
    {
        // SAFETY: the plugin asked for exactly this pointer to be handed back.
        unsafe { release(wrapper.data.cast()) };
    }
    text
}

/// Copies a string the plugin keeps ownership of.
///
/// # Safety
/// `ptr` is null or points to a NUL-terminated string.
unsafe fn borrowed_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: see the function contract.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

impl PluginLibrary {
    /// Loads `path`, checks the plugin API version and hands over the (empty) Mumble API.
    /// The plugin is not initialised yet.
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        // SAFETY: loading runs the library's initialisers. Plugins come from Mumcord's and
        // Mumble's plugin folders, which only the user (or their Mumble installation) fills:
        // the same trust the official client gives them.
        let lib = unsafe { Library::new(path) }?;
        let api_version: abi::GetVersionFn = required(&lib, "mumble_getAPIVersion")?;
        // SAFETY: declared by the plugin interface; no preconditions.
        let api = unsafe { api_version() };
        // Within one major version the interface only grows; a newer minor version can only
        // use API functions, which the spare table entries answer with an error.
        if api.major != abi::MIN_API_VERSION.major || api < abi::MIN_API_VERSION {
            return Err(LoadError::Incompatible(api));
        }
        let init = required(&lib, "mumble_init")?;
        let shutdown = required(&lib, "mumble_shutdown")?;
        let get_name: abi::GetStringFn = required(&lib, "mumble_getName")?;
        let register: abi::RegisterApiFunctionsFn = required(&lib, "mumble_registerAPIFunctions")?;
        let release = symbol::<abi::ReleaseResourceFn>(&lib, "mumble_releaseResource");

        let features = symbol::<abi::GetFeaturesFn>(&lib, "mumble_getFeatures")
            // SAFETY: declared by the plugin interface; no preconditions.
            .map_or(abi::FEATURE_POSITIONAL, |f| unsafe { f() });
        let (Some(init_positional), Some(fetch_positional), Some(shutdown_positional)) = (
            symbol(&lib, "mumble_initPositionalData"),
            symbol(&lib, "mumble_fetchPositionalData"),
            symbol(&lib, "mumble_shutdownPositionalData"),
        ) else {
            return Err(LoadError::NotPositional);
        };
        if features & abi::FEATURE_POSITIONAL == 0 {
            return Err(LoadError::NotPositional);
        }

        if let Some(set_info) = symbol::<abi::SetMumbleInfoFn>(&lib, "mumble_setMumbleInfo") {
            // SAFETY: declared by the plugin interface; plain values.
            unsafe { set_info(abi::MUMBLE_VERSION, abi::API_VERSION, abi::MIN_API_VERSION) };
        }
        // SAFETY: the table is static and has more entries than any known API struct.
        unsafe { register(abi::API_TABLE.as_ptr()) };
        // SAFETY: declared by the plugin interface; no preconditions.
        let name = take_string(unsafe { get_name() }, release);
        let version = symbol::<abi::GetVersionFn>(&lib, "mumble_getVersion")
            // SAFETY: declared by the plugin interface; no preconditions.
            .map(|f| unsafe { f() }.to_string());
        Ok(Self {
            name,
            version,
            fns: Fns {
                init,
                shutdown,
                init_positional,
                fetch_positional,
                shutdown_positional,
            },
            _lib: lib,
        })
    }

    /// `mumble_init`; `false` if the plugin reports an error.
    pub fn init(&self, id: abi::PluginId) -> bool {
        // SAFETY: called once before any other plugin call that needs initialisation.
        unsafe { (self.fns.init)(id) == abi::STATUS_OK }
    }

    /// `mumble_shutdown`.
    pub fn shutdown(&self) {
        // SAFETY: only called after a successful `init`.
        unsafe { (self.fns.shutdown)() }
    }

    /// `mumble_initPositionalData` with the running programs (name, process id).
    pub fn init_positional(&self, programs: &[(CString, u64)]) -> Search {
        let names: Vec<*const c_char> = programs.iter().map(|(n, _)| n.as_ptr()).collect();
        let pids: Vec<u64> = programs.iter().map(|(_, p)| *p).collect();
        // SAFETY: both arrays have `programs.len()` entries and outlive the call.
        let code =
            unsafe { (self.fns.init_positional)(names.as_ptr(), pids.as_ptr(), names.len()) };
        match code {
            abi::PDEC_OK => Search::Found,
            abi::PDEC_ERROR_TEMP => Search::NotNow,
            _ => Search::Never,
        }
    }

    /// `mumble_fetchPositionalData`; `None` once the game is gone.
    pub fn fetch_positional(&self) -> Option<RawPositions> {
        let mut v = [[0f32; 3]; 6];
        let mut context: *const c_char = std::ptr::null();
        let mut identity: *const c_char = std::ptr::null();
        let [ap, af, at, cp, cf, ct] = &mut v;
        // SAFETY: six writable three-float vectors and two string out-pointers, as declared.
        let ok = unsafe {
            (self.fns.fetch_positional)(
                ap.as_mut_ptr(),
                af.as_mut_ptr(),
                at.as_mut_ptr(),
                cp.as_mut_ptr(),
                cf.as_mut_ptr(),
                ct.as_mut_ptr(),
                &raw mut context,
                &raw mut identity,
            )
        };
        if !ok {
            return None;
        }
        let [ap, af, at, cp, cf, ct] = v;
        Some(RawPositions {
            avatar: Pose {
                position: ap,
                front: af,
                top: at,
            },
            camera: Pose {
                position: cp,
                front: cf,
                top: ct,
            },
            // SAFETY: null or NUL-terminated strings the plugin keeps until its next call.
            context: unsafe { borrowed_string(context) },
            // SAFETY: as above.
            identity: unsafe { borrowed_string(identity) },
        })
    }

    /// `mumble_shutdownPositionalData`.
    pub fn shutdown_positional(&self) {
        // SAFETY: only called after `init_positional` found the game.
        unsafe { (self.fns.shutdown_positional)() }
    }
}
