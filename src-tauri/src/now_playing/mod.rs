//! Reads the track playing in the system's media player for "Now playing" (FR-1.58) and hands
//! it to the engine, which shares it. Linux and FreeBSD: MPRIS over the session D-Bus. Windows: the system
//! media session (GlobalSystemMediaTransportControls). macOS: the private MediaRemote framework,
//! read through Apple's `osascript` (issue #658). The player is read only while the setting is on, off the
//! UI thread, every `POLL_INTERVAL`; the engine decides what changed.

#[cfg(target_os = "windows")]
mod gsmtc;
#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod mpris;

use std::sync::atomic::{AtomicBool, Ordering};

use mumcord_engine::Engine;
use mumcord_engine::now_playing::POLL_INTERVAL;

/// Whether this platform can read the media player.
const SUPPORTED: bool = cfg!(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "windows",
    target_os = "macos"
));

/// Set once a reader learns that this system gives no access after all (an older macOS).
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);

/// Whether the media player can be read here, as far as is known so far.
pub fn supported() -> bool {
    SUPPORTED && !unavailable()
}

fn unavailable() -> bool {
    UNAVAILABLE.load(Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
fn set_unavailable() {
    UNAVAILABLE.store(true, Ordering::Relaxed);
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
type Source = mpris::Reader;
#[cfg(target_os = "windows")]
type Source = gsmtc::Reader;
#[cfg(target_os = "macos")]
type Source = macos::Reader;
#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "windows",
    target_os = "macos"
)))]
type Source = Unsupported;

/// No media information on this platform.
#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "windows",
    target_os = "macos"
)))]
#[derive(Default)]
struct Unsupported;

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "windows",
    target_os = "macos"
)))]
impl Unsupported {
    // Same shape as the real readers.
    async fn read(&mut self) -> Option<mumcord_engine::LocalTrack> {
        None
    }

    fn release(&mut self) {}
}

/// Polls the media player for the engine as long as the app runs.
pub fn start(engine: Engine) {
    tauri::async_runtime::spawn(async move {
        let mut source = Source::default();
        let mut tick = tokio::time::interval(POLL_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let track = if engine.now_playing_enabled() {
                source.read().await
            } else {
                source.release();
                None
            };
            engine.now_playing_update(track);
        }
    });
}
