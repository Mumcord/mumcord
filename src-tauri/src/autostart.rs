//! Start Mumcord when the user logs in (FR-1.48d, QA.17).
//!
//! Uses each OS's own per-user mechanism: an XDG autostart entry on Linux, a LaunchAgent on
//! macOS and the `Run` registry key on Windows. The entry starts Mumcord with [`FLAG`], so a
//! start at login can go straight to the tray when "Start minimized" is on. The stored setting
//! is the truth: every start rewrites the entry (the program may have moved, e.g. a newer
//! AppImage in another folder) or removes one that was left behind.

use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;

use mumcord_engine::store;
use tauri::AppHandle;

/// Command-line flag the autostart entry passes.
pub const FLAG: &str = "--autostart";

/// Whether these program arguments come from the autostart entry.
pub fn launched_at_login<I>(args: I) -> bool
where
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
{
    args.into_iter().any(|arg| arg.as_ref() == FLAG)
}

/// A start at login with "Start minimized" on keeps the window hidden. A manual start always
/// shows it, and so does a stale entry the user already turned off.
pub fn start_hidden(launched_at_login: bool, settings: &store::Settings) -> bool {
    launched_at_login && settings.autostart && settings.start_minimized
}

/// The program the OS should start: the AppImage file (its mount path changes on every run),
/// else the running binary.
#[cfg(target_os = "linux")]
pub fn program(app: &AppHandle) -> io::Result<PathBuf> {
    use tauri::Manager;
    match app.env().appimage {
        Some(appimage) => Ok(PathBuf::from(appimage)),
        None => tauri::utils::platform::current_exe(),
    }
}

#[cfg(not(target_os = "linux"))]
fn program(_app: &AppHandle) -> io::Result<PathBuf> {
    tauri::utils::platform::current_exe()
}

/// Adds (`enabled`) or removes the OS autostart entry.
pub fn apply(app: &AppHandle, enabled: bool) -> io::Result<()> {
    os::apply(app, &program(app)?, enabled)
}

/// Brings the OS entry in line with the stored setting; runs off the main thread because it
/// touches files or the registry.
pub fn sync(app: &AppHandle, enabled: bool) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("autostart".into())
        .spawn(move || {
            if let Err(e) = apply(&app, enabled) {
                tracing::warn!(error = %e, enabled, "syncing the autostart entry failed");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "starting the autostart sync failed");
    }
}

/// An autostart entry stored as a file (Linux, macOS).
#[cfg(any(target_os = "linux", target_os = "macos"))]
struct FileEntry {
    path: PathBuf,
    content: String,
    /// Text only Mumcord's own entries contain, so a file the user wrote is never touched.
    marker: &'static str,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl FileEntry {
    fn apply(&self, enabled: bool) -> io::Result<()> {
        let current = match std::fs::read_to_string(&self.path) {
            Ok(content) => Some(content),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let ours = current.as_deref().is_none_or(|c| c.contains(self.marker));
        if !ours {
            tracing::info!(path = %self.path.display(), "keeping an autostart entry not written by Mumcord");
            return Ok(());
        }
        match (enabled, current) {
            (true, Some(current)) if current == self.content => {}
            (true, _) => {
                if let Some(dir) = self.path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&self.path, &self.content)?;
                tracing::info!(path = %self.path.display(), "autostart entry written");
            }
            (false, Some(_)) => {
                std::fs::remove_file(&self.path)?;
                tracing::info!(path = %self.path.display(), "autostart entry removed");
            }
            (false, None) => {}
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod os {
    use std::io;
    use std::path::{Path, PathBuf};

    use tauri::{AppHandle, Manager};

    use super::{FLAG, FileEntry};
    use crate::desktop_entry::{MANAGED_KEY, exec_program};

    /// `$XDG_CONFIG_HOME/autostart/<app id>.desktop` (XDG Autostart spec).
    pub(super) fn entry(config_home: &Path, app_id: &str, program: &Path) -> FileEntry {
        FileEntry {
            path: config_home
                .join("autostart")
                .join(format!("{app_id}.desktop")),
            content: format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=Mumcord\n\
                 Comment=Voice chat compatible with Mumble\n\
                 Exec={} {FLAG}\n\
                 Icon={app_id}\n\
                 Terminal=false\n\
                 X-GNOME-Autostart-enabled=true\n\
                 {MANAGED_KEY}\n",
                exec_program(&program.to_string_lossy())
            ),
            marker: MANAGED_KEY,
        }
    }

    pub(super) fn apply(app: &AppHandle, program: &Path, enabled: bool) -> io::Result<()> {
        let config_home: PathBuf = app.path().config_dir().map_err(io::Error::other)?;
        entry(&config_home, &app.config().identifier, program).apply(enabled)
    }
}

#[cfg(target_os = "macos")]
mod os {
    use std::io;
    use std::path::Path;

    use tauri::{AppHandle, Manager};

    use super::{FLAG, FileEntry};

    const MARKER: &str = "<!-- X-Mumcord-Managed -->";

    fn xml_escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    /// `~/Library/LaunchAgents/<app id>.plist`: launchd loads it at the next login. It is not
    /// loaded right away, because `RunAtLoad` would then start a second Mumcord now.
    pub(super) fn entry(home: &Path, app_id: &str, program: &Path) -> FileEntry {
        let app_id = xml_escape(app_id);
        let program = xml_escape(&program.to_string_lossy());
        FileEntry {
            path: home
                .join("Library/LaunchAgents")
                .join(format!("{app_id}.plist")),
            content: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 {MARKER}\n\
                 <plist version=\"1.0\">\n\
                 <dict>\n\
                 \t<key>Label</key>\n\
                 \t<string>{app_id}</string>\n\
                 \t<key>ProgramArguments</key>\n\
                 \t<array>\n\
                 \t\t<string>{program}</string>\n\
                 \t\t<string>{FLAG}</string>\n\
                 \t</array>\n\
                 \t<key>RunAtLoad</key>\n\
                 \t<true/>\n\
                 \t<key>ProcessType</key>\n\
                 \t<string>Interactive</string>\n\
                 </dict>\n\
                 </plist>\n"
            ),
            marker: MARKER,
        }
    }

    pub(super) fn apply(app: &AppHandle, program: &Path, enabled: bool) -> io::Result<()> {
        let home = app.path().home_dir().map_err(io::Error::other)?;
        entry(&home, &app.config().identifier, program).apply(enabled)
    }
}

#[cfg(target_os = "windows")]
mod os {
    use std::io;
    use std::path::Path;

    use tauri::AppHandle;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
    };

    use super::FLAG;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Value name under the `Run` key; Task Manager shows the program's own name.
    const VALUE_NAME: &str = "Mumcord";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Windows paths cannot contain `"`, so quoting is enough.
    pub(super) fn command(program: &Path) -> String {
        format!("\"{}\" {FLAG}", program.display())
    }

    pub(super) fn apply(_app: &AppHandle, program: &Path, enabled: bool) -> io::Result<()> {
        let key = wide(RUN_KEY);
        let name = wide(VALUE_NAME);
        let status = if enabled {
            let data = wide(&command(program));
            let bytes = u32::try_from(data.len() * 2).map_err(io::Error::other)?;
            // SAFETY: all pointers come from live, NUL-terminated UTF-16 buffers and `bytes`
            // is the size of `data` including its terminator, as REG_SZ requires.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    bytes,
                )
            }
        } else {
            // SAFETY: both pointers come from live, NUL-terminated UTF-16 buffers.
            match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                other => other,
            }
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status.cast_signed()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const APP_ID: &str = "app.mumcord.client";

    fn settings(autostart: bool, start_minimized: bool) -> store::Settings {
        store::Settings {
            autostart,
            start_minimized,
            ..store::Settings::default()
        }
    }

    #[test]
    fn recognises_the_autostart_flag() {
        assert!(launched_at_login(["mumcord", FLAG]));
        assert!(!launched_at_login(["mumcord"]));
        assert!(!launched_at_login(["mumcord", "mumble://host"]));
    }

    #[test]
    fn starts_hidden_only_at_login_with_start_minimized() {
        assert!(start_hidden(true, &settings(true, true)));
        assert!(!start_hidden(true, &settings(true, false)));
        assert!(!start_hidden(false, &settings(true, true)), "manual start");
        assert!(
            !start_hidden(true, &settings(false, true)),
            "a stale entry after turning autostart off"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    struct TempDir(PathBuf);

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("mumcord-autostart-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn enabling_writes_and_repairs_the_entry_and_disabling_removes_it() {
        let dir = TempDir::new("toggle");
        let entry = os::entry(&dir.0, APP_ID, "/old/Mumcord".as_ref());
        entry.apply(true).expect("enable");
        assert_eq!(
            std::fs::read_to_string(&entry.path).expect("written"),
            entry.content
        );

        // The program moved (e.g. a new AppImage): the next start rewrites the entry.
        let moved = os::entry(&dir.0, APP_ID, "/new/Mumcord".as_ref());
        assert_eq!(moved.path, entry.path);
        moved.apply(true).expect("repair");
        let content = std::fs::read_to_string(&moved.path).expect("rewritten");
        assert!(content.contains("/new/Mumcord") && !content.contains("/old/Mumcord"));

        moved.apply(false).expect("disable");
        assert!(!moved.path.exists());
        moved.apply(false).expect("disabling twice is fine");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn never_touches_an_entry_the_user_wrote() {
        let dir = TempDir::new("foreign");
        let entry = os::entry(&dir.0, APP_ID, "/opt/Mumcord".as_ref());
        std::fs::create_dir_all(entry.path.parent().expect("parent")).expect("dir");
        std::fs::write(&entry.path, "my own entry").expect("write");
        entry.apply(true).expect("enable");
        entry.apply(false).expect("disable");
        assert_eq!(
            std::fs::read_to_string(&entry.path).expect("kept"),
            "my own entry"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_entry_follows_the_xdg_autostart_spec() {
        let entry = os::entry(
            "/home/u/.config".as_ref(),
            APP_ID,
            "/home/u/A b/Mumcord.AppImage".as_ref(),
        );
        assert_eq!(
            entry.path,
            PathBuf::from("/home/u/.config/autostart/app.mumcord.client.desktop")
        );
        let lines: Vec<&str> = entry.content.lines().collect();
        assert_eq!(lines[0], "[Desktop Entry]");
        for wanted in [
            "Type=Application",
            "Name=Mumcord",
            "Exec=\"/home/u/A b/Mumcord.AppImage\" --autostart",
            "Icon=app.mumcord.client",
            "X-Mumcord-Managed=true",
        ] {
            assert!(lines.contains(&wanted), "missing {wanted}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_launch_agent_runs_the_app_with_the_flag() {
        let entry = os::entry(
            "/Users/u".as_ref(),
            APP_ID,
            "/Applications/A&B.app/Contents/MacOS/mumcord".as_ref(),
        );
        assert_eq!(
            entry.path,
            PathBuf::from("/Users/u/Library/LaunchAgents/app.mumcord.client.plist")
        );
        assert!(
            entry
                .content
                .contains("<string>app.mumcord.client</string>")
        );
        assert!(
            entry
                .content
                .contains("<string>/Applications/A&amp;B.app/Contents/MacOS/mumcord</string>")
        );
        assert!(entry.content.contains("<string>--autostart</string>"));
        assert!(entry.content.contains("<key>RunAtLoad</key>"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_run_command_quotes_the_program() {
        assert_eq!(
            os::command(r"C:\Program Files\Mumcord\mumcord.exe".as_ref()),
            r#""C:\Program Files\Mumcord\mumcord.exe" --autostart"#
        );
    }
}
