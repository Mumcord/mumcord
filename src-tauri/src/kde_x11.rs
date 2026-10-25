//! KDE Plasma (Wayland): lets X11 apps in the background see mouse buttons (FR-1.48).
//!
//! Mumcord's compatibility mode reads input through XWayland, and KWin only passes mouse
//! buttons to X11 apps when `kwinrc` `[Xwayland]` allows it. This module reads and changes that
//! setting through `kreadconfig6`/`kwriteconfig6` (fixed argv, no shell) and remembers the
//! previous values so the change can be undone. All config access goes through [`Kconfig`] so
//! tests never touch the real `kwinrc`.

use std::io;
use std::path::Path;
use std::process::Command;

const FILE: &str = "kwinrc";
const GROUP: &str = "Xwayland";
const EAVESDROPS: &str = "XwaylandEavesdrops";
const EAVESDROPS_MOUSE: &str = "XwaylandEavesdropsMouse";

/// KWin's default when `XwaylandEavesdrops` is not set.
const DEFAULT_EAVESDROPS: &str = "Combinations";
/// The lowest `XwaylandEavesdrops` level at which the mouse switch has an effect.
const COMBINATIONS: &str = "Combinations";

/// Reads and writes keys of `kwinrc` `[Xwayland]`.
pub trait Kconfig {
    /// `None` if the key is not set.
    fn read(&self, key: &str) -> io::Result<Option<String>>;
    /// `None` removes the key, so KWin falls back to its default.
    fn write(&self, key: &str, value: Option<&str>) -> io::Result<()>;
}

/// The real `kreadconfig6` / `kwriteconfig6`.
pub struct Kde;

fn read_argv(key: &str) -> Vec<String> {
    ["--file", FILE, "--group", GROUP, "--key", key]
        .map(str::to_owned)
        .to_vec()
}

fn write_argv(key: &str, value: Option<&str>) -> Vec<String> {
    let mut argv = read_argv(key);
    match value {
        Some(v) => argv.push(v.to_owned()),
        None => argv.push("--delete".to_owned()),
    }
    argv
}

impl Kconfig for Kde {
    fn read(&self, key: &str) -> io::Result<Option<String>> {
        let out = Command::new("kreadconfig6").args(read_argv(key)).output()?;
        if !out.status.success() {
            return Err(io::Error::other("kreadconfig6 failed"));
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        Ok((!text.is_empty()).then_some(text))
    }

    fn write(&self, key: &str, value: Option<&str>) -> io::Result<()> {
        let status = Command::new("kwriteconfig6")
            .args(write_argv(key, value))
            .status()?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| io::Error::other("kwriteconfig6 failed"))
    }
}

/// Whether X11 apps may see mouse buttons now, and whether Mumcord can undo its change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    pub allowed: bool,
    pub undo_available: bool,
}

fn is_allowed(config: &dyn Kconfig) -> io::Result<bool> {
    let eavesdrops = config.read(EAVESDROPS)?;
    let mouse = config.read(EAVESDROPS_MOUSE)?;
    let level = eavesdrops.as_deref().unwrap_or(DEFAULT_EAVESDROPS);
    Ok(level == "All" || mouse.as_deref() == Some("true") && level == COMBINATIONS)
}

/// Current state; `undo_file` holds the values from before Mumcord's change.
pub fn access(config: &dyn Kconfig, undo_file: &Path) -> io::Result<Access> {
    Ok(Access {
        allowed: is_allowed(config)?,
        undo_available: undo_file.exists(),
    })
}

/// Undo file: one line per key, `key=value`, an empty value meaning "was not set".
fn encode(mouse: Option<&str>, eavesdrops: Option<&str>) -> String {
    format!(
        "{EAVESDROPS_MOUSE}={}\n{EAVESDROPS}={}\n",
        mouse.unwrap_or(""),
        eavesdrops.unwrap_or("")
    )
}

fn decode(text: &str) -> (Option<String>, Option<String>) {
    let value = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    (value(EAVESDROPS_MOUSE), value(EAVESDROPS))
}

/// Allows mouse buttons for X11 apps: mouse switch on, and the key level raised to
/// "Combinations" if it was lower (the mouse switch has no effect below that). The previous
/// values are saved first, but never overwritten by a second call.
pub fn allow_mouse(config: &dyn Kconfig, undo_file: &Path) -> io::Result<Access> {
    let mouse = config.read(EAVESDROPS_MOUSE)?;
    let eavesdrops = config.read(EAVESDROPS)?;
    if !undo_file.exists() {
        std::fs::write(undo_file, encode(mouse.as_deref(), eavesdrops.as_deref()))?;
    }
    config.write(EAVESDROPS_MOUSE, Some("true"))?;
    if matches!(eavesdrops.as_deref(), Some("None" | "Modifiers")) {
        config.write(EAVESDROPS, Some(COMBINATIONS))?;
    }
    access(config, undo_file)
}

/// Restores the values saved by [`allow_mouse`]. `NotFound` if there is nothing to undo.
pub fn undo(config: &dyn Kconfig, undo_file: &Path) -> io::Result<Access> {
    let (mouse, eavesdrops) = decode(&std::fs::read_to_string(undo_file)?);
    config.write(EAVESDROPS_MOUSE, mouse.as_deref())?;
    config.write(EAVESDROPS, eavesdrops.as_deref())?;
    std::fs::remove_file(undo_file)?;
    access(config, undo_file)
}

/// Asks KWin to reload its settings.
pub async fn reconfigure() -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    connection
        .call_method(
            Some("org.kde.KWin"),
            "/KWin",
            Some("org.kde.KWin"),
            "reconfigure",
            &(),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake(RefCell<HashMap<String, String>>);

    impl Fake {
        fn with(pairs: &[(&str, &str)]) -> Self {
            Self(RefCell::new(
                pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            ))
        }
        fn get(&self, key: &str) -> Option<String> {
            self.0.borrow().get(key).cloned()
        }
    }

    impl Kconfig for Fake {
        fn read(&self, key: &str) -> io::Result<Option<String>> {
            Ok(self.get(key))
        }
        fn write(&self, key: &str, value: Option<&str>) -> io::Result<()> {
            let mut map = self.0.borrow_mut();
            match value {
                Some(v) => map.insert(key.to_owned(), v.to_owned()),
                None => map.remove(key),
            };
            Ok(())
        }
    }

    fn undo_file() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("ok");
        let path = dir.path().join("undo");
        (dir, path)
    }

    #[test]
    fn argv_is_fixed_and_never_goes_through_a_shell() {
        assert_eq!(
            write_argv(EAVESDROPS_MOUSE, Some("true")),
            [
                "--file",
                "kwinrc",
                "--group",
                "Xwayland",
                "--key",
                EAVESDROPS_MOUSE,
                "true"
            ]
        );
        assert_eq!(write_argv(EAVESDROPS, None).last().expect("ok"), "--delete");
    }

    #[test]
    fn defaults_do_not_allow_mouse_buttons() {
        let (_dir, file) = undo_file();
        let state = access(&Fake::default(), &file).expect("ok");
        assert_eq!(
            state,
            Access {
                allowed: false,
                undo_available: false
            }
        );
    }

    #[test]
    fn all_keys_or_combinations_with_mouse_count_as_allowed() {
        let (_dir, file) = undo_file();
        let all = Fake::with(&[(EAVESDROPS, "All")]);
        assert!(access(&all, &file).expect("ok").allowed);
        let mouse = Fake::with(&[(EAVESDROPS_MOUSE, "true")]);
        assert!(access(&mouse, &file).expect("ok").allowed);
        let low = Fake::with(&[(EAVESDROPS, "Modifiers"), (EAVESDROPS_MOUSE, "true")]);
        assert!(!access(&low, &file).expect("ok").allowed);
    }

    #[test]
    fn allowing_sets_the_mouse_switch_and_undo_restores_the_old_values() {
        let (_dir, file) = undo_file();
        let config = Fake::with(&[(EAVESDROPS, "Modifiers")]);
        let state = allow_mouse(&config, &file).expect("ok");
        assert!(state.allowed && state.undo_available);
        assert_eq!(config.get(EAVESDROPS_MOUSE).as_deref(), Some("true"));
        assert_eq!(config.get(EAVESDROPS).as_deref(), Some("Combinations"));

        let state = undo(&config, &file).expect("ok");
        assert!(!state.allowed && !state.undo_available);
        assert_eq!(config.get(EAVESDROPS_MOUSE), None);
        assert_eq!(config.get(EAVESDROPS).as_deref(), Some("Modifiers"));
    }

    #[test]
    fn allowing_keeps_a_higher_key_level() {
        let (_dir, file) = undo_file();
        let config = Fake::with(&[(EAVESDROPS, "All")]);
        allow_mouse(&config, &file).expect("ok");
        assert_eq!(config.get(EAVESDROPS).as_deref(), Some("All"));
    }

    #[test]
    fn a_second_allow_keeps_the_original_undo_values() {
        let (_dir, file) = undo_file();
        let config = Fake::default();
        allow_mouse(&config, &file).expect("ok");
        allow_mouse(&config, &file).expect("ok");
        undo(&config, &file).expect("ok");
        assert_eq!(config.get(EAVESDROPS_MOUSE), None);
    }

    #[test]
    fn undo_without_a_saved_state_is_not_found() {
        let (_dir, file) = undo_file();
        let err = undo(&Fake::default(), &file).expect_err("must fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
