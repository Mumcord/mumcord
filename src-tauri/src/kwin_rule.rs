//! KDE Plasma (Wayland): a KWin window rule for the overlay window.
//!
//! A native Wayland window can neither stay on top nor place itself, but KWin can force both
//! through a window rule in `kwinrulesrc`. This module writes, updates and removes one rule,
//! found again through its stable description, and asks KWin to reload its rules over D-Bus.
//! The rest of the file (other rules, comments, the legacy `Order` key) is left untouched.

use std::io;
use std::path::{Path, PathBuf};

/// Shown in KDE's window rule list; also how the rule is found again.
const DESCRIPTION: &str = "Mumcord overlay";
/// Group name of a newly created rule.
const RULE_ID: &str = "mumcord-overlay";
/// Window class (Wayland app id) of every Mumcord window: the Tauri identifier.
const WINDOW_CLASS: &str = "app.mumcord.client";
/// Title of the overlay window (`overlay::open`, `ui/overlay.html`).
const WINDOW_TITLE: &str = "Mumcord overlay";

/// KWin rule policies (`<name>rule`): force the value for as long as the window lives.
const FORCE: &str = "2";

/// Where the overlay window is forced to, in logical desktop pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// `kwinrulesrc` in the user's config directory.
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(base.join("kwinrulesrc"))
}

/// A config file as a preamble plus ordered `[group]`s of raw lines, so unknown content
/// survives a round trip.
#[derive(Default)]
struct Ini {
    preamble: Vec<String>,
    groups: Vec<(String, Vec<String>)>,
}

impl Ini {
    fn parse(text: &str) -> Self {
        let mut ini = Self::default();
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(name) = trimmed
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
            {
                ini.groups.push((name.to_owned(), Vec::new()));
            } else if let Some((_, lines)) = ini.groups.last_mut() {
                lines.push(line.to_owned());
            } else {
                ini.preamble.push(line.to_owned());
            }
        }
        for (_, lines) in &mut ini.groups {
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
        }
        ini
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.preamble {
            out.push_str(line);
            out.push('\n');
        }
        for (name, lines) in &self.groups {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("[{name}]\n"));
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    fn group(&self, name: &str) -> Option<&Vec<String>> {
        self.groups.iter().find(|(n, _)| n == name).map(|(_, l)| l)
    }

    fn group_mut(&mut self, name: &str) -> &mut Vec<String> {
        let index = match self.groups.iter().position(|(n, _)| n == name) {
            Some(index) => index,
            None => {
                self.groups.push((name.to_owned(), Vec::new()));
                self.groups.len() - 1
            }
        };
        &mut self.groups[index].1
    }

    fn value(&self, group: &str, key: &str) -> Option<&str> {
        self.group(group)?
            .iter()
            .find_map(|line| value_of(line, key))
    }

    fn set(&mut self, group: &str, key: &str, value: &str) {
        let lines = self.group_mut(group);
        let line = format!("{key}={value}");
        match lines.iter_mut().find(|l| value_of(l, key).is_some()) {
            Some(existing) => *existing = line,
            None => lines.push(line),
        }
    }

    /// The group whose `Description` is ours.
    fn our_rule(&self) -> Option<&str> {
        self.groups
            .iter()
            .find(|(name, _)| {
                name != "General" && self.value(name, "Description") == Some(DESCRIPTION)
            })
            .map(|(name, _)| name.as_str())
    }

    /// Rule ids in order. KWin 6 reads `rules`; older files list `1..=count` as group names.
    fn rule_ids(&self) -> Vec<String> {
        let listed = self.value("General", "rules").unwrap_or_default();
        if !listed.trim().is_empty() {
            return listed
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect();
        }
        let count: u32 = self
            .value("General", "count")
            .and_then(|c| c.trim().parse().ok())
            .unwrap_or(0);
        (1..=count).map(|n| n.to_string()).collect()
    }

    fn set_rule_ids(&mut self, ids: &[String]) {
        self.set("General", "count", &ids.len().to_string());
        self.set("General", "rules", &ids.join(","));
    }
}

fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (k, v) = line.split_once('=')?;
    (k.trim() == key).then_some(v.trim())
}

/// Whether `content` has our rule.
pub fn is_installed(content: &str) -> bool {
    Ini::parse(content).our_rule().is_some()
}

/// `content` with our rule added, or updated to `placement` if it exists. Idempotent.
pub fn with_rule(content: &str, placement: Placement) -> String {
    let mut ini = Ini::parse(content);
    // Created first so a new file starts with [General], like KDE writes it.
    ini.group_mut("General");
    let id = ini
        .our_rule()
        .map_or_else(|| RULE_ID.to_owned(), str::to_owned);
    let mut ids = ini.rule_ids();
    if !ids.contains(&id) {
        ids.push(id.clone());
    }
    // The whole group is rewritten, so a rule edited by hand converges on ours.
    *ini.group_mut(&id) = Vec::new();
    let force = |ini: &mut Ini, key: &str, value: &str| {
        ini.set(&id, key, value);
        ini.set(&id, &format!("{key}rule"), FORCE);
    };
    ini.set(&id, "Description", DESCRIPTION);
    ini.set(&id, "wmclass", WINDOW_CLASS);
    ini.set(&id, "wmclasscomplete", "false");
    ini.set(&id, "wmclassmatch", "1");
    ini.set(&id, "title", WINDOW_TITLE);
    ini.set(&id, "titlematch", "1");
    // Normal windows only, so a dialog of the app can never match.
    ini.set(&id, "types", "1");
    force(&mut ini, "above", "true");
    force(
        &mut ini,
        "position",
        &format!("{},{}", placement.x, placement.y),
    );
    force(
        &mut ini,
        "size",
        &format!("{},{}", placement.width, placement.height),
    );
    force(&mut ini, "skiptaskbar", "true");
    force(&mut ini, "skippager", "true");
    force(&mut ini, "skipswitcher", "true");
    force(&mut ini, "acceptfocus", "false");
    ini.set_rule_ids(&ids);
    ini.render()
}

/// `content` without our rule.
pub fn without_rule(content: &str) -> String {
    let mut ini = Ini::parse(content);
    let Some(id) = ini.our_rule().map(str::to_owned) else {
        return content.to_owned();
    };
    ini.groups.retain(|(name, _)| *name != id);
    let ids: Vec<String> = ini.rule_ids().into_iter().filter(|i| *i != id).collect();
    ini.set_rule_ids(&ids);
    ini.render()
}

fn read(path: &Path) -> io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// Replaces the file in one step, so KWin never reads a half-written rule list.
fn write(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("mumcord-tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Whether the rule exists in the file at `path`.
pub fn installed_in(path: &Path) -> io::Result<bool> {
    Ok(is_installed(&read(path)?))
}

/// Adds or updates the rule in the file at `path`. Returns whether the file changed.
pub fn install_in(path: &Path, placement: Placement) -> io::Result<bool> {
    let old = read(path)?;
    let new = with_rule(&old, placement);
    if new == old {
        return Ok(false);
    }
    write(path, &new)?;
    Ok(true)
}

/// Removes the rule from the file at `path`. Returns whether the file changed.
pub fn remove_in(path: &Path) -> io::Result<bool> {
    let old = read(path)?;
    let new = without_rule(&old);
    if new == old {
        return Ok(false);
    }
    write(path, &new)?;
    Ok(true)
}

/// Asks KWin to reload its window rules.
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

    const AT: Placement = Placement {
        x: 16,
        y: 16,
        width: 240,
        height: 320,
    };

    #[test]
    fn adds_the_rule_to_an_empty_file() {
        let text = with_rule("", AT);
        let ini = Ini::parse(&text);
        assert_eq!(ini.value("General", "count"), Some("1"));
        assert_eq!(ini.value("General", "rules"), Some(RULE_ID));
        assert_eq!(ini.value(RULE_ID, "Description"), Some(DESCRIPTION));
        assert_eq!(ini.value(RULE_ID, "wmclass"), Some("app.mumcord.client"));
        assert_eq!(ini.value(RULE_ID, "title"), Some("Mumcord overlay"));
        assert_eq!(ini.value(RULE_ID, "above"), Some("true"));
        assert_eq!(ini.value(RULE_ID, "aboverule"), Some("2"));
        assert_eq!(ini.value(RULE_ID, "position"), Some("16,16"));
        assert_eq!(ini.value(RULE_ID, "positionrule"), Some("2"));
        assert_eq!(ini.value(RULE_ID, "size"), Some("240,320"));
        assert!(is_installed(&text));
    }

    #[test]
    fn updates_in_place_without_duplicates() {
        let first = with_rule("", AT);
        let moved = with_rule(
            &first,
            Placement {
                x: 1500,
                y: 900,
                ..AT
            },
        );
        let ini = Ini::parse(&moved);
        assert_eq!(ini.value(RULE_ID, "position"), Some("1500,900"));
        assert_eq!(ini.value("General", "count"), Some("1"));
        assert_eq!(ini.groups.len(), 2);
        assert_eq!(
            with_rule(
                &moved,
                Placement {
                    x: 1500,
                    y: 900,
                    ..AT
                }
            ),
            moved
        );
    }

    #[test]
    fn keeps_other_rules_and_legacy_keys() {
        let other = "[General]\nOrder=steam\ncount=1\nrules=abc\n\n[abc]\nDescription=Steam\nwmclass=steam\n";
        let added = with_rule(other, AT);
        let ini = Ini::parse(&added);
        assert_eq!(ini.value("General", "Order"), Some("steam"));
        assert_eq!(ini.value("General", "count"), Some("2"));
        assert_eq!(ini.value("General", "rules"), Some("abc,mumcord-overlay"));
        assert_eq!(ini.value("abc", "wmclass"), Some("steam"));

        let removed = without_rule(&added);
        let ini = Ini::parse(&removed);
        assert!(!is_installed(&removed));
        assert_eq!(ini.value("General", "count"), Some("1"));
        assert_eq!(ini.value("General", "rules"), Some("abc"));
        assert_eq!(ini.value("abc", "Description"), Some("Steam"));
        assert_eq!(ini.value("General", "Order"), Some("steam"));
    }

    #[test]
    fn upgrades_a_legacy_numbered_rule_list() {
        let legacy = "[General]\ncount=2\n\n[1]\nDescription=A\n\n[2]\nDescription=B\n";
        let ini = Ini::parse(&with_rule(legacy, AT));
        assert_eq!(ini.value("General", "rules"), Some("1,2,mumcord-overlay"));
        assert_eq!(ini.value("General", "count"), Some("3"));
    }

    #[test]
    fn finds_a_rule_that_kde_renamed() {
        let text =
            "[General]\ncount=1\nrules=7f\n\n[7f]\nDescription=Mumcord overlay\nabove=false\n";
        let ini = Ini::parse(&with_rule(text, AT));
        assert_eq!(ini.value("7f", "above"), Some("true"));
        assert_eq!(ini.groups.len(), 2);
        assert_eq!(ini.value("General", "rules"), Some("7f"));
    }

    #[test]
    fn removing_a_missing_rule_changes_nothing() {
        let text = "[General]\ncount=0\n";
        assert_eq!(without_rule(text), text);
    }

    #[test]
    fn files_are_created_updated_and_cleaned_up() {
        let dir = std::env::temp_dir().join(format!("mumcord-kwin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("kwinrulesrc");
        assert!(!installed_in(&path).expect("missing file"));
        assert!(install_in(&path, AT).expect("install"));
        assert!(!install_in(&path, AT).expect("unchanged"));
        assert!(installed_in(&path).expect("installed"));
        assert!(remove_in(&path).expect("remove"));
        assert!(!remove_in(&path).expect("already gone"));
        assert!(!installed_in(&path).expect("gone"));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
