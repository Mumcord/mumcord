//! Loads GPU driver libraries at runtime (dlopen / LoadLibrary). The drivers are never linked, so
//! the app starts on machines without them; a missing library only makes that encoder
//! unavailable.

use libloading::Library;

use super::HardwareEncoder;
use crate::Result;

#[cfg(windows)]
pub(super) const NVENC: &[&str] = &["nvEncodeAPI64.dll"];
#[cfg(not(windows))]
pub(super) const NVENC: &[&str] = &["libnvidia-encode.so.1"];

#[cfg(windows)]
pub(super) const AMF: &[&str] = &["amfrt64.dll"];
#[cfg(not(windows))]
pub(super) const AMF: &[&str] = &["libamfrt64.so.1"];

#[cfg(target_os = "linux")]
pub(super) const VA: &[&str] = &["libva.so.2"];
#[cfg(target_os = "linux")]
pub(super) const VA_DRM: &[&str] = &["libva-drm.so.2"];

/// Loads the first of `names` that exists, or reports `encoder` as unavailable.
pub(super) fn open(encoder: HardwareEncoder, names: &[&str]) -> Result<Library> {
    let mut failures = Vec::new();
    for name in names {
        match load(name) {
            Ok(library) => return Ok(library),
            Err(err) => failures.push(format!("{name}: {err}")),
        }
    }
    Err(encoder.unavailable(format!(
        "driver library not loaded ({})",
        failures.join("; ")
    )))
}

/// Looks up the function or variable `name` in `library`.
///
/// # Safety
/// `T` must be the exact type of the symbol (usually an `unsafe extern "C" fn` pointer).
pub(super) unsafe fn symbol<T: Copy>(
    encoder: HardwareEncoder,
    library: &Library,
    name: &str,
) -> Result<T> {
    // SAFETY: the caller guarantees that `T` matches the symbol.
    unsafe { library.get::<T>(name.as_bytes()) }
        .map(|symbol| *symbol)
        .map_err(|err| encoder.unavailable(format!("driver lacks {name}: {err}")))
}

#[cfg(windows)]
fn load(name: &str) -> std::result::Result<Library, libloading::Error> {
    use libloading::os::windows::{LOAD_LIBRARY_SEARCH_SYSTEM32, Library as WindowsLibrary};
    // Drivers install their DLLs into System32; searching only there keeps a planted DLL next
    // to the app or in the working directory from being loaded instead.
    // SAFETY: loading runs the driver's initialisation code, which the system's own graphics
    // stack runs as well.
    unsafe { WindowsLibrary::load_with_flags(name, LOAD_LIBRARY_SEARCH_SYSTEM32) }.map(Into::into)
}

#[cfg(not(windows))]
fn load(name: &str) -> std::result::Result<Library, libloading::Error> {
    // SAFETY: loading runs the driver's initialisation code, which the system's own graphics
    // stack runs as well.
    unsafe { Library::new(name) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn a_missing_library_makes_the_encoder_unavailable() {
        let err = open(
            HardwareEncoder::Nvenc,
            &["mumcord-no-such-driver-1", "mumcord-no-such-driver-2"],
        )
        .expect_err("no such library");
        let Error::HardwareUnavailable { encoder, reason } = err else {
            panic!("unexpected error {err}");
        };
        assert_eq!(encoder, HardwareEncoder::Nvenc);
        assert!(reason.contains("mumcord-no-such-driver-2"), "{reason}");
    }

    #[test]
    fn loads_a_system_library_and_finds_its_symbols() {
        #[cfg(windows)]
        let (names, function) = (["kernel32.dll"], "GetCurrentProcessId");
        #[cfg(not(windows))]
        let (names, function) = (["libc.so.6"], "getpid");
        let library = open(HardwareEncoder::Amf, &names).expect("system library");
        // SAFETY: both functions take no arguments and return a 32-bit process id.
        let pid = unsafe {
            symbol::<unsafe extern "system" fn() -> u32>(HardwareEncoder::Amf, &library, function)
        }
        .expect("symbol");
        // SAFETY: see above.
        assert_eq!(unsafe { pid() }, std::process::id());
        let missing = unsafe {
            symbol::<unsafe extern "C" fn()>(
                HardwareEncoder::Amf,
                &library,
                "mumcord_no_such_symbol",
            )
        };
        assert!(matches!(missing, Err(Error::HardwareUnavailable { .. })));
    }
}
