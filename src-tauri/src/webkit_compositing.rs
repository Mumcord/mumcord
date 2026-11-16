//! Linux-only WebKitGTK rendering workarounds.
//!
//! WebKitGTK 2.54's damage-based compositing makes the UI flicker while the watch-party video
//! iframe plays. Disabling the compositing mode was verified on a real machine to be both
//! flicker-free and fast (the DMA-BUF renderer switch is flicker-free but slow, CPU rendering
//! still flickers). Setting `MUMCORD_WEBKIT_COMPOSITING=1` keeps WebKit's default, and a
//! `WEBKIT_DISABLE_COMPOSITING_MODE` the user already set is never overwritten.
//!
//! On a Raspberry Pi 4 with Pi OS 13 and WebKitGTK 2.54, the DMA-BUF renderer corrupts the
//! entire window into horizontal stripes, even without accelerated compositing. Disable that
//! renderer only on the reproduced board; other machines keep their existing behavior. An
//! explicit `WEBKIT_DISABLE_DMABUF_RENDERER` (including `0`) is always respected.

const DMABUF_VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
const DISABLE_VAR: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";
const ESCAPE_HATCH_VAR: &str = "MUMCORD_WEBKIT_COMPOSITING";

/// Whether to set `WEBKIT_DISABLE_COMPOSITING_MODE=1`.
fn should_disable(already_set: bool, escape_hatch: Option<&str>) -> bool {
    !already_set && escape_hatch != Some("1")
}

fn is_pi4(compatible: &[u8]) -> bool {
    compatible
        .split(|byte| *byte == 0)
        .any(|entry| entry == b"raspberrypi,4-model-b")
}

fn should_disable_dmabuf(already_set: bool, pi4: bool) -> bool {
    !already_set && pi4
}

fn host_is_pi4() -> bool {
    match std::fs::read("/proc/device-tree/compatible") {
        Ok(compatible) => is_pi4(&compatible),
        // Most Linux PCs have no device tree.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            eprintln!("cannot identify the board for the WebKit DMA-BUF workaround: {err}");
            false
        }
    }
}

/// Must run before WebKit or GTK start and before any other thread exists.
pub fn prepare() {
    let dmabuf_set = std::env::var_os(DMABUF_VAR).is_some();
    if should_disable_dmabuf(dmabuf_set, host_is_pi4()) {
        // SAFETY: same single-threaded startup requirement as the compositing switch below.
        unsafe { std::env::set_var(DMABUF_VAR, "1") };
    }
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
    use super::{is_pi4, should_disable, should_disable_dmabuf};

    #[test]
    fn pi4_device_tree_is_matched_by_complete_entry() {
        assert!(is_pi4(b"raspberrypi,4-model-b\0brcm,bcm2711\0"));
        assert!(is_pi4(b"brcm,bcm2711\0raspberrypi,4-model-b\0"));
        assert!(!is_pi4(b"raspberrypi,5-model-b\0brcm,bcm2712\0"));
        assert!(!is_pi4(b"vendor,board\0brcm,bcm2711\0"));
        assert!(!is_pi4(b"raspberrypi,4-model-b-other\0"));
        assert!(!is_pi4(b""));
    }

    #[test]
    fn dmabuf_workaround_is_pi4_only_and_preserves_user_settings() {
        assert!(should_disable_dmabuf(false, true));
        assert!(!should_disable_dmabuf(false, false));
        assert!(!should_disable_dmabuf(true, true));
        assert!(!should_disable_dmabuf(true, false));
    }

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
