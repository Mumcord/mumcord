//! State shared by the Vulkan and OpenGL paths: the background IPC thread and the finished
//! overlay image it publishes for the render thread.
//!
//! The render thread never waits: it compares [`generation`] with what it last uploaded and only
//! then tries to take the image with `try_lock`; if the IPC thread holds the lock that frame
//! keeps the previous image.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};

use mumcord_overlay_ipc::render::{self, Image};
use mumcord_overlay_ipc::wire::{self, Api, Corner, Hello};

/// The latest overlay image and where it goes; `None` = draw nothing.
#[derive(Debug, Clone)]
pub struct Frame {
    pub image: Image,
    pub corner: Corner,
    /// Distance from the screen edges in pixels.
    pub margin: u32,
}

static FRAME: Mutex<Option<Arc<Frame>>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Display scale (f32 bits) the IPC thread renders at; set from the swapchain height.
static DISPLAY_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0
static START: Once = Once::new();

/// Margin between the overlay and the screen edges at a scale of 1.
const MARGIN: f32 = 16.0;

/// Bumped every time the frame changes.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// The current frame, or `Err(())` when the IPC thread is publishing right now (try again next
/// frame). Never blocks.
pub fn try_frame() -> Result<Option<Arc<Frame>>, ()> {
    match FRAME.try_lock() {
        Ok(slot) => Ok(slot.clone()),
        Err(std::sync::TryLockError::Poisoned(p)) => Ok(p.into_inner().clone()),
        Err(std::sync::TryLockError::WouldBlock) => Err(()),
    }
}

fn publish(frame: Option<Frame>) {
    let mut slot = FRAME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *slot = frame.map(Arc::new);
    GENERATION.fetch_add(1, Ordering::Release);
}

/// Scale for a screen `height` pixels tall: 1 up to 1080p, then growing with the height.
pub fn display_scale_for(height: u32) -> f32 {
    // Screen heights are far below f32 precision limits.
    #[allow(clippy::cast_precision_loss)]
    let scale = (height as f32 / 1080.0).max(1.0);
    scale
}

pub fn set_display_height(height: u32) {
    DISPLAY_SCALE.store(display_scale_for(height).to_bits(), Ordering::Relaxed);
}

/// Top-left position of a `size` overlay in `corner` of a `screen`, or `None` when it does not
/// fit (it is then not drawn).
pub fn place(
    corner: Corner,
    margin: u32,
    size: (u32, u32),
    screen: (u32, u32),
) -> Option<(u32, u32)> {
    let free_x = screen.0.checked_sub(size.0)?;
    let free_y = screen.1.checked_sub(size.1)?;
    let left = margin.min(free_x);
    let top = margin.min(free_y);
    let right = free_x.saturating_sub(margin);
    let bottom = free_y.saturating_sub(margin);
    Some(match corner {
        Corner::TopLeft => (left, top),
        Corner::TopRight => (right, top),
        Corner::BottomLeft => (left, bottom),
        Corner::BottomRight => (right, bottom),
    })
}

/// Starts the IPC thread once per process. Errors only mean no overlay.
pub fn start(api: Api) {
    START.call_once(|| {
        let hello = hello(api);
        crate::log::debug!(
            "active: {:?}, exe {:?}, Steam app {:?}",
            hello.api,
            hello.exe,
            hello.steam_app_id
        );
        let spawned = std::thread::Builder::new()
            .name("mumcord-overlay".into())
            .spawn(move || run(&hello));
        // Without the thread there is simply no overlay; the game is unaffected.
        drop(spawned);
    });
}

fn run(hello: &Hello) -> ! {
    let font = render::font();
    mumcord_overlay_ipc::client::run(hello, |update| {
        let frame = match (update, &font) {
            (Some((config, scene)), Some(font)) => {
                let scale = f32::from_bits(DISPLAY_SCALE.load(Ordering::Relaxed));
                render::render(config, scene, font, scale).map(|image| Frame {
                    image,
                    corner: config.corner,
                    // Positive and small.
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    margin: (MARGIN * scale).round() as u32,
                })
            }
            _ => None,
        };
        // Only counts and sizes: names stay out of the log.
        match (update, &frame) {
            (None, _) => crate::log::debug!("Mumcord: not connected or overlay off for this game"),
            (Some((_, scene)), Some(f)) => crate::log::debug!(
                "Mumcord: {} people ({} talking), overlay {}x{}",
                scene.people.len(),
                scene.people.iter().filter(|p| p.talking).count(),
                f.image.width,
                f.image.height
            ),
            (Some((_, scene)), None) => {
                crate::log::debug!("Mumcord: {} people, nothing to draw", scene.people.len());
            }
        }
        publish(frame);
    })
}

fn hello(api: Api) -> Hello {
    let steam_app_id = ["SteamAppId", "SteamGameId"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find_map(|v| v.trim().parse::<u32>().ok().filter(|id| *id > 0));
    Hello {
        version: wire::VERSION,
        api,
        pid: std::process::id(),
        steam_app_id,
        exe: exe_name(),
    }
}

/// The game's executable name. Under Wine/Proton `argv[0]` is the Windows path of the game's
/// `.exe`, which is what Mumcord matches against; natively it is the program path.
fn exe_name() -> String {
    let argv0 = std::fs::read("/proc/self/cmdline")
        .ok()
        .and_then(|raw| {
            raw.split(|b| *b == 0)
                .next()
                .map(|a| String::from_utf8_lossy(a).into_owned())
        })
        .unwrap_or_default();
    base_name(&argv0).to_owned()
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Whether the user switched the overlay off for this process (`MUMCORD_OVERLAY_DISABLE=1`, the
/// same variable the Vulkan manifest names as its disable switch).
pub fn disabled_by_env() -> bool {
    std::env::var_os("MUMCORD_OVERLAY_DISABLE").is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_are_placed_in_corners_with_a_margin() {
        let screen = (1920, 1080);
        let size = (200, 100);
        assert_eq!(place(Corner::TopLeft, 16, size, screen), Some((16, 16)));
        assert_eq!(place(Corner::TopRight, 16, size, screen), Some((1704, 16)));
        assert_eq!(place(Corner::BottomLeft, 16, size, screen), Some((16, 964)));
        assert_eq!(
            place(Corner::BottomRight, 16, size, screen),
            Some((1704, 964))
        );
        // Tight screens shrink the margin; too small ones skip drawing.
        assert_eq!(
            place(Corner::BottomRight, 16, size, (210, 100)),
            Some((0, 0))
        );
        assert_eq!(place(Corner::TopLeft, 16, size, (100, 100)), None);
    }

    #[test]
    fn display_scale_grows_above_1080p() {
        assert!((display_scale_for(720) - 1.0).abs() < f32::EPSILON);
        assert!((display_scale_for(2160) - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn exe_names_come_from_unix_and_windows_paths() {
        assert_eq!(base_name("/usr/bin/vkcube"), "vkcube");
        assert_eq!(
            base_name(r"Z:\games\Foo\Foo-Win64-Shipping.exe"),
            "Foo-Win64-Shipping.exe"
        );
        assert_eq!(base_name("cs2"), "cs2");
    }

    #[test]
    fn frames_are_published_with_a_new_generation() {
        let before = generation();
        publish(None);
        assert!(generation() > before);
        assert!(matches!(try_frame(), Ok(None)));
    }
}
