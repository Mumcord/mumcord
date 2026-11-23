//! Diagnostic file and About information (FR-1.39, FR-1.40).

use mumcord_engine::diag;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::WebviewWindow;

use super::file_dialog::{SaveDialog, pick_save_path};
use super::types::{ApiError, ApiResult, ErrorCode};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// The display version: `X.Y.Z` for a release, `dev~<commit>` or `dev~YYYYMMDD` otherwise.
    pub version: String,
    /// Numeric `X.Y.Z` (the last release for dev builds), for versions other clients parse.
    pub package_version: String,
    pub os: String,
    /// Running from an AppImage: the UI offers the app-menu entry (FR-2.55).
    pub appimage: bool,
    /// Running in portable mode from a folder with the `portable` marker (Windows, #181): the
    /// About section says so and the update hint asks for the new portable zip.
    pub portable: bool,
    /// The Linux distribution, for the missing-codec install hint (FR-2.41a); `None` on other
    /// systems or without a readable os-release file.
    pub distro: Option<LinuxDistro>,
    /// What this platform cannot offer (FR-13.27): the UI disables those entries and the About
    /// note lists them. Empty where everything works.
    pub limits: Vec<PlatformLimit>,
    /// The desktop session in use, where the platform has more than one (FR-13.31); `None`
    /// elsewhere or when neither `DISPLAY` nor `WAYLAND_DISPLAY` says anything.
    pub session: Option<SessionKind>,
}

/// Which display server the app talks to (FR-13.31, FR-13.21).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    /// X11, also through XWayland on a Wayland session.
    X11,
    /// A Wayland session; hotkeys and capture still go through XWayland.
    Wayland,
}

/// `XDG_SESSION_TYPE` first, then the two display variables; Wayland wins when both are set.
fn session_kind() -> Option<SessionKind> {
    let var = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    match std::env::var("XDG_SESSION_TYPE").ok().as_deref() {
        Some("wayland") => return Some(SessionKind::Wayland),
        Some("x11") => return Some(SessionKind::X11),
        _ => {}
    }
    if var("WAYLAND_DISPLAY") {
        Some(SessionKind::Wayland)
    } else if var("DISPLAY") {
        Some(SessionKind::X11)
    } else {
        None
    }
}

/// A feature this platform cannot provide, with the reason named in the About note.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PlatformLimit {
    /// No camera backend.
    Camera,
    /// No ONNX Runtime for the AI backgrounds.
    CameraEffects,
    /// No PipeWire to take an application's sound into a share.
    ScreenShareSound,
    /// No `xdg-desktop-portal` to pick a window or an app.
    WindowPicker,
    /// No in-game overlay.
    Overlay,
    /// No EME in the platform's WebView (FR-13.38): protected video can only play in a browser.
    ProtectedVideo,
}

/// What this build cannot do, from the platform it is compiled for.
fn platform_limits() -> Vec<PlatformLimit> {
    #[cfg(target_os = "freebsd")]
    {
        vec![
            PlatformLimit::Camera,
            PlatformLimit::CameraEffects,
            PlatformLimit::ScreenShareSound,
            PlatformLimit::WindowPicker,
            PlatformLimit::Overlay,
            PlatformLimit::ProtectedVideo,
        ]
    }
    #[cfg(not(target_os = "freebsd"))]
    {
        Vec::new()
    }
}

/// `ID` and `ID_LIKE` from os-release, e.g. `linuxmint` like `ubuntu debian`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct LinuxDistro {
    pub id: String,
    pub id_like: Vec<String>,
}

/// Asks where to save the diagnostic file (no chat contents, passwords, certificates or
/// personal data) and writes it there. Returns false when the user cancelled the save dialog.
#[tauri::command]
#[specta::specta]
pub async fn diagnostics_save(window: WebviewWindow, dialog: SaveDialog) -> ApiResult<bool> {
    let Some(path) = pick_save_path(&window, dialog).await? else {
        return Ok(false);
    };
    let info = diag::collect_info(crate::version::DISPLAY);
    diag::save_diagnostic_file(&path, &info).map_err(|e| {
        ApiError::new(
            ErrorCode::Io,
            format!("writing diagnostic file failed: {e}"),
        )
    })?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub fn app_info() -> ApiResult<AppInfo> {
    Ok(AppInfo {
        version: crate::version::DISPLAY.to_owned(),
        package_version: crate::version::PACKAGE.to_owned(),
        os: diag::os_description(),
        appimage: appimage(),
        portable: crate::portable::is_active(),
        distro: distro(),
        limits: platform_limits(),
        session: session_kind(),
    })
}

#[cfg(target_os = "linux")]
fn appimage() -> bool {
    crate::desktop_entry::is_appimage()
}

#[cfg(not(target_os = "linux"))]
fn appimage() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn distro() -> Option<LinuxDistro> {
    // The standard location first, then the fallback the os-release spec names.
    ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| parse_os_release(&text))
}

#[cfg(not(target_os = "linux"))]
fn distro() -> Option<LinuxDistro> {
    None
}

/// Reads `ID` and `ID_LIKE` from os-release's shell-style `KEY=value` lines.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_os_release(text: &str) -> Option<LinuxDistro> {
    let value = |key: &str| {
        text.lines()
            .filter_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
            .next_back()
            .map(|raw| {
                raw.trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_owned()
            })
    };
    let id = value("ID").filter(|id| !id.is_empty())?;
    let id_like = value("ID_LIKE")
        .map(|like| like.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    Some(LinuxDistro { id, id_like })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FreeBSD names every gap the specification documents (FR-13.27).
    #[cfg(target_os = "freebsd")]
    #[test]
    fn freebsd_reports_every_platform_limit() {
        let limits = platform_limits();
        assert!(limits.contains(&PlatformLimit::Camera));
        assert!(limits.contains(&PlatformLimit::CameraEffects));
        assert!(limits.contains(&PlatformLimit::ScreenShareSound));
        assert!(limits.contains(&PlatformLimit::WindowPicker));
        assert!(limits.contains(&PlatformLimit::Overlay));
        assert!(limits.contains(&PlatformLimit::ProtectedVideo));
    }

    #[test]
    fn reads_id_and_id_like() {
        let text =
            "NAME=\"Linux Mint\"\nID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_ID=\"22\"\n";
        assert_eq!(
            parse_os_release(text),
            Some(LinuxDistro {
                id: "linuxmint".into(),
                id_like: vec!["ubuntu".into(), "debian".into()],
            })
        );
    }

    #[test]
    fn id_like_is_optional_and_id_is_required() {
        assert_eq!(
            parse_os_release("ID='arch'\n"),
            Some(LinuxDistro {
                id: "arch".into(),
                id_like: Vec::new(),
            })
        );
        assert_eq!(parse_os_release("NAME=Something\nID=\n"), None);
    }
}
