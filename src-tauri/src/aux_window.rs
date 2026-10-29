//! The packet inspector's own window (issue #60): opened from Settings, at most one at a time,
//! with access to only the few commands it needs. It remembers its size and position on this
//! device.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

/// Label of the inspector window; the UI renders the inspector alone for this window.
pub const LABEL: &str = "packet-inspector";
const URL: &str = "index.html?window=packet-inspector";
const TITLE: &str = "Packet inspector — Mumcord";
const DEFAULT_SIZE: (f64, f64) = (1100.0, 700.0);
const MIN_SIZE: (f64, f64) = (640.0, 400.0);
const GEOMETRY_FILE: &str = "packet-inspector-window.json";

/// Commands the inspector window may call. Every other command is refused for it, so the
/// window cannot connect, send messages or change settings even if its page misbehaves.
const ALLOWED_COMMANDS: &[&str] = &[
    "app_info",
    "packet_capture_poll",
    "packet_capture_clear",
    "packet_capture_export",
    "packet_inspector_view",
    "packet_inspector_close",
];

/// Whether the webview `label` may call `command`; only the inspector window is limited.
pub fn command_allowed(label: &str, command: &str) -> bool {
    label != LABEL || ALLOWED_COMMANDS.contains(&command)
}

/// Serializes opening so two quick clicks cannot both create the window.
#[derive(Default)]
pub struct OpenLock(Mutex<()>);

/// Size and position of the window when it was last closed, in physical pixels.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
struct Geometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// Brings the inspector window up, creating it if it is not open yet.
pub fn open(app: &AppHandle) -> tauri::Result<()> {
    let lock = app.state::<OpenLock>();
    let _guard = lock
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(window) = app.get_webview_window(LABEL) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    // Starts hidden so the stored size and position apply before it shows up.
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(URL.into()))
        .title(TITLE)
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .resizable(true)
        .visible(false)
        .build()?;
    if let Some(geometry) = load_geometry(app) {
        restore(&window, geometry);
    }
    window.show()?;
    window.set_focus()
}

/// Closes the inspector window if it is open.
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL)
        && let Err(e) = window.close()
    {
        tracing::warn!(error = %e, "closing the packet inspector window failed");
    }
}

/// Window event hook registered on the Tauri builder: keeps the geometry of a closing window.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() == LABEL && matches!(event, WindowEvent::CloseRequested { .. }) {
        save_geometry(window);
    }
}

/// Keeps the geometry of an open inspector window; quitting skips its close request.
pub fn save_before_exit(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        save_geometry(&window.as_ref().window());
    }
}

fn geometry_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    match app.path().app_data_dir() {
        Ok(dir) => Some(dir.join(GEOMETRY_FILE)),
        Err(e) => {
            tracing::warn!(error = %e, "no data folder for the packet inspector window size");
            None
        }
    }
}

fn load_geometry(app: &AppHandle) -> Option<Geometry> {
    let path = geometry_path(app)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(error = %e, "reading the packet inspector window size failed");
            return None;
        }
    };
    serde_json::from_str(&text)
        .inspect_err(|e| {
            tracing::warn!(error = %e, "the stored packet inspector window size is invalid");
        })
        .ok()
}

fn save_geometry<R: Runtime>(window: &Window<R>) {
    // A minimized window reports a meaningless position.
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        tracing::warn!("reading the packet inspector window size failed");
        return;
    };
    let geometry = Geometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    let Some(path) = geometry_path(window.app_handle()) else {
        return;
    };
    let result = serde_json::to_string(&geometry)
        .map_err(std::io::Error::other)
        .and_then(|text| std::fs::write(&path, text));
    if let Err(e) = result {
        tracing::warn!(error = %e, "saving the packet inspector window size failed");
    }
}

fn restore(window: &WebviewWindow, geometry: Geometry) {
    if let Err(e) = window.set_size(PhysicalSize::new(geometry.width, geometry.height)) {
        tracing::warn!(error = %e, "restoring the packet inspector window size failed");
    }
    // A monitor that is gone now would leave the window off screen; the OS places it then.
    let monitors = window.available_monitors().unwrap_or_default();
    let on_screen = monitors.iter().any(|m| {
        let (pos, size) = (m.position(), m.size());
        contains(
            (pos.x, pos.y, size.width, size.height),
            (geometry.x, geometry.y),
        )
    });
    if on_screen && let Err(e) = window.set_position(PhysicalPosition::new(geometry.x, geometry.y))
    {
        tracing::warn!(error = %e, "restoring the packet inspector window position failed");
    }
}

/// Whether `point` lies inside the rectangle `(x, y, width, height)`.
fn contains(rect: (i32, i32, u32, u32), point: (i32, i32)) -> bool {
    let (x, y, width, height) = rect;
    let (px, py) = (i64::from(point.0), i64::from(point.1));
    px >= i64::from(x)
        && py >= i64::from(y)
        && px < i64::from(x) + i64::from(width)
        && py < i64::from(y) + i64::from(height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inspector_window_gets_only_its_commands() {
        assert!(command_allowed(LABEL, "packet_capture_poll"));
        assert!(command_allowed(LABEL, "packet_inspector_close"));
        assert!(!command_allowed(LABEL, "packet_capture_set"));
        assert!(!command_allowed(LABEL, "packet_inspector_open"));
        assert!(!command_allowed(LABEL, "deep_link_take_pending"));
        assert!(!command_allowed(LABEL, "settings_update"));
    }

    #[test]
    fn other_windows_are_not_limited() {
        assert!(command_allowed("main", "settings_update"));
        assert!(command_allowed("main", "packet_inspector_share"));
    }

    #[test]
    fn a_position_must_lie_on_a_monitor() {
        let monitor = (0, 0, 1920, 1080);
        assert!(contains(monitor, (100, 100)));
        assert!(!contains(monitor, (1920, 100)));
        assert!(!contains(monitor, (-5, 100)));
        assert!(contains((-1920, 0, 1920, 1080), (-5, 100)));
    }

    #[test]
    fn geometry_round_trips() {
        let geometry = Geometry {
            x: -20,
            y: 40,
            width: 1100,
            height: 700,
        };
        let text = serde_json::to_string(&geometry).expect("serializes");
        assert_eq!(serde_json::from_str::<Geometry>(&text).ok(), Some(geometry));
    }
}
