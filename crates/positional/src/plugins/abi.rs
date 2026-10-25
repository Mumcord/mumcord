//! The C interface of Mumble plugins as defined by Mumble 1.4 and later (`MumblePlugin.h`,
//! `PluginComponents_v_1_0_x.h`, BSD-licensed). Only what positional audio needs is declared;
//! game plugins export these functions with C linkage under the `mumble_` names.

use std::ffi::{c_char, c_void};

/// `mumble_plugin_id_t`: the number the host assigns to a plugin in `mumble_init`.
pub type PluginId = u32;
/// `mumble_error_t`.
pub type ErrorCode = i32;

/// `MUMBLE_STATUS_OK`.
pub const STATUS_OK: ErrorCode = 0;
/// `MUMBLE_EC_GENERIC_ERROR`.
pub const EC_GENERIC_ERROR: ErrorCode = -1;

/// `MUMBLE_FEATURE_POSITIONAL` in `mumble_getFeatures`.
pub const FEATURE_POSITIONAL: u32 = 1;

/// `MUMBLE_PDEC_OK`: the game was found, positions can be fetched.
pub const PDEC_OK: u8 = 0;
/// `MUMBLE_PDEC_ERROR_TEMP`: not now (usually: the game is not running), ask again later.
pub const PDEC_ERROR_TEMP: u8 = 1;
/// `MUMBLE_PDEC_ERROR_PERM`: never on this system; do not ask again.
pub const PDEC_ERROR_PERM: u8 = 2;

/// `mumble_version_t`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The Mumble version Mumcord presents itself as: it implements the plugin interface of
/// Mumble 1.5.
pub const MUMBLE_VERSION: Version = Version {
    major: 1,
    minor: 5,
    patch: 0,
};
/// The plugin API version of Mumble 1.5.
pub const API_VERSION: Version = Version {
    major: 1,
    minor: 2,
    patch: 0,
};
/// The oldest plugin API (Mumble 1.4).
pub const MIN_API_VERSION: Version = Version {
    major: 1,
    minor: 0,
    patch: 0,
};

/// `struct MumbleStringWrapper`: a string owned by the plugin; released with
/// `mumble_releaseResource(data)` when `needs_releasing` is set.
#[repr(C)]
pub struct StringWrapper {
    pub data: *const c_char,
    pub size: usize,
    pub needs_releasing: bool,
}

pub type InitFn = unsafe extern "C" fn(PluginId) -> ErrorCode;
pub type ShutdownFn = unsafe extern "C" fn();
pub type GetStringFn = unsafe extern "C" fn() -> StringWrapper;
pub type GetVersionFn = unsafe extern "C" fn() -> Version;
pub type RegisterApiFunctionsFn = unsafe extern "C" fn(*mut c_void);
pub type ReleaseResourceFn = unsafe extern "C" fn(*const c_void);
pub type SetMumbleInfoFn = unsafe extern "C" fn(Version, Version, Version);
pub type GetFeaturesFn = unsafe extern "C" fn() -> u32;
/// `mumble_initPositionalData(programNames, programPIDs, programCount)`.
pub type InitPositionalDataFn = unsafe extern "C" fn(*const *const c_char, *const u64, usize) -> u8;
/// `mumble_fetchPositionalData(avatarPos, avatarDir, avatarAxis, cameraPos, cameraDir,
/// cameraAxis, context, identity)`: three floats per vector; the strings stay owned by the
/// plugin and valid until its next call.
pub type FetchPositionalDataFn = unsafe extern "C" fn(
    *mut f32,
    *mut f32,
    *mut f32,
    *mut f32,
    *mut f32,
    *mut f32,
    *mut *const c_char,
    *mut *const c_char,
) -> bool;
pub type ShutdownPositionalDataFn = unsafe extern "C" fn();

/// Number of entries in [`API_TABLE`]. The `MumbleAPI` structs are tables of function pointers
/// (about 40 in API 1.2); the spare entries keep plugins built against a newer API from
/// reading past the end.
pub const API_SLOTS: usize = 64;

/// What every `MumbleAPI` function does here: report an error. Every function of the struct
/// returns a `mumble_error_t`, and with the C calling convention the caller cleans up its
/// arguments, so one function that ignores them serves every entry. Game plugins do not need
/// the API (at most they log); an error tells them the information is unavailable, so they
/// never read an out-parameter. Logging is a no-op this way.
extern "C" fn unavailable() -> ErrorCode {
    EC_GENERIC_ERROR
}

/// The `MumbleAPI` handed to `mumble_registerAPIFunctions`; static because it must outlive
/// every plugin.
#[repr(C)]
pub struct ApiTable([extern "C" fn() -> ErrorCode; API_SLOTS]);

pub static API_TABLE: ApiTable = ApiTable([unavailable; API_SLOTS]);

impl ApiTable {
    /// The pointer `mumble_registerAPIFunctions` expects. Plugins copy the struct and never
    /// write to it.
    pub fn as_ptr(&'static self) -> *mut c_void {
        std::ptr::from_ref(self).cast_mut().cast()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_api_function_reports_an_error() {
        for entry in &API_TABLE.0 {
            assert_eq!(entry(), EC_GENERIC_ERROR);
        }
    }

    #[test]
    fn versions_order_like_mumble() {
        assert!(MIN_API_VERSION < API_VERSION);
        assert_eq!(API_VERSION.to_string(), "1.2.0");
    }
}
