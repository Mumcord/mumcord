//! KDE Plasma detection and the one settings page Mumcord opens for it (FR-1.48).

/// KDE Plasma, on any session type.
pub fn kde() -> bool {
    #[cfg(target_os = "linux")]
    return std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .is_some_and(|desktops| {
            desktops
                .split(':')
                .any(|d| d.trim().eq_ignore_ascii_case("KDE"))
        });
    #[cfg(not(target_os = "linux"))]
    false
}

/// Commands that open KDE's Legacy X11 App Support page, tried in order.
#[cfg(target_os = "linux")]
const KDE_X11_SETTINGS_ARGV: [[&str; 2]; 2] = [
    ["kcmshell6", "kwinxwayland"],
    ["systemsettings", "kcm_kwinxwayland"],
];

/// Opens KDE's Legacy X11 App Support settings. Fixed argv, no shell; the first command that
/// exists wins.
#[cfg(target_os = "linux")]
pub fn open_kde_x11_settings() -> std::io::Result<()> {
    use std::process::Command;
    let mut last_error = None;
    for [program, arg] in KDE_X11_SETTINGS_ARGV {
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
