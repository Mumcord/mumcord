//! Linux only: turns off WebKitGTK's accelerated compositing mode.
//!
//! WebKitGTK 2.54's damage-based compositing makes the UI flicker while the watch-party video
//! iframe plays. Disabling the compositing mode was verified on a real machine to be both
//! flicker-free and fast (the DMA-BUF renderer switch is flicker-free but slow, CPU rendering
//! still flickers). Setting `MUMCORD_WEBKIT_COMPOSITING=1` keeps WebKit's default, and a
//! `WEBKIT_DISABLE_COMPOSITING_MODE` the user already set is never overwritten.

const DISABLE_VAR: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";
const ESCAPE_HATCH_VAR: &str = "MUMCORD_WEBKIT_COMPOSITING";

/// Whether to set `WEBKIT_DISABLE_COMPOSITING_MODE=1`.
fn should_disable(already_set: bool, escape_hatch: Option<&str>) -> bool {
    !already_set && escape_hatch != Some("1")
}

/// Must run before WebKit or GTK start and before any other thread exists.
pub fn prepare() {
    let already_set = std::env::var_os(DISABLE_VAR).is_some();
    let escape_hatch = std::env::var(ESCAPE_HATCH_VAR).ok();
    if should_disable(already_set, escape_hatch.as_deref()) {
        // SAFETY: called at the very start of `run`, before Tauri, GTK or the async runtime
        // start any thread, so nothing reads the environment concurrently.
        unsafe { std::env::set_var(DISABLE_VAR, "1") };
    }
}

#[cfg(test)]
mod tests {
    use super::should_disable;

    #[test]
    fn already_set_is_left_untouched() {
        assert!(!should_disable(true, None));
    }

    #[test]
    fn escape_hatch_skips() {
        assert!(!should_disable(false, Some("1")));
    }

    #[test]
    fn otherwise_sets() {
        assert!(should_disable(false, None));
        assert!(should_disable(false, Some("0")));
    }
}
