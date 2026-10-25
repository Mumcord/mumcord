//! Overlay window (FR-1.44–FR-1.46): a separate transparent, undecorated, always-on-top window
//! that lets clicks pass through and shows who is talking (page: `ui/overlay.html`).
//!
//! It never hooks into games; it is an ordinary top-most window, so it shows over windowed and
//! borderless-fullscreen apps but not over exclusive-fullscreen games. The window only exists
//! while the overlay is enabled, so a disabled overlay costs nothing.

use std::sync::atomic::{AtomicBool, Ordering};

use mumcord_engine::{Engine, EngineError, store};
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tauri_specta::Event;

use crate::commands::overlay::{OverlayChanged, OverlayState, state_of};

/// Label of the overlay window.
pub const LABEL: &str = "overlay";

/// The only commands the overlay page may call: the server state it renders, avatars, its own
/// options and leaving move mode. Everything else is refused for it in `may_invoke`.
const OVERLAY_COMMANDS: &[&str] = &[
    "connection_state",
    "session_snapshot",
    "person_profile",
    "overlay_state",
    "overlay_move_mode",
];

/// Distance between the overlay and the screen edges in a corner, logical pixels.
const MARGIN: f64 = 16.0;

/// Whether the webview `label` may call `command`. Windows other than the overlay are not
/// restricted here (their access is the main window's capability set).
pub fn may_invoke(label: &str, command: &str) -> bool {
    label != LABEL || OVERLAY_COMMANDS.contains(&command)
}

/// Move mode flag; only meaningful while the window exists.
#[derive(Default)]
struct Runtime {
    move_mode: AtomicBool,
}

/// What this desktop lets the overlay do (FR-1.46, AC-1.34).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    pub on_top: bool,
    pub position: bool,
    /// i18n key (namespace `overlay`) explaining the limitation, if any.
    pub explanation_key: Option<&'static str>,
}

const FULL_SUPPORT: Support = Support {
    on_top: true,
    position: true,
    explanation_key: None,
};

/// Linux: X11 (and XWayland, i.e. compatibility mode) honours "keep above" and window
/// positions. A native Wayland window can neither ask to stay on top nor place itself; the
/// compositor decides. KDE can force both with a window rule, so it gets its own hint.
#[cfg(target_os = "linux")]
fn support_for(env: &crate::ptt::SessionEnv, current_desktop: Option<&str>) -> Support {
    if !env.runs_on_wayland() {
        return FULL_SUPPORT;
    }
    let kde = is_kde(current_desktop);
    Support {
        on_top: false,
        position: false,
        explanation_key: Some(if kde {
            "limits.waylandKde"
        } else {
            "limits.wayland"
        }),
    }
}

#[cfg(target_os = "linux")]
fn is_kde(current_desktop: Option<&str>) -> bool {
    current_desktop.is_some_and(|desktops| {
        desktops
            .split(':')
            .any(|d| d.trim().eq_ignore_ascii_case("KDE"))
    })
}

/// KDE Plasma, on any session type.
pub fn kde() -> bool {
    #[cfg(target_os = "linux")]
    return is_kde(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref());
    #[cfg(not(target_os = "linux"))]
    false
}

/// KDE Plasma on native Wayland: the one place where a KWin window rule can fix the overlay.
#[cfg(target_os = "linux")]
pub fn kde_wayland() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    crate::ptt::SessionEnv::current().runs_on_wayland() && is_kde(desktop.as_deref())
}

/// `(kde_wayland, our KWin rule exists)` for the settings UI.
pub fn kwin_rule_state() -> (bool, bool) {
    #[cfg(target_os = "linux")]
    if kde_wayland() {
        let installed = crate::kwin_rule::config_path().is_some_and(|path| {
            crate::kwin_rule::installed_in(&path).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "reading the KWin rules failed");
                false
            })
        });
        return (true, installed);
    }
    (false, false)
}

/// Where the corner (or the size) puts the overlay on the primary screen, in the logical
/// pixels KWin rules use. The dragged place is not used: Wayland cannot report it.
#[cfg(target_os = "linux")]
pub fn kwin_placement(
    app: &AppHandle,
    overlay: &store::OverlaySettings,
) -> Result<crate::kwin_rule::Placement, String> {
    let monitor = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no screen found".to_owned())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let origin = area.position.to_logical::<f64>(scale);
    let extent = area.size.to_logical::<f64>(scale);
    // Rounded logical pixels are far below i32::MAX / u32::MAX.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (x, y) = corner_position(
        overlay.corner,
        Rect {
            x: origin.x.round() as i32,
            y: origin.y.round() as i32,
            width: extent.width.round() as u32,
            height: extent.height.round() as u32,
        },
        (overlay.width, overlay.height),
        1.0,
    );
    Ok(crate::kwin_rule::Placement {
        x,
        y,
        width: overlay.width,
        height: overlay.height,
    })
}

/// Keeps an installed KWin rule in step with the chosen corner and size.
#[cfg(target_os = "linux")]
fn refresh_kwin_rule(app: &AppHandle, overlay: &store::OverlaySettings) {
    if !kde_wayland() {
        return;
    }
    let Some(path) = crate::kwin_rule::config_path() else {
        return;
    };
    if !crate::kwin_rule::installed_in(&path).unwrap_or(false) {
        return;
    }
    let result = kwin_placement(app, overlay).and_then(|placement| {
        crate::kwin_rule::install_in(&path, placement).map_err(|e| e.to_string())
    });
    match result {
        Ok(true) => {
            tauri::async_runtime::spawn(async {
                if let Err(e) = crate::kwin_rule::reconfigure().await {
                    tracing::warn!(error = %e, "asking KWin to reload its rules failed");
                }
            });
        }
        Ok(false) => {}
        Err(e) => tracing::warn!(error = %e, "updating the KWin rule failed"),
    }
}

/// Commands that open KDE's window rule settings, tried in order.
#[cfg(target_os = "linux")]
const KDE_WINDOW_RULES_ARGV: [[&str; 2]; 2] = [
    ["systemsettings", "kcm_kwinrules"],
    ["kcmshell6", "kwinrules"],
];

/// Commands that open KDE's Legacy X11 App Support page, tried in order.
#[cfg(target_os = "linux")]
const KDE_X11_SETTINGS_ARGV: [[&str; 2]; 2] = [
    ["kcmshell6", "kwinxwayland"],
    ["systemsettings", "kcm_kwinxwayland"],
];

/// Starts the first command that exists. Fixed argv, no shell.
#[cfg(target_os = "linux")]
fn spawn_first(candidates: &[[&str; 2]]) -> std::io::Result<()> {
    use std::process::Command;
    let mut last_error = None;
    for [program, arg] in candidates {
        match Command::new(program).arg(arg).spawn() {
            Ok(mut child) => {
                // Reaped in the background so the finished settings window leaves no zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("no command to start")))
}

/// Opens KDE's window rule settings.
#[cfg(target_os = "linux")]
pub fn open_kde_window_rules() -> std::io::Result<()> {
    spawn_first(&KDE_WINDOW_RULES_ARGV)
}

/// Opens KDE's Legacy X11 App Support settings (FR-1.48).
#[cfg(target_os = "linux")]
pub fn open_kde_x11_settings() -> std::io::Result<()> {
    spawn_first(&KDE_X11_SETTINGS_ARGV)
}

/// What the current desktop supports.
#[cfg(target_os = "linux")]
pub fn support() -> Support {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    support_for(&crate::ptt::SessionEnv::current(), desktop.as_deref())
}

/// Windows and macOS keep a top-most window above borderless-fullscreen apps.
#[cfg(not(target_os = "linux"))]
pub fn support() -> Support {
    FULL_SUPPORT
}

/// Registers the overlay state and opens the overlay if it was enabled at the last quit.
pub fn init(app: &AppHandle) {
    app.manage(Runtime::default());
    match app.state::<Engine>().settings() {
        Ok(stored) if stored.overlay.enabled => {
            apply(app, &stored);
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "reading the overlay settings failed"),
    }
}

/// Whether move mode is on.
pub fn move_mode(app: &AppHandle) -> bool {
    app.try_state::<Runtime>()
        .is_some_and(|r| r.move_mode.load(Ordering::Relaxed))
}

fn set_move_flag(app: &AppHandle, on: bool) {
    if let Some(runtime) = app.try_state::<Runtime>() {
        runtime.move_mode.store(on, Ordering::Relaxed);
    }
}

/// Brings the window in line with `stored` (open, close, place, resize), updates the tray
/// and tells both pages. Window failures are logged: the options stay stored either way.
pub fn apply(app: &AppHandle, stored: &store::Settings) -> OverlayState {
    let overlay = &stored.overlay;
    let existing = app.get_webview_window(LABEL);
    if overlay.enabled {
        let result = match existing {
            Some(window) => place(app, &window, overlay),
            None => open(app, overlay),
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, "showing the overlay failed");
        }
    } else {
        set_move_flag(app, false);
        if let Some(window) = existing
            && let Err(e) = window.destroy()
        {
            tracing::warn!(error = %e, "closing the overlay failed");
        }
    }
    #[cfg(target_os = "linux")]
    refresh_kwin_rule(app, overlay);
    let state = state_of(stored, move_mode(app));
    announce(app, &state);
    state
}

/// Sends the current state to both pages and the tray, e.g. after a language change.
pub fn notify(app: &AppHandle) {
    match app.state::<Engine>().settings() {
        Ok(stored) => announce(app, &state_of(&stored, move_mode(app))),
        Err(e) => tracing::warn!(error = %e, "reading the overlay settings failed"),
    }
}

fn announce(app: &AppHandle, state: &OverlayState) {
    crate::tray::overlay_changed(app, state.settings.enabled);
    if let Err(e) = (OverlayChanged {
        state: state.clone(),
    })
    .emit(app)
    {
        tracing::warn!(error = %e, "announcing the overlay state failed");
    }
}

/// Enables or disables the overlay and stores the choice.
pub async fn set_enabled(
    app: &AppHandle,
    engine: &Engine,
    enabled: bool,
) -> Result<OverlayState, EngineError> {
    let mut stored = engine.settings()?;
    stored.overlay.enabled = enabled;
    let stored = engine.update_settings(stored).await?;
    Ok(apply(app, &stored))
}

/// Switches move mode. Leaving it stores where the user dragged the overlay.
pub async fn set_move_mode(
    app: &AppHandle,
    engine: &Engine,
    active: bool,
) -> Result<OverlayState, EngineError> {
    let mut stored = engine.settings()?;
    let Some(window) = app.get_webview_window(LABEL) else {
        set_move_flag(app, false);
        return Ok(state_of(&stored, false));
    };
    if active == move_mode(app) {
        return Ok(state_of(&stored, active));
    }
    set_move_flag(app, active);
    if let Err(e) = take_clicks(&window, active) {
        tracing::warn!(error = %e, "switching the overlay move mode failed");
    }
    // Without positioning support the reported position is meaningless (always 0, 0).
    if !active && support().position {
        match window.outer_position() {
            Ok(p) => {
                stored.overlay.custom_position = Some(store::OverlayPoint { x: p.x, y: p.y });
                stored = engine.update_settings(stored).await?;
            }
            Err(e) => tracing::warn!(error = %e, "reading the overlay position failed"),
        }
    }
    let state = state_of(&stored, active);
    announce(app, &state);
    Ok(state)
}

/// Click-through off (move mode) or on. The window may take focus only in move mode, so
/// Escape and the Done button work there.
fn take_clicks(window: &WebviewWindow, take: bool) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    set_input_region(window, take)?;
    #[cfg(not(target_os = "linux"))]
    window.set_ignore_cursor_events(!take)?;
    window.set_focusable(take)
}

/// Linux: Tauri's click-through still leaves a 1×1 px input area in the top-left corner that
/// catches clicks (QA.61), so the overlay sets an empty input region itself. GTK keeps the
/// region with the widget and reapplies it whenever the window is realized again.
#[cfg(target_os = "linux")]
fn set_input_region(window: &WebviewWindow, take: bool) -> tauri::Result<()> {
    let target = window.clone();
    window.run_on_main_thread(move || {
        use gtk::prelude::WidgetExt;

        match target.gtk_window() {
            Ok(gtk_window) if take => gtk_window.input_shape_combine_region(None),
            Ok(gtk_window) => {
                gtk_window.input_shape_combine_region(Some(&gdk::cairo::Region::create()));
            }
            Err(e) => tracing::warn!(error = %e, "setting the overlay input region failed"),
        }
    })
}

fn open(app: &AppHandle, overlay: &store::OverlaySettings) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
        .title("Mumcord overlay")
        .inner_size(f64::from(overlay.width), f64::from(overlay.height))
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .resizable(false)
        .disable_drag_drop_handler()
        // Shown after placing it, so it never flashes at the default position. Click-through
        // is set after showing: GTK needs the realized window for its input region.
        .visible(false)
        .build()?;
    place(app, &window, overlay)?;
    window.show()?;
    take_clicks(&window, move_mode(app))
}

/// Resizes the window and moves it to the stored place: the dragged position if it is still
/// on a screen, else the chosen corner of the primary screen.
fn place(
    app: &AppHandle,
    window: &WebviewWindow,
    overlay: &store::OverlaySettings,
) -> tauri::Result<()> {
    window.set_size(LogicalSize::new(
        f64::from(overlay.width),
        f64::from(overlay.height),
    ))?;
    if !support().position {
        return Ok(());
    }
    if let Some(p) = overlay.custom_position
        && app
            .monitor_from_point(f64::from(p.x), f64::from(p.y))?
            .is_some()
    {
        return window.set_position(PhysicalPosition::new(p.x, p.y));
    }
    let Some(monitor) = app.primary_monitor()? else {
        return Ok(());
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let size = LogicalSize::new(f64::from(overlay.width), f64::from(overlay.height))
        .to_physical::<u32>(scale);
    let (x, y) = corner_position(
        overlay.corner,
        Rect {
            x: area.position.x,
            y: area.position.y,
            width: area.size.width,
            height: area.size.height,
        },
        (size.width, size.height),
        scale,
    );
    window.set_position(PhysicalPosition::new(x, y))
}

/// A screen area in physical pixels.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// Top-left position of a window of `size` (physical pixels) in `corner` of `area`.
fn corner_position(
    corner: store::OverlayCorner,
    area: Rect,
    size: (u32, u32),
    scale: f64,
) -> (i32, i32) {
    // Pixel counts are far below i32::MAX; the casts cannot truncate in practice.
    #[allow(clippy::cast_possible_truncation)]
    let margin = (MARGIN * scale).round() as i32;
    let free_x = i32::try_from(area.width.saturating_sub(size.0)).unwrap_or(0);
    let free_y = i32::try_from(area.height.saturating_sub(size.1)).unwrap_or(0);
    let left = area.x + margin.min(free_x);
    let top = area.y + margin.min(free_y);
    let right = area.x + (free_x - margin).max(0);
    let bottom = area.y + (free_y - margin).max(0);
    match corner {
        store::OverlayCorner::TopLeft => (left, top),
        store::OverlayCorner::TopRight => (right, top),
        store::OverlayCorner::BottomLeft => (left, bottom),
        store::OverlayCorner::BottomRight => (right, bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_may_only_call_its_own_commands() {
        assert!(may_invoke(LABEL, "session_snapshot"));
        assert!(may_invoke(LABEL, "overlay_move_mode"));
        assert!(!may_invoke(LABEL, "send_channel_message"));
        assert!(!may_invoke(LABEL, "overlay_update"));
        assert!(!may_invoke(LABEL, "identity_export"));
        assert!(may_invoke("main", "identity_export"));
    }

    #[test]
    fn corners_keep_a_margin_inside_the_work_area() {
        let area = Rect {
            x: 1920,
            y: 30,
            width: 2560,
            height: 1410,
        };
        let size = (480, 640);
        let at = |corner| corner_position(corner, area, size, 2.0);
        assert_eq!(at(store::OverlayCorner::TopLeft), (1952, 62));
        assert_eq!(
            at(store::OverlayCorner::TopRight),
            (1920 + 2560 - 480 - 32, 62)
        );
        assert_eq!(
            at(store::OverlayCorner::BottomLeft),
            (1952, 30 + 1410 - 640 - 32)
        );
        assert_eq!(
            at(store::OverlayCorner::BottomRight),
            (1920 + 2560 - 480 - 32, 30 + 1410 - 640 - 32)
        );
    }

    #[test]
    fn an_overlay_bigger_than_the_screen_stays_at_its_origin() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 300,
            height: 200,
        };
        assert_eq!(
            corner_position(store::OverlayCorner::BottomRight, area, (400, 400), 1.0),
            (0, 0)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_x11_settings_try_kcmshell6_then_systemsettings() {
        assert_eq!(KDE_X11_SETTINGS_ARGV[0], ["kcmshell6", "kwinxwayland"]);
        assert_eq!(
            KDE_X11_SETTINGS_ARGV[1],
            ["systemsettings", "kcm_kwinxwayland"]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn spawning_reports_an_error_when_no_command_exists() {
        let missing = [
            ["mumcord-no-such-program-1", "a"],
            ["mumcord-no-such-program-2", "b"],
        ];
        assert!(spawn_first(&missing).is_err());
        assert!(spawn_first(&[]).is_err());
    }

    #[cfg(target_os = "linux")]
    fn env(session: &str, gdk: Option<&str>) -> crate::ptt::SessionEnv {
        crate::ptt::SessionEnv {
            session_type: Some(session.to_owned()),
            wayland_display: (session == "wayland").then(|| "wayland-0".to_owned()),
            gdk_backend: gdk.map(str::to_owned),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_and_xwayland_support_everything() {
        assert_eq!(support_for(&env("x11", None), Some("KDE")), FULL_SUPPORT);
        // Compatibility mode: Mumcord runs through XWayland, X11 rules apply.
        assert_eq!(
            support_for(&env("wayland", Some("x11")), Some("KDE")),
            FULL_SUPPORT
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_wayland_explains_the_limitation() {
        let kde = support_for(&env("wayland", None), Some("KDE"));
        assert!(!kde.on_top && !kde.position);
        assert_eq!(kde.explanation_key, Some("limits.waylandKde"));
        let gnome = support_for(&env("wayland", None), Some("ubuntu:GNOME"));
        assert!(!gnome.on_top);
        assert_eq!(gnome.explanation_key, Some("limits.wayland"));
        assert_eq!(
            support_for(&env("wayland", None), None).explanation_key,
            Some("limits.wayland")
        );
    }
}
