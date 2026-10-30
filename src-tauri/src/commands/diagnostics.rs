//! Diagnostic file and About information (FR-1.39, FR-1.40).

use std::path::Path;

use mumcord_engine::diag;
use serde::{Deserialize, Serialize};
use specta::Type;

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
}

/// `ID` and `ID_LIKE` from os-release, e.g. `linuxmint` like `ubuntu debian`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct LinuxDistro {
    pub id: String,
    pub id_like: Vec<String>,
}

/// Writes the diagnostic file (no chat contents, passwords, certificates or personal data).
#[tauri::command]
#[specta::specta]
pub fn diagnostics_save(path: String) -> ApiResult<()> {
    let info = diag::collect_info(crate::version::DISPLAY);
    diag::save_diagnostic_file(Path::new(&path), &info).map_err(|e| {
        ApiError::new(
            ErrorCode::Io,
            format!("writing diagnostic file failed: {e}"),
        )
    })
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
