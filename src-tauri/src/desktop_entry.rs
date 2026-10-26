//! Linux desktop entry and icons.
//!
//! On Wayland the compositor finds a window's icon and name through the `.desktop` file whose
//! name matches the window's app ID (`app.mumcord.client`, set as the program name in `run`); window icons
//! set by the app are ignored. The desktop portal maps global shortcuts to an app the same way.
//! Development builds are not installed by a package manager, so Mumcord writes that entry and
//! its icons into the user's data folder at start-up. An AppImage does so only after the user
//! agreed (`appimage_menu_entry`, FR-2.55); the UI asks once. The same entry is the `mumble://`
//! handler, so there is only one Mumcord entry.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use mumcord_engine::{Engine, store::Consent};
use tauri::{AppHandle, Manager};

/// Shown in menus and the taskbar.
const NAME: &str = "Mumcord";
const MIME_TYPE: &str = "x-scheme-handler/mumble";
/// Marks entries Mumcord wrote itself, so it never overwrites or deletes one the user edited.
pub(crate) const MANAGED_KEY: &str = "X-Mumcord-Managed=true";
/// Entry the deep-link plugin used to write; replaced by the app-ID entry.
const LEGACY_HANDLER: &str = "mumcord-handler.desktop";

/// Icons installed into the user's hicolor theme, by size folder.
const ICONS: [(&str, &[u8]); 4] = [
    ("32x32", include_bytes!("../icons/32x32.png")),
    ("128x128", include_bytes!("../icons/128x128.png")),
    ("256x256", include_bytes!("../icons/128x128@2x.png")),
    ("512x512", include_bytes!("../icons/icon.png")),
];

/// Whether this process runs from an AppImage (the runtime sets `APPIMAGE` to its path).
pub(crate) fn is_appimage() -> bool {
    std::env::var_os("APPIMAGE").is_some_and(|path| !path.is_empty())
}

/// Whether the user's entry may be written and kept up to date. Package installs and
/// development builds always may; an AppImage only after a "yes", because it is not installed
/// and the user never asked for a menu entry.
fn may_write_entry(appimage: bool, consent: Consent) -> bool {
    !appimage || consent == Consent::Yes
}

/// Whether an entry Mumcord manages already exists although the user was never asked: the curl
/// installer writes it when it installs the AppImage (FR-2.54), which counts as a yes.
fn installer_consented(consent: Consent, user_entry: Option<&str>) -> bool {
    consent == Consent::Unset && user_entry.is_some_and(is_managed)
}

/// Stores the yes when the installer already wrote the entry, so the UI does not offer the
/// menu entry again. Blocks briefly: it runs during setup, before the UI reads the settings.
fn adopt_installer_entry(app: &AppHandle, install: &Install, consent: Consent) -> Consent {
    let entry = match read_optional(&install.entry_path()) {
        Ok(entry) => entry,
        Err(e) => {
            tracing::warn!(error = %e, "reading the desktop entry failed");
            return consent;
        }
    };
    if !installer_consented(consent, entry.as_deref()) {
        return consent;
    }
    let engine = app.state::<Engine>();
    let stored = engine.settings().and_then(|mut settings| {
        settings.appimage_menu_entry = Consent::Yes;
        tauri::async_runtime::block_on(engine.update_settings(settings))
    });
    match stored {
        Ok(_) => {
            tracing::info!("the installer added the menu entry; keeping it up to date");
            Consent::Yes
        }
        Err(e) => {
            tracing::warn!(error = %e, "storing the installer's menu entry choice failed");
            consent
        }
    }
}

fn appimage_consent(app: &AppHandle) -> Consent {
    match app.state::<Engine>().settings() {
        Ok(settings) => settings.appimage_menu_entry,
        Err(e) => {
            tracing::warn!(error = %e, "reading the menu entry choice failed; not writing the entry");
            Consent::Unset
        }
    }
}

/// File name of the entry for an app ID.
fn file_name(app_id: &str) -> String {
    format!("{app_id}.desktop")
}

/// Quotes the program path for `Exec` (desktop entry spec: reserved characters inside double
/// quotes get a backslash, and the value itself escapes every backslash once more).
pub(crate) fn exec_program(path: &str) -> String {
    let mut quoted = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted.replace('\\', "\\\\")
}

/// Content of the desktop entry for the given app ID and program path.
fn desktop_file(app_id: &str, program: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={NAME}\n\
         Comment=Voice chat compatible with Mumble\n\
         Exec={} %u\n\
         Icon={app_id}\n\
         Terminal=false\n\
         Categories=Network;Chat;AudioVideo;\n\
         MimeType={MIME_TYPE};\n\
         StartupWMClass={app_id}\n\
         {MANAGED_KEY}\n",
        exec_program(program)
    )
}

fn is_managed(content: &str) -> bool {
    content.lines().any(|line| line.trim() == MANAGED_KEY)
}

/// Whether a legacy handler entry was written for Mumcord (and not by someone else).
fn is_legacy_handler(content: &str) -> bool {
    let has = |wanted: &str| content.lines().any(|line| line.trim() == wanted);
    has(&format!("Name={NAME}")) && has(&format!("MimeType={MIME_TYPE}"))
}

/// What to do with the user's entry.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// A system package provides the entry; nothing to write.
    UseSystem,
    /// A system package provides the entry; remove the one an earlier dev build or AppImage
    /// wrote, because it would shadow the packaged one.
    RemoveUser,
    UpToDate,
    /// The user's entry was not written by Mumcord; leave it alone.
    KeepForeign,
    Write,
}

fn plan(system_entry: bool, user_entry: Option<&str>, wanted: &str) -> Plan {
    match (system_entry, user_entry) {
        (true, Some(current)) if is_managed(current) => Plan::RemoveUser,
        (true, _) => Plan::UseSystem,
        (false, Some(current)) if current == wanted => Plan::UpToDate,
        (false, Some(current)) if !is_managed(current) => Plan::KeepForeign,
        (false, _) => Plan::Write,
    }
}

/// Folders where packages install entries: `$XDG_DATA_DIRS` or the spec's default.
fn system_data_dirs(xdg_data_dirs: Option<OsString>) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = xdg_data_dirs
        .map(|dirs| std::env::split_paths(&dirs).collect())
        .unwrap_or_default();
    let dirs: Vec<PathBuf> = dirs.into_iter().filter(|d| d.is_absolute()).collect();
    if dirs.is_empty() {
        vec!["/usr/local/share".into(), "/usr/share".into()]
    } else {
        dirs
    }
}

/// Paths and names for one Mumcord installation.
struct Install {
    app_id: String,
    /// The program to start (`autostart::program`).
    program: PathBuf,
    /// `$XDG_DATA_HOME`.
    data_home: PathBuf,
    system_dirs: Vec<PathBuf>,
}

impl Install {
    fn from_app(app: &AppHandle) -> io::Result<Self> {
        let program = crate::autostart::program(app)?;
        let data_home = app.path().data_dir().map_err(io::Error::other)?;
        Ok(Self {
            app_id: app.config().identifier.clone(),
            program,
            data_home,
            system_dirs: system_data_dirs(std::env::var_os("XDG_DATA_DIRS")),
        })
    }

    fn applications(&self) -> PathBuf {
        self.data_home.join("applications")
    }

    fn entry_path(&self) -> PathBuf {
        self.applications().join(file_name(&self.app_id))
    }

    fn system_entry_exists(&self) -> bool {
        let name = file_name(&self.app_id);
        self.system_dirs
            .iter()
            .any(|dir| dir.join("applications").join(&name).is_file())
    }

    /// Writes, updates or removes the user's entry and icons as needed.
    fn ensure_entry(&self) -> io::Result<()> {
        let path = self.entry_path();
        let current = read_optional(&path)?;
        let wanted = desktop_file(&self.app_id, &self.program.to_string_lossy());
        match plan(self.system_entry_exists(), current.as_deref(), &wanted) {
            Plan::UseSystem | Plan::UpToDate => {}
            Plan::KeepForeign => {
                tracing::info!(path = %path.display(), "keeping a desktop entry not written by Mumcord");
            }
            Plan::RemoveUser => {
                std::fs::remove_file(&path)?;
                tracing::info!("removed the user desktop entry; a system package provides one");
                update_desktop_database(&self.applications());
            }
            Plan::Write => {
                std::fs::create_dir_all(self.applications())?;
                std::fs::write(&path, wanted)?;
                self.install_icons()?;
                tracing::info!(path = %path.display(), "desktop entry written");
                update_desktop_database(&self.applications());
            }
        }
        Ok(())
    }

    fn install_icons(&self) -> io::Result<()> {
        let theme = self.data_home.join("icons/hicolor");
        for (size, bytes) in ICONS {
            let dir = theme.join(size).join("apps");
            let path = dir.join(format!("{}.png", self.app_id));
            if std::fs::read(&path).ok().as_deref() == Some(bytes) {
                continue;
            }
            std::fs::create_dir_all(&dir)?;
            std::fs::write(&path, bytes)?;
        }
        Ok(())
    }

    /// Removes the old deep-link handler entry and moves the `mumble://` default over to the
    /// app-ID entry if the old one was the default.
    fn migrate_legacy_handler(&self) -> io::Result<()> {
        let path = self.applications().join(LEGACY_HANDLER);
        let Some(content) = read_optional(&path)? else {
            return Ok(());
        };
        if !is_legacy_handler(&content) {
            return Ok(());
        }
        let was_default = default_handler()?.as_deref() == Some(LEGACY_HANDLER);
        std::fs::remove_file(&path)?;
        tracing::info!("removed the old mumble:// handler entry");
        update_desktop_database(&self.applications());
        if was_default {
            set_default_handler(&file_name(&self.app_id))?;
        }
        Ok(())
    }
}

fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Refreshes the MIME cache so `mumble://` finds the entry. Optional: desktops also rescan.
fn update_desktop_database(applications: &Path) {
    match Command::new("update-desktop-database")
        .arg(applications)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::warn!(%status, "update-desktop-database failed"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            tracing::info!("update-desktop-database not installed; skipping the MIME cache update");
        }
        Err(e) => tracing::warn!(error = %e, "running update-desktop-database failed"),
    }
}

/// The entry `xdg-mime` reports as the `mumble://` handler, if any.
fn default_handler() -> io::Result<Option<String>> {
    let output = Command::new("xdg-mime")
        .args(["query", "default", MIME_TYPE])
        .output()?;
    // xdg-utils exits with 4 ("action failed") when no default handler exists.
    if output.status.code() == Some(4) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "xdg-mime query failed: {}",
            output.status
        )));
    }
    // Backends may pad the name or print more lines, so only the first line counts.
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty()))
}

fn set_default_handler(entry: &str) -> io::Result<()> {
    let status = Command::new("xdg-mime")
        .args(["default", entry, MIME_TYPE])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "xdg-mime default failed: {status}"
        )))
    }
}

/// Makes sure the desktop entry and icons exist; runs off the main thread because it starts
/// external commands.
pub fn ensure(app: &AppHandle) {
    let install = match Install::from_app(app) {
        Ok(install) => install,
        Err(e) => {
            tracing::warn!(error = %e, "desktop entry setup skipped");
            return;
        }
    };
    let appimage = is_appimage();
    let mut consent = appimage_consent(app);
    if appimage {
        consent = adopt_installer_entry(app, &install, consent);
    }
    if !may_write_entry(appimage, consent) {
        tracing::info!("not writing the desktop entry: the AppImage menu entry was not accepted");
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("desktop-entry".into())
        .spawn(move || {
            if let Err(e) = install.ensure_entry() {
                tracing::warn!(error = %e, "writing the desktop entry failed");
            }
            if let Err(e) = install.migrate_legacy_handler() {
                tracing::warn!(error = %e, "migrating the old mumble:// handler entry failed");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "starting the desktop entry setup failed");
    }
}

/// Whether Mumcord's entry is the default `mumble://` handler.
pub fn is_handler(app: &AppHandle) -> io::Result<bool> {
    Ok(default_handler()?.as_deref() == Some(file_name(&app.config().identifier).as_str()))
}

/// AppImage only, after the user agreed: writes the entry and icons and moves an old
/// `mumble://` default over. Safe to repeat.
pub fn add_for_appimage(app: &AppHandle) -> io::Result<()> {
    if !is_appimage() {
        return Err(io::Error::other("not running as an AppImage"));
    }
    let install = Install::from_app(app)?;
    install.ensure_entry()?;
    install.migrate_legacy_handler()
}

/// Makes Mumcord's entry the default `mumble://` handler. An AppImage needs the user's
/// menu-entry consent first, since the handler is that same entry.
pub fn register_handler(app: &AppHandle) -> io::Result<()> {
    if !may_write_entry(is_appimage(), appimage_consent(app)) {
        return Err(io::Error::other(
            "allow Mumcord to add itself to the app menu first",
        ));
    }
    let install = Install::from_app(app)?;
    install.ensure_entry()?;
    set_default_handler(&file_name(&install.app_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_ID: &str = "app.mumcord.client";

    #[test]
    fn desktop_file_matches_the_app_id_and_handles_mumble_links() {
        let content = desktop_file(APP_ID, "/opt/mumcord/mumcord");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "[Desktop Entry]");
        for wanted in [
            "Type=Application",
            "Name=Mumcord",
            "Exec=\"/opt/mumcord/mumcord\" %u",
            "Icon=app.mumcord.client",
            "MimeType=x-scheme-handler/mumble;",
            "StartupWMClass=app.mumcord.client",
            "Categories=Network;Chat;AudioVideo;",
            "Terminal=false",
            MANAGED_KEY,
        ] {
            assert!(lines.contains(&wanted), "missing {wanted}");
        }
        assert!(!content.contains("NoDisplay"));
        assert_eq!(file_name(APP_ID), "app.mumcord.client.desktop");
    }

    #[test]
    fn an_appimage_writes_only_after_a_yes() {
        assert!(!may_write_entry(true, Consent::Unset));
        assert!(!may_write_entry(true, Consent::No));
        assert!(may_write_entry(true, Consent::Yes));
    }

    #[test]
    fn an_entry_written_by_the_installer_counts_as_a_yes() {
        let managed = desktop_file(APP_ID, "/home/a/.local/bin/Mumcord.AppImage");
        assert!(installer_consented(Consent::Unset, Some(&managed)));
        // Never overrides an answer, and never adopts an entry Mumcord did not write.
        assert!(!installer_consented(Consent::No, Some(&managed)));
        assert!(!installer_consented(Consent::Yes, Some(&managed)));
        assert!(!installer_consented(
            Consent::Unset,
            Some("[Desktop Entry]\nName=Mumcord\n")
        ));
        assert!(!installer_consented(Consent::Unset, None));
    }

    #[test]
    fn packages_and_dev_builds_do_not_ask() {
        for consent in [Consent::Unset, Consent::No, Consent::Yes] {
            assert!(may_write_entry(false, consent));
        }
    }

    #[test]
    fn exec_escapes_reserved_characters() {
        assert_eq!(exec_program("/home/a b/mumcord"), "\"/home/a b/mumcord\"");
        // `"` becomes `\"` inside the quotes, then the value escapes the backslash again.
        assert_eq!(exec_program("/x\"y$z"), "\"/x\\\\\"y\\\\$z\"");
        assert_eq!(exec_program("/a\\b"), "\"/a\\\\\\\\b\"");
    }

    #[test]
    fn writes_when_missing_or_outdated() {
        let wanted = desktop_file(APP_ID, "/new/mumcord");
        let old = desktop_file(APP_ID, "/old/mumcord");
        assert_eq!(plan(false, None, &wanted), Plan::Write);
        assert_eq!(plan(false, Some(&old), &wanted), Plan::Write);
        assert_eq!(plan(false, Some(&wanted), &wanted), Plan::UpToDate);
    }

    #[test]
    fn never_overwrites_an_entry_the_user_wrote() {
        let wanted = desktop_file(APP_ID, "/new/mumcord");
        let foreign = "[Desktop Entry]\nName=My Mumcord\nExec=/x\n";
        assert_eq!(plan(false, Some(foreign), &wanted), Plan::KeepForeign);
        assert_eq!(plan(true, Some(foreign), &wanted), Plan::UseSystem);
    }

    #[test]
    fn a_system_package_entry_wins() {
        let wanted = desktop_file(APP_ID, "/usr/bin/mumcord");
        let ours = desktop_file(APP_ID, "/home/u/dev/mumcord");
        assert_eq!(plan(true, None, &wanted), Plan::UseSystem);
        assert_eq!(plan(true, Some(&ours), &wanted), Plan::RemoveUser);
    }

    #[test]
    fn recognises_only_mumcords_legacy_handler() {
        let legacy = "[Desktop Entry]\nType=Application\nName=Mumcord\nExec=\"/x/mumcord\" %u\n\
                      Terminal=false\nMimeType=x-scheme-handler/mumble\nNoDisplay=true\n";
        assert!(is_legacy_handler(legacy));
        assert!(!is_legacy_handler(
            "[Desktop Entry]\nName=Other\nMimeType=x-scheme-handler/mumble\n"
        ));
    }

    #[test]
    fn system_dirs_default_per_spec() {
        assert_eq!(
            system_data_dirs(None),
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share")
            ]
        );
        assert_eq!(
            system_data_dirs(Some("/a:rel:/b".into())),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn writes_entry_and_icons_into_the_data_home() {
        let dir =
            std::env::temp_dir().join(format!("mumcord-desktop-entry-{}", std::process::id()));
        let install = Install {
            app_id: APP_ID.into(),
            program: "/opt/mumcord".into(),
            data_home: dir.clone(),
            system_dirs: vec![dir.join("system")],
        };
        install.ensure_entry().expect("entry written");
        let entry = std::fs::read_to_string(install.entry_path()).expect("entry exists");
        assert_eq!(entry, desktop_file(APP_ID, "/opt/mumcord"));
        for (size, bytes) in ICONS {
            let icon = dir.join(format!("icons/hicolor/{size}/apps/{APP_ID}.png"));
            assert_eq!(std::fs::read(icon).expect("icon exists"), bytes);
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
