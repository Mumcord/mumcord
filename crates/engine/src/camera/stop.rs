//! The stop rules of the own camera (spec 08, FR-8.21 – FR-8.24).
//!
//! The camera turns off, releases the device, ends the broadcast and announces `off` when the user
//! leaves or is moved out of the voice channel, switches or leaves the voice server, disconnects,
//! quits, or locks the screen (with "Turn my camera off when my screen locks" on). It also turns
//! off when the device disappears or the relay goes away; those stops carry the "Camera stopped"
//! notice in [`CameraView::error`](super::CameraView). It never turns back on by itself, and a
//! minimised window changes nothing (FR-8.22).
//!
//! A lost connection is handled where the session ends (`session::on_connection_reset`): the
//! channel and the broadcast are gone with it, so nothing is left to announce.

use std::sync::{Mutex, Once};

use mumcord_camera::CameraError;
use mumcord_p2p::BroadcastEnd;

use super::send::SendEnded;
use super::session::{CameraFailure, CameraState, ServerCameras};
use crate::ServerId;
use crate::session::{Engine, WeakEngine, lock};

/// Why the own camera turns off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Cancel, the camera button, the tray or the hotkey.
    TurnedOff,
    /// The user left or was moved out of the voice channel (FR-8.21, FR-8.50).
    LeftChannel,
    /// The user switched the voice server or left voice (FR-8.21).
    VoiceServerChanged,
    /// Mumcord quits (FR-8.21).
    Quit,
    /// The screen locked while "Turn my camera off when my screen locks" is on (FR-8.21).
    ScreenLocked,
    /// The capture or encoder pipeline ended on its own (FR-8.23, FR-8.26).
    DeviceLost(SendEnded),
    /// The relay became unavailable, or its link ended with `closed`, `idle` or without an `end`
    /// (FR-8.7, FR-8.24).
    RelayLost,
    /// The relay ended the broadcast with `deleted` or `revoked` (FR-8.24).
    RelayEnded,
}

impl StopReason {
    /// The relay's `end` of the own broadcast.
    pub(crate) fn from_end(end: BroadcastEnd) -> Self {
        match end {
            BroadcastEnd::Deleted | BroadcastEnd::Revoked => Self::RelayEnded,
            BroadcastEnd::Closed | BroadcastEnd::Idle | BroadcastEnd::Lost => Self::RelayLost,
        }
    }

    /// The "Camera stopped" notice the off view carries (section 5.6); the user's own actions
    /// carry none.
    pub fn failure(self) -> Option<CameraFailure> {
        match self {
            Self::TurnedOff
            | Self::LeftChannel
            | Self::VoiceServerChanged
            | Self::Quit
            | Self::ScreenLocked => None,
            Self::DeviceLost(SendEnded::DeviceGone) => {
                Some(CameraFailure::Camera(CameraError::DeviceGone))
            }
            Self::DeviceLost(SendEnded::Unreadable) => {
                Some(CameraFailure::Camera(CameraError::Unreadable))
            }
            Self::DeviceLost(SendEnded::CameraFailed) => Some(CameraFailure::Camera(
                CameraError::Backend("the camera stopped".into()),
            )),
            Self::DeviceLost(SendEnded::EncoderFailed) => Some(CameraFailure::Camera(
                CameraError::Backend("no video encoder".into()),
            )),
            Self::RelayLost => Some(CameraFailure::RelayDown),
            Self::RelayEnded => Some(CameraFailure::RelayEnded),
        }
    }
}

impl Engine {
    /// Ends the own camera on `server_id` for `reason` (no-op when it does not run there). Must
    /// not be called while the session core is locked; use [`Self::camera_stop_later`] there.
    pub fn camera_stop(&self, server_id: ServerId, reason: StopReason) {
        if reason != StopReason::TurnedOff
            && self
                .camera_own()
                .is_some_and(|(server, _)| server == server_id)
        {
            tracing::info!(?reason, "camera stop rule");
        }
        self.camera_end(server_id, reason.failure());
    }

    /// [`Self::camera_stop`] on the engine's runtime, for callers holding engine locks.
    fn camera_stop_later(&self, server_id: ServerId, reason: StopReason) {
        let engine = self.clone();
        self.runtime()
            .spawn(async move { engine.camera_stop(server_id, reason) });
    }

    /// Ends the own camera wherever it runs (quit, screen lock).
    fn camera_stop_own(&self, reason: StopReason) {
        if let Some((server_id, _)) = self.camera_own() {
            self.camera_stop(server_id, reason);
        }
    }

    /// Mumcord quits: the camera announces `off` before the connections drain (FR-8.21).
    pub(crate) fn camera_quit(&self) {
        self.camera_stop_own(StopReason::Quit);
    }

    /// The screen locked (the OS listener or the `lockScreen` test command): the camera turns off
    /// when "Turn my camera off when my screen locks" is on (FR-8.21, AC-8.18).
    pub fn camera_screen_locked(&self) {
        if self.camera_own().is_none() {
            return;
        }
        if self.encoder_settings().camera_off_on_lock {
            self.camera_stop_own(StopReason::ScreenLocked);
        } else {
            tracing::debug!("screen locked; the camera stays on (setting off)");
        }
    }

    /// The `lockScreen` test command: simulates a screen lock when the debug hook
    /// `MUMCORD_TEST_SCREEN_LOCK=1` is set (spec 08, section 10.3). False when inert.
    pub fn camera_test_lock_screen(&self) -> bool {
        if !crate::test_hooks::screen_lock() {
            return false;
        }
        self.camera_screen_locked();
        true
    }

    /// Camera availability on `server_id` became `unavailable`: a camera on air there loses its
    /// relay (FR-8.7, FR-8.24). The caller may hold engine locks.
    pub(crate) fn camera_relay_unavailable(&self, server_id: ServerId) {
        if self
            .camera_own()
            .is_some_and(|(server, state)| server == server_id && state == CameraState::OnAir)
        {
            self.camera_stop_later(server_id, StopReason::RelayLost);
        }
    }
}

/// The voice server changed to `voice` (switched or left): a camera on another server turns off
/// (FR-8.21). Called with the session core locked, so the stop runs on the runtime.
pub(crate) fn on_voice_server_changed(state: &ServerCameras, voice: Option<ServerId>) {
    let Some(engine) = lock(state).engine() else {
        return;
    };
    if let Some((server_id, _)) = engine.camera_own()
        && Some(server_id) != voice
    {
        engine.camera_stop_later(server_id, StopReason::VoiceServerChanged);
    }
}

/// Engines told about screen locks; one OS listener serves the whole process.
static LOCK_LISTENERS: Mutex<Vec<WeakEngine>> = Mutex::new(Vec::new());
static LOCK_WATCH: Once = Once::new();

/// Starts the OS screen-lock listener once per process and registers `engine` with it. Called
/// when a camera opens, so nothing listens while cameras are unused.
pub(crate) fn watch_screen_lock(engine: &Engine) {
    {
        let mut listeners = lock(&LOCK_LISTENERS);
        listeners.retain(|e| e.upgrade().is_some());
        if !listeners.iter().any(|e| e.is(engine)) {
            listeners.push(engine.downgrade());
        }
    }
    LOCK_WATCH.call_once(|| start_platform_watch(engine));
}

#[cfg(target_os = "linux")]
fn start_platform_watch(engine: &Engine) {
    super::lock_linux::watch(engine.runtime(), screen_locked);
}

#[cfg(target_os = "macos")]
fn start_platform_watch(_engine: &Engine) {
    super::lock_macos::watch(screen_locked);
}

#[cfg(windows)]
fn start_platform_watch(_engine: &Engine) {
    super::lock_windows::watch(screen_locked);
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn start_platform_watch(_engine: &Engine) {}

/// The OS reported a screen lock. Runs on the listener's thread (the macOS main thread, the
/// Windows message loop), so the work moves to each engine's runtime.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn screen_locked() {
    let engines: Vec<Engine> = lock(&LOCK_LISTENERS)
        .iter()
        .filter_map(WeakEngine::upgrade)
        .collect();
    for engine in engines {
        let runtime = engine.runtime().clone();
        runtime.spawn(async move { engine.camera_screen_locked() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_actions_carry_no_notice() {
        for reason in [
            StopReason::TurnedOff,
            StopReason::LeftChannel,
            StopReason::VoiceServerChanged,
            StopReason::Quit,
            StopReason::ScreenLocked,
        ] {
            assert_eq!(reason.failure(), None, "{reason:?}");
        }
    }

    #[test]
    fn device_and_relay_stops_carry_the_spec_texts() {
        let code = |r: StopReason| r.failure().map(|f| f.code());
        assert_eq!(
            code(StopReason::DeviceLost(SendEnded::DeviceGone)),
            Some("device-gone")
        );
        assert_eq!(
            code(StopReason::DeviceLost(SendEnded::Unreadable)),
            Some("unreadable")
        );
        assert_eq!(
            code(StopReason::DeviceLost(SendEnded::EncoderFailed)),
            Some("camera-failed")
        );
        assert_eq!(code(StopReason::RelayLost), Some("relay-down"));
        assert_eq!(code(StopReason::RelayEnded), Some("relay-ended"));
    }

    #[test]
    fn relay_end_reasons_map_to_the_two_notices() {
        assert_eq!(
            StopReason::from_end(BroadcastEnd::Deleted),
            StopReason::RelayEnded
        );
        assert_eq!(
            StopReason::from_end(BroadcastEnd::Revoked),
            StopReason::RelayEnded
        );
        for end in [BroadcastEnd::Closed, BroadcastEnd::Idle, BroadcastEnd::Lost] {
            assert_eq!(StopReason::from_end(end), StopReason::RelayLost, "{end:?}");
        }
    }
}
