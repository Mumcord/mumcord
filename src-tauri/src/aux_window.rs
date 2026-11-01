//! Windows of their own next to the main window: the packet inspector (issue #60), the
//! third-party licences (issue #138), "What's new" (issue #145) and the watch party and screen
//! share pop-outs (issue #164). Each is open at most once at a time, may call only the few
//! commands it needs, and remembers its size and position on this device.

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

/// What both media pop-outs need: their own window state, the main window's look, and the
/// server state (connections, snapshot, profiles) for the names, avatars and channel the overlay
/// shows. All of it is read-only.
macro_rules! popout_commands {
    ($($extra:literal),* $(,)?) => {
        &[
            "window_look",
            "media_popout_state",
            "media_popout_close",
            "media_popout_set_on_top",
            "media_popout_set_title",
            "connections_list",
            "voice_server_get",
            "session_snapshot",
            "person_profile",
            $($extra),*
        ]
    };
}

/// The watch party player in its own window (issue #164). It drives the party exactly like the
/// docked player: play, pause, seek and skip, its position reports, `link_preview` for the video
/// title and `app_info` for the codec hint.
pub const WATCH_POPOUT: Spec = Spec {
    label: "media-popout-watch",
    url: "index.html?window=media-popout&kind=watch",
    title: "Watch together — Mumcord",
    default_size: (960.0, 600.0),
    min_size: (320.0, 200.0),
    geometry_file: "media-popout-watch-window.json",
    allowed_commands: popout_commands![
        "link_preview",
        "app_info",
        "watch_party_get",
        "watch_party_player_url",
        "watch_party_command",
        "watch_party_report",
        "watch_party_next",
        "watch_party_previous",
    ],
};

/// A watched screen share in its own window (issue #164). It takes over the frames of the
/// running watch (`screen_share_frames_attach`) but can neither start nor stop watching;
/// `p2p_link` names the path in the stream details.
pub const SHARE_POPOUT: Spec = Spec {
    label: "media-popout-share",
    url: "index.html?window=media-popout&kind=share",
    title: "Screen share — Mumcord",
    default_size: (960.0, 600.0),
    min_size: (320.0, 200.0),
    geometry_file: "media-popout-share-window.json",
    allowed_commands: popout_commands![
        "p2p_link",
        "screen_share_list",
        "screen_share_frames_attach",
        "screen_share_request_keyframe",
        "screen_share_set_decode_fallback",
        "screen_share_watch_stats",
        "person_set_volume",
    ],
};

const ALL: [&Spec; 5] = [
    &INSPECTOR,
    &LICENCES,
    &CHANGELOG,
    &WATCH_POPOUT,
    &SHARE_POPOUT,
];

/// The windows that follow the main window's theme and language.
pub const FOLLOW_LOOK: [&Spec; 4] = [&LICENCES, &CHANGELOG, &WATCH_POPOUT, &SHARE_POPOUT];

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
    open_with(app, spec, None, false)
}

/// Like [`open`], with a title in place of the spec's and the always-on-top flag applied to the
/// new or already open window. Wayland compositors may ignore always on top.
pub fn open_with(
    app: &AppHandle,
    spec: &Spec,
    title: Option<&str>,
    on_top: bool,
) -> tauri::Result<()> {
    let title = title.unwrap_or(spec.title);
    let lock = app.state::<OpenLock>();
    let _guard = lock
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(window) = app.get_webview_window(spec.label) {
        window.set_title(title)?;
        window.set_always_on_top(on_top)?;
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    // Starts hidden so the stored size and position apply before it shows up.
    let mut builder = WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(spec.url.into()))
        .title(title)
        .always_on_top(on_top)
        .inner_size(spec.default_size.0, spec.default_size.1)
        .min_inner_size(spec.min_size.0, spec.min_size.1)
        .resizable(true)
        .visible(false);
    if let Some(dir) = crate::portable::webview_data_dir() {
        builder = builder.data_directory(dir);
    }
    let window = builder.build()?;
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
    match crate::portable::data_dir(app) {
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
    fn the_watch_popout_gets_only_the_player_commands() {
        let label = WATCH_POPOUT.label;
        for command in [
            "window_look",
            "media_popout_state",
            "media_popout_close",
            "media_popout_set_on_top",
            "media_popout_set_title",
            "connections_list",
            "voice_server_get",
            "session_snapshot",
            "person_profile",
            "link_preview",
            "app_info",
            "watch_party_get",
            "watch_party_player_url",
            "watch_party_command",
            "watch_party_report",
            "watch_party_next",
            "watch_party_previous",
        ] {
            assert!(command_allowed(label, command), "{command}");
        }
        for command in [
            "media_popout_open",
            "watch_party_start",
            "watch_party_leave",
            "watch_party_add",
            "watch_party_remove",
            "screen_share_frames_attach",
            "send_channel_message",
            "settings_get",
            "settings_update",
            "connect",
        ] {
            assert!(!command_allowed(label, command), "{command}");
        }
    }

    #[test]
    fn the_share_popout_gets_only_the_viewer_commands() {
        let label = SHARE_POPOUT.label;
        for command in [
            "window_look",
            "media_popout_state",
            "media_popout_close",
            "media_popout_set_on_top",
            "media_popout_set_title",
            "connections_list",
            "voice_server_get",
            "session_snapshot",
            "person_profile",
            "p2p_link",
            "screen_share_list",
            "screen_share_frames_attach",
            "screen_share_request_keyframe",
            "screen_share_set_decode_fallback",
            "screen_share_watch_stats",
            "person_set_volume",
        ] {
            assert!(command_allowed(label, command), "{command}");
        }
        for command in [
            "media_popout_open",
            "screen_share_watch",
            "screen_share_unwatch",
            "screen_share_start",
            "screen_share_stop",
            "watch_party_command",
            "person_set_local_mute",
            "settings_get",
            "settings_update",
        ] {
            assert!(!command_allowed(label, command), "{command}");
        }
    }

    #[test]
    fn every_window_has_its_own_label_and_size_file() {
        for (i, a) in ALL.iter().enumerate() {
            assert!(a.url.starts_with("index.html?window="));
            for b in &ALL[i + 1..] {
                assert_ne!(a.label, b.label);
                assert_ne!(a.url, b.url);
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
        assert!(command_allowed("main", "media_popout_open"));
        assert!(command_allowed("main", "screen_share_watch"));
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
