//! Packet inspector for debugging (issue #60): hidden until the user unlocks the developer
//! tools. Captured packets stay in the engine's memory; the UI fetches new ones every 250 ms
//! while the inspector window is open. The main window shares its theme, language and the names
//! it knows with that window, which has no access to the rest of the app.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use mumcord_engine::Engine;
use mumcord_engine::inspector::{InspectorPoll, PacketCount, PacketView, TapDirection, VoiceCount};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use super::session::AudioRatingLabel;
use super::types::{ApiError, ApiResult, ErrorCode, ServerId, SessionId, to_api_server_id};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PacketDirection {
    Sent,
    Received,
}

impl From<TapDirection> for PacketDirection {
    fn from(d: TapDirection) -> Self {
        match d {
            TapDirection::Sent => Self::Sent,
            TapDirection::Received => Self::Received,
        }
    }
}

/// One captured control-channel packet. Everything except `id`, `atMs`, `direction`, `size`
/// and the pacer fields comes from the server or other clients: render it as plain text only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct Packet {
    /// Increases by one per captured packet.
    pub id: u32,
    pub server_id: ServerId,
    /// Capture time in milliseconds since the Unix epoch.
    pub at_ms: f64,
    pub direction: PacketDirection,
    /// Plugin data id (e.g. `mumcord.watch`) or Mumble message name (e.g. `UserState`).
    pub kind: String,
    pub plugin_data: bool,
    /// Bytes of the plugin data, or of the whole message for other types.
    pub size: u32,
    /// Sender session of received plugin data.
    pub sender: Option<SessionId>,
    /// Receiver sessions of sent plugin data.
    pub receivers: Vec<SessionId>,
    /// How long the client pacer held sent plugin data back.
    pub delay_ms: Option<u32>,
    /// Plugin-data messages the pacer still lets through at once after this one.
    pub tokens_left: Option<u32>,
    /// The payload as pretty-printed JSON; `null` if it is neither JSON nor a known format.
    pub decoded: Option<String>,
    /// The plugin data as lowercase hex; empty for other types.
    pub raw_hex: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PacketCounter {
    pub server_id: ServerId,
    pub kind: String,
    pub direction: PacketDirection,
    pub count: u32,
}

/// Voice packets of a connected server since its voice link started.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct VoiceCounter {
    pub server_id: ServerId,
    pub sent: u32,
    pub received: u32,
    /// Audio rating on the server with the voice pipeline (issue #152); `null` elsewhere.
    pub rating: Option<AudioRatingLabel>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PacketCapture {
    pub capturing: bool,
    /// Capture keeps running while the inspector window is closed.
    pub background: bool,
    /// Packets newer than the requested id, oldest first.
    pub packets: Vec<Packet>,
    /// Packets per server, kind and direction since capture started or was cleared.
    pub counters: Vec<PacketCounter>,
    pub voice: Vec<VoiceCounter>,
}

fn saturate(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<PacketView> for Packet {
    fn from(p: PacketView) -> Self {
        Self {
            id: saturate(p.id),
            server_id: to_api_server_id(p.server_id),
            // Milliseconds since 1970 stay exact in an f64 for the next 285,000 years.
            at_ms: p.at_ms as f64,
            direction: p.direction.into(),
            kind: p.kind,
            plugin_data: p.plugin_data,
            size: p.size,
            sender: p.sender,
            receivers: p.receivers,
            delay_ms: p.delay_ms,
            tokens_left: p.tokens_left,
            decoded: p.decoded,
            raw_hex: p.raw_hex,
        }
    }
}

impl From<PacketCount> for PacketCounter {
    fn from(c: PacketCount) -> Self {
        Self {
            server_id: to_api_server_id(c.server_id),
            kind: c.kind,
            direction: c.direction.into(),
            count: saturate(c.count),
        }
    }
}

impl From<VoiceCount> for VoiceCounter {
    fn from(v: VoiceCount) -> Self {
        Self {
            server_id: to_api_server_id(v.server_id),
            sent: saturate(v.sent),
            received: saturate(v.received),
            rating: v.rating.map(|(rating, issue)| AudioRatingLabel {
                rating: rating.into(),
                issue: issue.map(Into::into),
            }),
        }
    }
}

impl PacketCapture {
    fn new(p: InspectorPoll, background: bool) -> Self {
        Self {
            capturing: p.capturing,
            background,
            packets: p.packets.into_iter().map(Into::into).collect(),
            counters: p.counts.into_iter().map(Into::into).collect(),
            voice: p.voice.into_iter().map(Into::into).collect(),
        }
    }
}

/// Why packets are captured: only while the inspector window is open, or, if the user asked for
/// it, also while it is closed. With neither the engine's taps do nothing and nothing is stored.
#[derive(Default)]
pub struct CaptureMode {
    window_open: AtomicBool,
    background: AtomicBool,
}

impl CaptureMode {
    fn wanted(&self) -> bool {
        self.window_open.load(Ordering::Relaxed) || self.background.load(Ordering::Relaxed)
    }

    fn apply(&self, engine: &Engine) {
        engine.set_packet_capture(self.wanted());
    }

    /// The inspector window opened or closed. Closing drops the capture unless it was asked to
    /// continue in the background.
    pub fn set_window_open(&self, engine: &Engine, open: bool) {
        self.window_open.store(open, Ordering::Relaxed);
        self.apply(engine);
    }
}

/// Window event hook: when the inspector window is gone, capture stops (and the buffer is
/// dropped) unless it was asked to continue in the background.
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() == crate::aux_window::INSPECTOR.label
        && matches!(event, tauri::WindowEvent::Destroyed)
    {
        let app = window.app_handle();
        // The engine is gone while the app shuts down; there is nothing left to stop then.
        if let (Some(mode), Some(engine)) =
            (app.try_state::<CaptureMode>(), app.try_state::<Engine>())
        {
            mode.set_window_open(&engine, false);
        }
    }
}

/// The developer tools were unlocked (`true`) or hidden again (`false`). Hiding them stops any
/// capture and closes the inspector window; capture itself starts with the window.
#[tauri::command]
#[specta::specta]
pub fn packet_capture_set(
    app: AppHandle,
    engine: State<'_, Engine>,
    mode: State<'_, CaptureMode>,
    enabled: bool,
) -> ApiResult<()> {
    if !enabled {
        mode.background.store(false, Ordering::Relaxed);
        mode.apply(&engine);
        crate::aux_window::close(&app, &crate::aux_window::INSPECTOR);
    }
    Ok(())
}

/// Keeps capturing while the inspector window is closed (`true`), or stops doing so; turning
/// it off with the window closed drops what was captured.
#[tauri::command]
#[specta::specta]
pub fn packet_capture_background(
    engine: State<'_, Engine>,
    mode: State<'_, CaptureMode>,
    enabled: bool,
) -> ApiResult<()> {
    mode.background.store(enabled, Ordering::Relaxed);
    mode.apply(&engine);
    Ok(())
}

/// Packets captured after the packet with id `after` (all kept packets with `null`), plus the
/// counters.
#[tauri::command]
#[specta::specta]
pub fn packet_capture_poll(
    engine: State<'_, Engine>,
    mode: State<'_, CaptureMode>,
    after: Option<u32>,
) -> ApiResult<PacketCapture> {
    let background = mode.background.load(Ordering::Relaxed);
    Ok(PacketCapture::new(
        engine.poll_packet_capture(after.map(u64::from)),
        background,
    ))
}

/// Drops the captured packets and resets the counters.
#[tauri::command]
#[specta::specta]
pub fn packet_capture_clear(engine: State<'_, Engine>) -> ApiResult<()> {
    engine.clear_packet_capture();
    Ok(())
}

/// Writes an export the UI built from the captured packets (JSON or Markdown) to `path`.
#[tauri::command]
#[specta::specta]
pub fn packet_capture_export(path: String, contents: String) -> ApiResult<()> {
    std::fs::write(&path, contents)
        .map_err(|e| ApiError::new(ErrorCode::Io, format!("writing packet export failed: {e}")))
}

/// A person the main window knows, for the inspector's from / to column.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectorUser {
    pub server_id: ServerId,
    pub session: SessionId,
    pub name: String,
}

/// A saved server's label, for the inspector's server filter.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectorServer {
    pub server_id: ServerId,
    pub label: String,
}

/// What the inspector window takes over from the main window: the look of the page and the
/// names to show.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectorView {
    /// `data-*` attributes of the main window's root element (theme, visual effects, hints).
    pub attributes: BTreeMap<String, String>,
    /// CSS custom properties set on the main window's root element (theme colours).
    pub properties: BTreeMap<String, String>,
    pub language: String,
    pub users: Vec<InspectorUser>,
    pub servers: Vec<InspectorServer>,
}

/// The main window's view changed; sent to the inspector window only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct InspectorViewChanged {
    pub view: InspectorView,
}

/// The last view the main window shared, for an inspector window that opens later.
#[derive(Default)]
pub struct SharedView(Mutex<Option<InspectorView>>);

/// Opens the packet inspector window, or brings it to the front if it is already open.
#[tauri::command]
#[specta::specta]
pub async fn packet_inspector_open(
    app: AppHandle,
    engine: State<'_, Engine>,
    mode: State<'_, CaptureMode>,
) -> ApiResult<()> {
    // Capture starts before the window so its first poll already sees packets.
    mode.set_window_open(&engine, true);
    // Async: creating a window from a synchronous command deadlocks on Windows.
    crate::aux_window::open(&app, &crate::aux_window::INSPECTOR).map_err(|e| {
        mode.set_window_open(&engine, false);
        tracing::warn!(error = %e, "opening the packet inspector window failed");
        ApiError::new(
            ErrorCode::Io,
            format!("opening the packet inspector failed: {e}"),
        )
    })
}

/// Closes the packet inspector window (Ctrl+W in that window).
#[tauri::command]
#[specta::specta]
pub fn packet_inspector_close(app: AppHandle) -> ApiResult<()> {
    crate::aux_window::close(&app, &crate::aux_window::INSPECTOR);
    Ok(())
}

/// Called by the main window whenever its theme, language or known names change.
#[tauri::command]
#[specta::specta]
pub fn packet_inspector_share(
    app: AppHandle,
    shared: State<'_, SharedView>,
    view: InspectorView,
) -> ApiResult<()> {
    *shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(view.clone());
    if let Err(e) =
        (InspectorViewChanged { view }).emit_to(&app, crate::aux_window::INSPECTOR.label)
    {
        tracing::warn!(error = %e, "updating the packet inspector window failed");
    }
    Ok(())
}

/// The view the main window shared last; `null` before it shared one.
#[tauri::command]
#[specta::specta]
pub fn packet_inspector_view(shared: State<'_, SharedView>) -> ApiResult<Option<InspectorView>> {
    Ok(shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_is_wanted_only_with_the_window_open_or_in_the_background() {
        let mode = CaptureMode::default();
        assert!(!mode.wanted(), "nothing is captured by default");
        mode.window_open.store(true, Ordering::Relaxed);
        assert!(mode.wanted());
        mode.window_open.store(false, Ordering::Relaxed);
        assert!(!mode.wanted(), "closing the window stops the capture");
        mode.background.store(true, Ordering::Relaxed);
        assert!(mode.wanted(), "the background switch keeps it going");
    }
}
