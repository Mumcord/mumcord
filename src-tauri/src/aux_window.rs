//! Windows of their own next to the main window: the packet inspector (issue #60), the
//! third-party licences (issue #138) and "What's new" (issue #145). Each is opened from Settings, at most one at a time, may
//! call only the few commands it needs, and remembers its size and position on this device.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

/// One kind of window; the UI renders only its content for the `window` query parameter.
pub struct Spec {
    pub label: &'static str,
    url: &'static str,
    title: &'static str,
    default_size: (f64, f64),
    min_size: (f64, f64),
    geometry_file: &'static str,
    /// Commands the window may call. Every other command is refused for it, so the window
    /// cannot connect, send messages or change settings even if its page misbehaves.
    allowed_commands: &'static [&'static str],
}

pub const INSPECTOR: Spec = Spec {
    label: "packet-inspector",
    url: "index.html?window=packet-inspector",
    title: "Packet inspector — Mumcord",
    default_size: (1100.0, 700.0),
    min_size: (640.0, 400.0),
    geometry_file: "packet-inspector-window.json",
    allowed_commands: &[
        "app_info",
        "packet_capture_poll",
        "packet_capture_background",
        "packet_capture_clear",
        "packet_capture_export",
        "packet_inspector_view",
        "packet_inspector_close",
    ],
};

pub const LICENCES: Spec = Spec {
    label: "licences",
    url: "index.html?window=licences",
    title: "Third-party licences — Mumcord",
    default_size: (900.0, 700.0),
    min_size: (480.0, 360.0),
    geometry_file: "licences-window.json",
    allowed_commands: &["window_look", "licences_close", "copy_text", "open_link"],
};

pub const CHANGELOG: Spec = Spec {
    label: "changelog",
    url: "index.html?window=changelog",
    title: "What's new — Mumcord",
    default_size: (900.0, 760.0),
    min_size: (480.0, 360.0),
    geometry_file: "changelog-window.json",
    // `app_info` for the version the window marks as current.
    allowed_commands: &[
        "window_look",
        "changelog_close",
        "copy_text",
        "open_link",
        "app_info",
    ],
};

const ALL: [&Spec; 3] = [&INSPECTOR, &LICENCES, &CHANGELOG];

/// The windows that follow the main window's theme and language.
pub const FOLLOW_LOOK: [&Spec; 2] = [&LICENCES, &CHANGELOG];

/// Whether the webview `label` may call `command`; only the windows above are limited.
pub fn command_allowed(label: &str, command: &str) -> bool {
    ALL.iter()
        .find(|spec| spec.label == label)
        .is_none_or(|spec| spec.allowed_commands.contains(&command))
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

/// Brings the window up, creating it if it is not open yet.
pub fn open(app: &AppHandle, spec: &Spec) -> tauri::Result<()> {
    let lock = app.state::<OpenLock>();
    let _guard = lock
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(window) = app.get_webview_window(spec.label) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    // Starts hidden so the stored size and position apply before it shows up.
    let window = WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(spec.url.into()))
        .title(spec.title)
        .inner_size(spec.default_size.0, spec.default_size.1)
        .min_inner_size(spec.min_size.0, spec.min_size.1)
        .resizable(true)
        .visible(false)
        .build()?;
    if let Some(geometry) = load_geometry(app, spec) {
        restore(&window, geometry);
    }
    window.show()?;
    window.set_focus()
}

/// Closes the window if it is open.
pub fn close(app: &AppHandle, spec: &Spec) {
    if let Some(window) = app.get_webview_window(spec.label)
        && let Err(e) = window.close()
    {
        tracing::warn!(error = %e, window = spec.label, "closing a window failed");
    }
}

/// Window event hook registered on the Tauri builder: keeps the geometry of a closing window.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let Some(spec) = ALL.iter().find(|spec| spec.label == window.label())
        && matches!(event, WindowEvent::CloseRequested { .. })
    {
        save_geometry(window, spec);
    }
}

/// Keeps the geometry of the open windows; quitting skips their close request.
pub fn save_before_exit(app: &AppHandle) {
    for spec in ALL {
        if let Some(window) = app.get_webview_window(spec.label) {
            save_geometry(&window.as_ref().window(), spec);
        }
    }
}

fn geometry_path<R: Runtime>(app: &AppHandle<R>, spec: &Spec) -> Option<PathBuf> {
    match app.path().app_data_dir() {
        Ok(dir) => Some(dir.join(spec.geometry_file)),
        Err(e) => {
            tracing::warn!(error = %e, window = spec.label, "no data folder for the window size");
            None
        }
    }
}

fn load_geometry(app: &AppHandle, spec: &Spec) -> Option<Geometry> {
    let path = geometry_path(app, spec)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(error = %e, window = spec.label, "reading the window size failed");
            return None;
        }
    };
    serde_json::from_str(&text)
        .inspect_err(|e| {
            tracing::warn!(error = %e, window = spec.label, "the stored window size is invalid");
        })
        .ok()
}

fn save_geometry<R: Runtime>(window: &Window<R>, spec: &Spec) {
    // A minimized window reports a meaningless position.
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        tracing::warn!(window = spec.label, "reading the window size failed");
        return;
    };
    let geometry = Geometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    let Some(path) = geometry_path(window.app_handle(), spec) else {
        return;
    };
    let result = serde_json::to_string(&geometry)
        .map_err(std::io::Error::other)
        .and_then(|text| std::fs::write(&path, text));
    if let Err(e) = result {
        tracing::warn!(error = %e, window = spec.label, "saving the window size failed");
    }
}

fn restore(window: &WebviewWindow, geometry: Geometry) {
    if let Err(e) = window.set_size(PhysicalSize::new(geometry.width, geometry.height)) {
        tracing::warn!(error = %e, "restoring a window size failed");
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
        tracing::warn!(error = %e, "restoring a window position failed");
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
        let label = INSPECTOR.label;
        assert!(command_allowed(label, "packet_capture_poll"));
        assert!(command_allowed(label, "packet_inspector_close"));
        assert!(command_allowed(label, "packet_capture_background"));
        assert!(!command_allowed(label, "packet_capture_set"));
        assert!(!command_allowed(label, "packet_inspector_open"));
        assert!(!command_allowed(label, "deep_link_take_pending"));
        assert!(!command_allowed(label, "settings_update"));
    }

    #[test]
    fn the_licences_window_gets_only_its_commands() {
        let label = LICENCES.label;
        assert!(command_allowed(label, "window_look"));
        assert!(command_allowed(label, "licences_close"));
        assert!(command_allowed(label, "copy_text"));
        assert!(command_allowed(label, "open_link"));
        assert!(!command_allowed(label, "licences_open"));
        assert!(!command_allowed(label, "window_look_share"));
        assert!(!command_allowed(label, "app_info"));
        assert!(!command_allowed(label, "packet_capture_poll"));
        assert!(!command_allowed(label, "settings_update"));
    }

    #[test]
    fn the_changelog_window_gets_only_its_commands() {
        let label = CHANGELOG.label;
        assert!(command_allowed(label, "window_look"));
        assert!(command_allowed(label, "changelog_close"));
        assert!(command_allowed(label, "copy_text"));
        assert!(command_allowed(label, "open_link"));
        assert!(command_allowed(label, "app_info"));
        assert!(!command_allowed(label, "changelog_open"));
        assert!(!command_allowed(label, "licences_close"));
        assert!(!command_allowed(label, "window_look_share"));
        assert!(!command_allowed(label, "settings_get"));
        assert!(!command_allowed(label, "settings_update"));
    }

    #[test]
    fn every_window_has_its_own_label_and_size_file() {
        for (i, a) in ALL.iter().enumerate() {
            assert!(a.url.ends_with(a.label));
            for b in &ALL[i + 1..] {
                assert_ne!(a.label, b.label);
                assert_ne!(a.geometry_file, b.geometry_file);
            }
        }
    }

    #[test]
    fn other_windows_are_not_limited() {
        assert!(command_allowed("main", "settings_update"));
        assert!(command_allowed("main", "packet_inspector_share"));
        assert!(command_allowed("main", "licences_open"));
        assert!(command_allowed("main", "changelog_open"));
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
