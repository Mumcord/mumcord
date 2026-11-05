//! Debug-only test hooks for the end-to-end tests of file sharing and screen sharing.
//!
//! Each hook is an environment variable read only when `debug_assertions` is on. In release
//! builds every function returns its inert default, so a user's environment can never change
//! behaviour there. The pure `parse_*` helpers are always compiled so they stay unit-testable.

use std::time::Duration;

/// Outcome the encoder probe is told to report for one encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeResult {
    /// The encoder is found and its smoke test passes.
    Ok,
    /// The encoder is found but its smoke test fails.
    SmokeFail,
    /// The encoder is not found.
    Absent,
}

/// Forced probe result for one encoder, e.g. `av1_hw:smoke_fail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOverride {
    pub encoder: String,
    pub result: ProbeResult,
}

/// Forced result of the software AV1 (SVT) self-test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvtSelftest {
    Pass,
    Fail,
}

/// What the pretend system picker hands over (issue #435, AC-7.61 to AC-7.64): the test
/// backend has no window picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharePick {
    /// A whole screen.
    Screen,
    /// A window, with the app hint the portal or compositor would report (`None`: no hint).
    Window(Option<String>),
}

/// Reads a hook variable; always `None` in release builds.
fn var(name: &str) -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    } else {
        None
    }
}

fn flag(name: &str) -> bool {
    var(name).as_deref() == Some("1")
}

/// Offer lifetime override (normally 24 h) for AC-7.15.
pub fn file_expiry() -> Option<Duration> {
    parse_seconds(var("MUMCORD_TEST_FILE_EXPIRY_S")?.as_str())
}

/// Flip one byte of a file in transit (AC-7.11).
pub fn file_corrupt() -> bool {
    flag("MUMCORD_TEST_FILE_CORRUPT")
}

/// Stop sending file bytes after this many bytes (F3a (e)). Only the file stream pauses, the
/// client stays connected to the server, so the recipient's stall timer is the one that ends the
/// transfer. See the flake in <https://github.com/Mumcord/mumcord/issues/420>.
pub fn file_stall_after_bytes() -> Option<u64> {
    parse_number(var("MUMCORD_TEST_FILE_STALL_AFTER")?.as_str())
}

/// Forced encoder probe results (AC-7.36, AC-7.37); `None` means probe for real.
pub fn encoder_probe() -> Option<Vec<ProbeOverride>> {
    parse_probe_list(var("MUMCORD_TEST_ENCODER_PROBE")?.as_str())
}

/// Forced software AV1 self-test result; `None` means run the real test.
pub fn svt_selftest() -> Option<SvtSelftest> {
    parse_svt(var("MUMCORD_TEST_SVT_SELFTEST")?.as_str())
}

/// Artificial delay added to every encoded frame (AC-7.38).
pub fn encode_delay() -> Option<Duration> {
    parse_millis(var("MUMCORD_TEST_ENCODE_DELAY_MS")?.as_str())
}

/// Pretend the machine runs on battery (AC-7.40).
pub fn on_battery() -> bool {
    flag("MUMCORD_TEST_ON_BATTERY")
}

/// Pretend the webview cannot decode AV1 via WebCodecs (AC-7.41).
pub fn no_webcodecs_av1() -> bool {
    flag("MUMCORD_TEST_NO_WEBCODECS_AV1")
}

/// Keep using the previous stream key after a rekey (AC-7.30).
pub fn keep_old_key() -> bool {
    flag("MUMCORD_TEST_KEEP_OLD_KEY")
}

/// Enables the `lockScreen` test command, which simulates a screen lock (spec 08, AC-8.18).
pub fn screen_lock() -> bool {
    flag("MUMCORD_TEST_SCREEN_LOCK")
}

/// The pick the test backend pretends the user made: `MUMCORD_TEST_SHARE_PICK=screen`,
/// `window` or `window:<app hint>`. `None` keeps the plain test share without sound capture.
pub fn share_pick() -> Option<SharePick> {
    parse_pick(var("MUMCORD_TEST_SHARE_PICK")?.as_str())
}

fn parse_seconds(raw: &str) -> Option<Duration> {
    raw.trim().parse().ok().map(Duration::from_secs)
}

fn parse_millis(raw: &str) -> Option<Duration> {
    raw.trim().parse().ok().map(Duration::from_millis)
}

fn parse_number(raw: &str) -> Option<u64> {
    raw.trim().parse().ok()
}

fn parse_pick(raw: &str) -> Option<SharePick> {
    let raw = raw.trim();
    match raw.split_once(':') {
        None if raw == "screen" => Some(SharePick::Screen),
        None if raw == "window" => Some(SharePick::Window(None)),
        Some(("window", hint)) if !hint.trim().is_empty() => {
            Some(SharePick::Window(Some(hint.trim().to_owned())))
        }
        _ => None,
    }
}

fn parse_svt(raw: &str) -> Option<SvtSelftest> {
    match raw.trim() {
        "pass" => Some(SvtSelftest::Pass),
        "fail" => Some(SvtSelftest::Fail),
        _ => None,
    }
}

/// Parses `encoder[:result],…`; a bare name means `ok`. Returns `None` if any entry is invalid,
/// so a typo in a test setup is noticed instead of silently probing for real.
fn parse_probe_list(raw: &str) -> Option<Vec<ProbeOverride>> {
    raw.split(',')
        .map(|entry| {
            let (encoder, result) = entry.trim().split_once(':').unwrap_or((entry.trim(), "ok"));
            let result = match result {
                "ok" => ProbeResult::Ok,
                "smoke_fail" => ProbeResult::SmokeFail,
                "absent" => ProbeResult::Absent,
                _ => return None,
            };
            (!encoder.is_empty()).then(|| ProbeOverride {
                encoder: encoder.to_owned(),
                result,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_list_parses_results() {
        assert_eq!(
            parse_probe_list("av1_hw:smoke_fail, vp9_hw"),
            Some(vec![
                ProbeOverride {
                    encoder: "av1_hw".into(),
                    result: ProbeResult::SmokeFail
                },
                ProbeOverride {
                    encoder: "vp9_hw".into(),
                    result: ProbeResult::Ok
                },
            ])
        );
    }

    #[test]
    fn probe_list_rejects_invalid_entries() {
        assert!(parse_probe_list("av1_hw:nope").is_none());
        assert!(parse_probe_list(":ok").is_none());
        assert!(parse_probe_list("").is_none());
    }

    #[test]
    fn pick_parses_screen_and_window_hints() {
        assert_eq!(parse_pick("screen"), Some(SharePick::Screen));
        assert_eq!(parse_pick("window"), Some(SharePick::Window(None)));
        assert_eq!(
            parse_pick("window: Test App A"),
            Some(SharePick::Window(Some("Test App A".into())))
        );
        assert_eq!(parse_pick("window:"), None);
        assert_eq!(parse_pick("monitor"), None);
    }

    #[test]
    fn numbers_and_svt_parse() {
        assert_eq!(parse_seconds(" 60 "), Some(Duration::from_secs(60)));
        assert_eq!(parse_millis("x"), None);
        assert_eq!(parse_number(" 1048576 "), Some(1048576));
        assert_eq!(parse_number("x"), None);
        assert_eq!(parse_svt("fail"), Some(SvtSelftest::Fail));
        assert_eq!(parse_svt("maybe"), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn hooks_read_environment_in_debug() {
        // Unique variable (keep_old_key) so parallel tests don't race on shared env.
        // SAFETY: only this test touches this variable.
        unsafe { std::env::set_var("MUMCORD_TEST_KEEP_OLD_KEY", "1") };
        assert!(keep_old_key());
        unsafe { std::env::remove_var("MUMCORD_TEST_KEEP_OLD_KEY") };
        assert!(!keep_old_key());
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn hooks_are_inert_in_release() {
        // SAFETY: only this test touches these variables.
        unsafe {
            std::env::set_var("MUMCORD_TEST_FILE_STALL_AFTER", "1048576");
            std::env::set_var("MUMCORD_TEST_FILE_EXPIRY_S", "60");
            std::env::set_var("MUMCORD_TEST_FILE_CORRUPT", "1");
            std::env::set_var("MUMCORD_TEST_ENCODER_PROBE", "av1_hw:smoke_fail");
            std::env::set_var("MUMCORD_TEST_SVT_SELFTEST", "fail");
            std::env::set_var("MUMCORD_TEST_ENCODE_DELAY_MS", "50");
            std::env::set_var("MUMCORD_TEST_ON_BATTERY", "1");
            std::env::set_var("MUMCORD_TEST_NO_WEBCODECS_AV1", "1");
            std::env::set_var("MUMCORD_TEST_KEEP_OLD_KEY", "1");
            std::env::set_var("MUMCORD_TEST_SHARE_PICK", "screen");
        }
        assert!(share_pick().is_none());
        assert!(file_expiry().is_none());
        assert!(!file_corrupt());
        assert!(file_stall_after_bytes().is_none());
        assert!(encoder_probe().is_none());
        assert!(svt_selftest().is_none());
        assert!(encode_delay().is_none());
        assert!(!on_battery());
        assert!(!no_webcodecs_av1());
        assert!(!keep_old_key());
    }
}
