//! Opens links (chat links, game join links) with the user's default application (issue #9).
//!
//! On Linux the desktop launchers (`xdg-open`, `kde-open`, `gio`) are host programs. Inside an
//! AppImage they inherit the bundle's environment: `LD_LIBRARY_PATH`, GIO/GTK module paths and
//! more point into the mounted image, so the host launchers load the bundled (older) libraries
//! and exit with symbol errors before a browser starts. Launchers therefore get the host
//! environment back: every path that points into an AppImage mount is dropped.

use std::io;

use tauri::{Manager, Runtime, WebviewWindowBuilder};

/// Only web and mail links open; anything else from a message is ignored.
pub fn is_openable(url: &str) -> bool {
    let url = url.trim_start();
    ["http:", "https:", "mailto:"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })
}

/// The URL a page's new-window request may open, if any. A middle click on a link, a
/// `target="_blank"` and `window.open` all ask for a window of their own; Mumcord never opens
/// one but hands web and mail links to the system browser like a normal click (issue #418).
pub fn new_window_target(url: &str) -> Option<&str> {
    let url = url.trim();
    is_openable(url).then_some(url)
}

/// Refuses every window a page wants to open and opens web and mail links in the system browser
/// instead. Used on every webview Mumcord shows (main window, extra windows, plugin frames), so
/// a page can never end up in a window of its own, with or without the app's commands (#418).
pub fn deny_new_windows<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    builder.on_new_window(|url, _features| {
        match new_window_target(url.as_str()) {
            Some(target) => {
                if let Err(error) = open_url(target) {
                    tracing::warn!(error = %error, "opening a link in the system browser failed");
                }
            }
            None => tracing::debug!(scheme = url.scheme(), "refused to open a new window"),
        }
        tauri::webview::NewWindowResponse::Deny
    })
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
pub fn open_url(url: &str) -> io::Result<()> {
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(io::Error::other)
}

/// Opens `url` and returns once the browser is handed the link, so the caller may exit.
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
pub fn open_url_before_exit(url: &str) -> io::Result<()> {
    open_url(url)
}

/// Opens `url` and returns once a launcher has started, so the caller may exit: the
/// launcher keeps running on its own, which [`open_url`]'s thread would not after an exit.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn open_url_before_exit(url: &str) -> io::Result<()> {
    let mut last = io::Error::from(io::ErrorKind::NotFound);
    for mut launcher in launchers(url) {
        match launcher.spawn() {
            Ok(_) => return Ok(()),
            Err(error) => last = error,
        }
    }
    Err(last)
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn open_url(url: &str) -> io::Result<()> {
    let launchers = launchers(url);
    // `xdg-open` may wait for the browser, so the launchers run on their own thread; a failing
    // one hands over to the next, like `open::that`. The link itself is never logged.
    std::thread::Builder::new()
        .name("open-url".into())
        .spawn(move || {
            for mut launcher in launchers {
                let program = launcher.get_program().to_string_lossy().into_owned();
                match launcher.status() {
                    Ok(status) if status.success() => return,
                    Ok(status) => tracing::warn!(%program, %status, "link launcher failed"),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => tracing::warn!(%program, %error, "link launcher did not start"),
                }
            }
            tracing::error!("no launcher could open the link");
        })?;
    Ok(())
}

/// The desktop's link launchers for `url`, quiet and with the host's environment.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn launchers(url: &str) -> Vec<std::process::Command> {
    use std::process::Stdio;

    let overrides = std::env::var_os("APPDIR")
        .map(|appdir| host_env(&appdir, std::env::vars_os()))
        .unwrap_or_default();
    let mut launchers = open::commands(url);
    for launcher in &mut launchers {
        launcher
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (key, value) in &overrides {
            match value {
                Some(value) => launcher.env(key, value),
                None => launcher.env_remove(key),
            };
        }
    }
    launchers
}

/// Environment changes that give host programs their own environment back inside an AppImage
/// mounted at `appdir`: `Some` is the variable's value without bundle paths, `None` removes a
/// variable that held only bundle paths. Paths of other AppImage mounts next to it (left in
/// `PATH` by an AppImage started from this one) are dropped too.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn host_env(
    appdir: &std::ffi::OsStr,
    vars: impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let appdir = appdir.as_bytes();
    let appdir = appdir.strip_suffix(b"/").unwrap_or(appdir);
    if appdir.is_empty() {
        return Vec::new();
    }
    let sibling_mounts = appdir
        .iter()
        .rposition(|&byte| byte == b'/')
        .map(|slash| [&appdir[..=slash], b".mount_"].concat());
    let in_bundle = |entry: &[u8]| {
        let in_dir = |dir: &[u8]| {
            entry.starts_with(dir) && matches!(entry.get(dir.len()), None | Some(b'/'))
        };
        in_dir(appdir)
            || sibling_mounts
                .as_deref()
                .is_some_and(|m| entry.starts_with(m))
    };
    vars.filter_map(|(key, value)| {
        let entries: Vec<&[u8]> = value.as_bytes().split(|&byte| byte == b':').collect();
        if !entries.iter().any(|entry| in_bundle(entry)) {
            return None;
        }
        let kept: Vec<&[u8]> = entries
            .into_iter()
            .filter(|entry| !entry.is_empty() && !in_bundle(entry))
            .collect();
        let value = (!kept.is_empty()).then(|| OsString::from_vec(kept.join(&b':')));
        Some((key, value))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_only_web_and_mail_links() {
        assert!(is_openable("https://mumcord.app"));
        assert!(is_openable("HTTP://example.test/a"));
        assert!(is_openable(" mailto:someone@example.test"));
        assert!(!is_openable("javascript:alert(1)"));
        assert!(!is_openable("file:///etc/passwd"));
        assert!(!is_openable("mumble://example.test"));
        assert!(!is_openable("http"));
        assert!(!is_openable(""));
    }

    #[test]
    fn new_windows_only_reach_the_browser_for_web_and_mail_links() {
        assert_eq!(
            new_window_target("https://mumcord.app/a"),
            Some("https://mumcord.app/a")
        );
        assert_eq!(
            new_window_target(" mailto:someone@example.test"),
            Some("mailto:someone@example.test")
        );
        // Nothing else ever becomes a window: no app page, no file, no script.
        assert_eq!(new_window_target("about:blank"), None);
        assert_eq!(new_window_target("file:///etc/passwd"), None);
        assert_eq!(new_window_target("javascript:alert(1)"), None);
        assert_eq!(new_window_target("tauri://localhost/index.html"), None);
        assert_eq!(new_window_target(""), None);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    mod host_env {
        use super::super::host_env;
        use std::ffi::{OsStr, OsString};

        fn run(vars: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
            let vars = vars
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v)));
            host_env(OsStr::new("/tmp/.mount_MumcorAb12/"), vars)
                .into_iter()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.map(|v| v.to_string_lossy().into_owned()),
                    )
                })
                .collect()
        }

        fn some(key: &str, value: &str) -> (String, Option<String>) {
            (key.to_owned(), Some(value.to_owned()))
        }

        fn removed(key: &str) -> (String, Option<String>) {
            (key.to_owned(), None)
        }

        #[test]
        fn removes_bundle_library_and_module_paths() {
            let changes = run(&[
                (
                    "LD_LIBRARY_PATH",
                    "/tmp/.mount_MumcorAb12/usr/lib/:/tmp/.mount_MumcorAb12/lib/",
                ),
                (
                    "GIO_MODULE_DIR",
                    "/tmp/.mount_MumcorAb12//usr/lib/gio/modules",
                ),
                ("PYTHONPATH", "/tmp/.mount_MumcorAb12/usr/share/pyshared/:"),
                ("APPDIR", "/tmp/.mount_MumcorAb12"),
            ]);
            assert_eq!(
                changes,
                [
                    removed("LD_LIBRARY_PATH"),
                    removed("GIO_MODULE_DIR"),
                    removed("PYTHONPATH"),
                    removed("APPDIR"),
                ]
            );
        }

        #[test]
        fn keeps_host_entries_of_mixed_lists() {
            let changes = run(&[
                (
                    "PATH",
                    "/tmp/.mount_MumcorAb12/usr/bin/:/tmp/.mount_MumcorOld9/usr/bin/:/usr/local/bin:/usr/bin",
                ),
                (
                    "XDG_DATA_DIRS",
                    "/tmp/.mount_MumcorAb12/usr/share:/usr/share:/usr/local/share",
                ),
            ]);
            assert_eq!(
                changes,
                [
                    some("PATH", "/usr/local/bin:/usr/bin"),
                    some("XDG_DATA_DIRS", "/usr/share:/usr/local/share"),
                ]
            );
        }

        #[test]
        fn leaves_unrelated_variables_alone() {
            let changes = run(&[
                ("HOME", "/home/someone"),
                ("GTK_THEME", "Adwaita:dark"),
                ("OTHER", "/tmp/other/lib:/opt/tools"),
                ("DISPLAY", ":0"),
            ]);
            assert_eq!(changes, []);
        }

        #[test]
        fn does_nothing_without_a_bundle_directory() {
            let vars = [(OsString::from("PATH"), OsString::from("/usr/bin"))].into_iter();
            assert!(host_env(OsStr::new(""), vars).is_empty());
        }
    }
}
