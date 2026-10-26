//! Linux: programs from `/proc`, the Steam game from Steam's `reaper SteamLaunch AppId=<id>`
//! wrapper (every game Steam starts runs under it; its command line is world-readable, unlike
//! the `SteamAppId` variable in a process environment), `RunningAppID` in `registry.vdf` as a
//! fallback, and game names from the `appmanifest_<id>.acf` files of Steam's libraries.
//! Native, Flatpak and the `~/.steam` symlink installs are covered.

use std::fs;
use std::path::{Path, PathBuf};

use mumcord_engine::now_gaming::detect::{self, Process};

/// Every running program this user can see, by executable name and arguments.
pub fn processes() -> Vec<Process> {
    let Ok(entries) = fs::read_dir("/proc") else {
        tracing::warn!("/proc is not readable; cannot look for games");
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        // Processes can end while being read; such gaps are expected and skipped.
        let args: Vec<String> = fs::read(dir.join("cmdline"))
            .map(|raw| {
                raw.split(|b| *b == 0)
                    .filter(|a| !a.is_empty())
                    .map(|a| String::from_utf8_lossy(a).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        let comm = fs::read_to_string(dir.join("comm"))
            .map(|c| c.trim_end().to_owned())
            .unwrap_or_default();
        let exe = args
            .first()
            .map(|a| detect::exe_name(a).to_owned())
            .unwrap_or_default();
        // `comm` is cut to 15 bytes but survives programs that rewrite their arguments.
        if !comm.is_empty() && comm != exe {
            out.push(Process {
                name: comm,
                args: Vec::new(),
            });
        }
        if !exe.is_empty() {
            out.push(Process { name: exe, args });
        }
    }
    out
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Steam installs that exist, without duplicates (`~/.steam/steam` usually links to one).
fn steam_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for candidate in [
        home.join(".steam/steam"),
        home.join(".local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ] {
        if let Ok(real) = candidate.canonicalize()
            && !roots.contains(&real)
        {
            roots.push(real);
        }
    }
    roots
}

pub fn running_steam_app(processes: &[Process]) -> Option<u32> {
    if let Some(id) = processes.iter().find_map(detect::steam_launch_app_id) {
        return Some(id);
    }
    let home = home()?;
    [
        home.join(".steam/registry.vdf"),
        home.join(".var/app/com.valvesoftware.Steam/.steam/registry.vdf"),
    ]
    .iter()
    .filter_map(|path| fs::read_to_string(path).ok())
    .find_map(|text| detect::registry_running_app_id(&text))
}

pub fn steam_app_name(id: u32) -> Option<String> {
    let home = home()?;
    super::manifest_name(&steam_roots(&home), id)
}

#[cfg(test)]
mod tests {
    use mumcord_engine::now_gaming::detect::{self, Os, Process};

    use super::super::SystemSource;

    fn process(name: &str, args: &[&str]) -> Process {
        Process {
            name: name.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    /// `/proc` lists this very test process; nothing else on the machine is assumed.
    #[test]
    fn lists_own_process_from_proc() {
        let me = std::env::current_exe().expect("test binary");
        let exe = detect::exe_name(me.to_str().expect("utf-8 path")).to_owned();
        let own_comm: String = exe.chars().take(15).collect();
        assert!(
            super::processes()
                .iter()
                .any(|p| p.name == exe || p.name == own_comm),
            "own process not listed"
        );
    }

    /// A listed program is found by its name (picked directly: the Steam fallback reads the
    /// real `registry.vdf`).
    #[test]
    fn finds_listed_program() {
        let processes = [process("factorio", &["/opt/factorio/bin/x64/factorio"])];
        let game = detect::pick(
            detect::catalog(),
            Os::CURRENT.expect("linux"),
            &processes,
            None,
        )
        .expect("listed program found");
        assert_eq!(game.name, "Factorio");
        assert_eq!(game.steam_app_id, Some(427_520));
    }

    /// Steam's launch wrapper wins over a listed program and needs no Steam files.
    #[test]
    fn finds_steam_launch_wrapper() {
        let processes = [
            process("factorio", &["factorio"]),
            process(
                "reaper",
                &["reaper", "SteamLaunch", "AppId=227300", "--", "eurotrucks2"],
            ),
        ];
        let game = SystemSource::default()
            .detect_in(&processes)
            .expect("steam game found");
        assert_eq!(game.name, "Euro Truck Simulator 2");
        assert_eq!(game.steam_app_id, Some(227_300));
    }
}
