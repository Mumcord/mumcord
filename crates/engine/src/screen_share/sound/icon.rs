//! The icon of an app in "Which app's sound?" (FR-7.83, Linux).
//!
//! PipeWire streams name their app's icon with `application.icon-name`, a freedesktop icon
//! name. This looks it up the way the icon theme specification falls back for every theme: in
//! the `hicolor` theme of each data directory, then in `pixmaps`. The user's own theme is not
//! read; apps install their icons into `hicolor`, so that is where they are found. Without an
//! icon the list shows a coloured square instead.

use std::path::{Path, PathBuf};

use super::AppIcon;

/// Icons larger than this are skipped: the list shows them at 24 px.
const MAX_BYTES: u64 = 512 * 1024;

/// Sizes in the order they are tried: scalable first, then the ones closest to the list's 24 px
/// at 2× scale, then the rest.
const SIZES: [&str; 9] = [
    "scalable", "48x48", "64x64", "32x32", "128x128", "256x256", "24x24", "22x22", "16x16",
];

/// The icon named `name` from this system's data directories (`$XDG_DATA_HOME`,
/// `$XDG_DATA_DIRS`), `None` where there is none.
pub fn resolve(name: &str) -> Option<AppIcon> {
    find(name, &data_dirs())
}

/// The icon named `name` in `data_dirs`, which hold `icons/` and `pixmaps/`.
pub fn find(name: &str, data_dirs: &[PathBuf]) -> Option<AppIcon> {
    let name = name.trim();
    // A name is a file stem, never a path into somewhere else.
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    let themed = data_dirs.iter().flat_map(|dir| {
        SIZES
            .iter()
            .map(move |size| dir.join("icons/hicolor").join(size).join("apps"))
    });
    let pixmaps = data_dirs.iter().map(|dir| dir.join("pixmaps"));
    themed
        .chain(pixmaps)
        .find_map(|dir| ["svg", "png"].iter().find_map(|ext| read(&dir, name, ext)))
}

fn read(dir: &Path, name: &str, ext: &str) -> Option<AppIcon> {
    let path = dir.join(format!("{name}.{ext}"));
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    match std::fs::read(&path) {
        Ok(bytes) => Some(AppIcon {
            mime: if ext == "svg" {
                "image/svg+xml"
            } else {
                "image/png"
            },
            bytes,
        }),
        Err(e) => {
            // The path holds the app's icon name, which stays out of logs (FR-7.87).
            tracing::debug!(error = %e.kind(), "app icon not read");
            None
        }
    }
}

/// `$XDG_DATA_HOME` (default `~/.local/share`) before `$XDG_DATA_DIRS` (default
/// `/usr/local/share:/usr/share`), as the base directory specification orders them.
fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".local/share")));
    let system = std::env::var_os("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(std::env::split_paths(&system))
        .filter(|dir| dir.is_absolute())
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn prefers_the_scalable_hicolor_icon() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "icons/hicolor/48x48/apps/firefox.png", b"png");
        put(
            dir.path(),
            "icons/hicolor/scalable/apps/firefox.svg",
            b"<svg/>",
        );
        let icon = find("firefox", &[dir.path().to_owned()]).unwrap();
        assert_eq!(icon.mime, "image/svg+xml");
        assert_eq!(icon.bytes, b"<svg/>");
    }

    #[test]
    fn the_user_directory_comes_first_and_pixmaps_last() {
        let user = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        put(system.path(), "icons/hicolor/48x48/apps/vlc.png", b"system");
        put(user.path(), "icons/hicolor/48x48/apps/vlc.png", b"user");
        put(system.path(), "pixmaps/mpv.png", b"pixmap");
        let dirs = [user.path().to_owned(), system.path().to_owned()];
        assert_eq!(find("vlc", &dirs).unwrap().bytes, b"user");
        let pixmap = find("mpv", &dirs).unwrap();
        assert_eq!(
            (pixmap.mime, pixmap.bytes.as_slice()),
            ("image/png", &b"pixmap"[..])
        );
    }

    #[test]
    fn names_that_are_paths_or_missing_find_nothing() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "pixmaps/secret.png", b"x");
        let dirs = [dir.path().join("icons")];
        assert_eq!(find("../pixmaps/secret", &dirs), None);
        assert_eq!(find("", &dirs), None);
        assert_eq!(find("unknown", &[dir.path().to_owned()]), None);
    }

    #[test]
    fn oversized_icons_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![0_u8; usize::try_from(MAX_BYTES).unwrap() + 1];
        put(dir.path(), "icons/hicolor/48x48/apps/huge.png", &big);
        assert_eq!(find("huge", &[dir.path().to_owned()]), None);
    }
}
