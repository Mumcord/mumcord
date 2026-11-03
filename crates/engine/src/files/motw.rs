//! Mark-of-the-web for received files (FR-7.27): the operating system then treats the file as
//! coming from the internet (Windows SmartScreen, macOS Gatekeeper). Linux has no blocking mark,
//! so there the attribute only records where the file came from.

use std::io;
use std::path::Path;

/// Marks `path` as downloaded from the Mumble server `host`. Failing is logged without the file
/// name (it may be personal) and never fails the download.
pub(crate) fn mark(path: &Path, host: &str) {
    if let Err(error) = set_mark(path, host) {
        tracing::warn!(%error, "could not set the mark-of-the-web on a received file");
    }
}

#[cfg(windows)]
fn set_mark(path: &Path, _host: &str) -> io::Result<()> {
    // Zone 3 is "Internet"; the stream is what Explorer and SmartScreen read.
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n")
}

#[cfg(target_os = "macos")]
fn set_mark(path: &Path, _host: &str) -> io::Result<()> {
    xattr::set(
        path,
        "com.apple.quarantine",
        quarantine_value(now_unix()).as_bytes(),
    )
}

/// `flags;hex timestamp;agent;` as written by browsers; 0x0081 = "downloaded, user not asked yet".
#[cfg(any(target_os = "macos", test))]
fn quarantine_value(unix_time: u64) -> String {
    format!("0081;{unix_time:08x};Mumcord;")
}

#[cfg(target_os = "macos")]
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn set_mark(path: &Path, host: &str) -> io::Result<()> {
    xattr::set(path, "user.xdg.origin.url", origin_url(host).as_bytes())
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn origin_url(host: &str) -> String {
    format!("mumble://{host}")
}

#[cfg(not(any(unix, windows)))]
fn set_mark(_path: &Path, _host: &str) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn quarantine_value_has_browser_format() {
        assert_eq!(quarantine_value(0x6543_2100), "0081;65432100;Mumcord;");
    }

    #[test]
    fn origin_is_a_mumble_url() {
        assert_eq!(
            origin_url("voice.example.org"),
            "mumble://voice.example.org"
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_sets_origin_attribute_where_supported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        // Some file systems (e.g. older tmpfs) don't allow user attributes; then there's nothing
        // to check, and `mark` must still not panic.
        mark(&file, "voice.example.org");
        if xattr::SUPPORTED_PLATFORM
            && let Ok(Some(value)) = xattr::get(&file, "user.xdg.origin.url")
        {
            assert_eq!(value, b"mumble://voice.example.org");
        }
    }
}
