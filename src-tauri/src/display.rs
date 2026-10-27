//! Window-system choice made before GTK starts: push-to-talk compatibility mode (FR-1.48, QA.5).
//!
//! On a Linux Wayland desktop the stored `ptt_compatibility_mode` setting makes Mumcord run
//! through XWayland, so the X11 push-to-talk path works without the portal or
//! the `input` group. GTK reads `GDK_BACKEND` once at start-up, so the choice is made here
//! from the settings database and only changes with a restart.

#[cfg(target_os = "linux")]
use crate::ptt::SessionEnv;

/// Whether `GDK_BACKEND=x11` should be set for this start.
///
/// Only on a Wayland desktop with the setting on, and never over a `GDK_BACKEND` the user
/// set themselves.
#[cfg(target_os = "linux")]
fn wants_xwayland(env: &SessionEnv, compatibility_mode: bool) -> bool {
    compatibility_mode && env.gdk_backend.is_none() && env.wayland_desktop()
}

/// Selects XWayland for this process if compatibility mode is on. Call first in `run`, before
/// any other thread exists, with the app identifier (the data folder name).
#[cfg(target_os = "linux")]
pub fn prepare(identifier: &str) {
    let env = SessionEnv::current();
    // Checked first so other sessions never touch the database this early.
    if wants_xwayland(&env, true) && stored_compatibility_mode(identifier) {
        // SAFETY: called at the very start of `run`, before Tauri, GTK or the async runtime
        // start any thread, so nothing reads the environment concurrently.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
}

#[cfg(not(target_os = "linux"))]
pub fn prepare(_identifier: &str) {}

/// Reads the setting from the database Tauri's `app_data_dir` points at (XDG data dir plus
/// identifier). Missing or unreadable data means the modern path.
#[cfg(target_os = "linux")]
fn stored_compatibility_mode(identifier: &str) -> bool {
    data_dir(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .is_some_and(|base| read_from(&base, identifier))
}

#[cfg(target_os = "linux")]
fn read_from(base: &std::path::Path, identifier: &str) -> bool {
    use mumcord_engine::store::{DB_FILE_NAME, Store};

    let data_dir = base.join(identifier);
    // First start: nothing stored yet, and the database is not created before the engine.
    if !data_dir.join(DB_FILE_NAME).is_file() {
        return false;
    }
    match Store::open(&data_dir).and_then(|store| store.settings()) {
        Ok(settings) => settings.ptt_compatibility_mode,
        Err(e) => {
            // Logging starts later; stderr is the only channel this early.
            eprintln!("reading the compatibility mode setting failed: {e}");
            false
        }
    }
}

/// XDG base data directory: `$XDG_DATA_HOME` if absolute, else `$HOME/.local/share`.
#[cfg(target_os = "linux")]
fn data_dir(
    xdg_data_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};

    match xdg_data_home.map(Path::new) {
        Some(dir) if dir.is_absolute() => Some(dir.to_path_buf()),
        _ => home
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".local/share")),
    }
}

// Everything here is Linux-only; on other systems `prepare` does nothing.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn env(session: Option<&str>, display: Option<&str>, gdk: Option<&str>) -> SessionEnv {
        SessionEnv {
            session_type: session.map(str::to_owned),
            wayland_display: display.map(str::to_owned),
            gdk_backend: gdk.map(str::to_owned),
        }
    }

    #[test]
    fn xwayland_only_on_wayland_with_the_setting_on() {
        let wayland = env(Some("wayland"), Some("wayland-0"), None);
        assert!(wants_xwayland(&wayland, true));
        assert!(
            !wants_xwayland(&wayland, false),
            "default stays the modern path"
        );
        assert!(!wants_xwayland(&env(Some("x11"), None, None), true));
        assert!(!wants_xwayland(&env(None, None, None), true));
    }

    #[test]
    fn a_user_set_gdk_backend_is_kept() {
        let forced = env(Some("wayland"), Some("wayland-0"), Some("wayland"));
        assert!(!wants_xwayland(&forced, true));
    }

    #[test]
    fn switching_to_xwayland_selects_the_x11_push_to_talk_path() {
        let mut session = env(Some("wayland"), Some("wayland-0"), None);
        assert!(session.runs_on_wayland());
        assert!(wants_xwayland(&session, true));
        session.gdk_backend = Some("x11".into());
        assert!(!session.runs_on_wayland());
        assert!(session.compatibility_active());
    }

    #[test]
    fn data_dir_follows_xdg() {
        use std::ffi::OsStr;
        use std::path::PathBuf;

        let home = Some(OsStr::new("/home/u"));
        assert_eq!(
            data_dir(Some(OsStr::new("/x/data")), home),
            Some(PathBuf::from("/x/data"))
        );
        // Relative values are invalid per the XDG spec and ignored.
        assert_eq!(
            data_dir(Some(OsStr::new("rel")), home),
            Some(PathBuf::from("/home/u/.local/share"))
        );
        assert_eq!(
            data_dir(None, home),
            Some(PathBuf::from("/home/u/.local/share"))
        );
        assert_eq!(data_dir(None, None), None);
    }

    #[test]
    fn reads_the_stored_setting() {
        use mumcord_engine::store::Store;

        let root = std::env::temp_dir().join(format!("mumcord-display-{}", std::process::id()));
        let dir = root.join("app.test");
        std::fs::create_dir_all(&dir).expect("create dir");
        let store = Store::open(&dir).expect("open");
        let mut settings = store.settings().expect("settings");
        settings.ptt_compatibility_mode = true;
        store.save_settings(&settings).expect("save");
        drop(store);
        assert!(read_from(&root, "app.test"));
        assert!(!read_from(&root, "missing"));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
