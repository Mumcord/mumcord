//! Debug-only test hooks of the background effects (spec 08 section 10.3, FR-8.41, AC-8.28).
//!
//! Each hook is an environment variable read only when `debug_assertions` is on; release builds
//! always get the inert default, so a user's environment can never change behaviour there.
//!
//! - `MUMCORD_TEST_SEGMENT_MASK=fixture`: the test camera's known person outline replaces the
//!   model output.
//! - `MUMCORD_TEST_EFFECT_STATS=1`: the median effect time is shown in the debug panel (read by
//!   the app through [`effect_stats`]).
//! - `MUMCORD_TEST_ONNX=unavailable`: ONNX Runtime counts as missing, so effects are disabled.
//! - `MUMCORD_TEST_EFFECT_SLOW=<ms>`: adds `<ms>` to every measured effect time, so Auto picks
//!   and steps down to Light as on a slow PC.

use std::time::Duration;

fn var(name: &str) -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    } else {
        None
    }
}

/// Whether the fixture mask replaces the model output.
pub fn segment_mask_fixture() -> bool {
    var("MUMCORD_TEST_SEGMENT_MASK").as_deref() == Some("fixture")
}

/// Whether the debug panel shows the effect time.
pub fn effect_stats() -> bool {
    var("MUMCORD_TEST_EFFECT_STATS").as_deref() == Some("1")
}

/// Whether ONNX Runtime is to be treated as unavailable.
pub fn onnx_unavailable() -> bool {
    var("MUMCORD_TEST_ONNX").as_deref() == Some("unavailable")
}

/// Extra time added to each measured effect time.
pub fn effect_slow() -> Duration {
    var("MUMCORD_TEST_EFFECT_SLOW")
        .as_deref()
        .and_then(parse_millis)
        .unwrap_or_default()
}

fn parse_millis(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_slow_hook() {
        assert_eq!(parse_millis("25"), Some(Duration::from_millis(25)));
        assert_eq!(parse_millis("fast"), None);
    }
}
