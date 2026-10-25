//! Pure helpers for finding the game being played (FR-1.59); the app shell does the reading
//! (process list, Steam files, registry) and hands the raw text here, so all parsing is tested
//! on every platform.
//!
//! Sources, best first:
//! 1. The running Steam game. Linux: the `reaper SteamLaunch AppId=<id> -- …` wrapper process
//!    Steam starts every game with (its command line is readable without special rights);
//!    `RunningAppID` in Steam's `registry.vdf` as a fallback for older clients and macOS.
//!    Windows: `HKCU\Software\Valve\Steam\RunningAppID`. The name comes from the bundled list,
//!    else from the game's `appmanifest_<id>.acf` (Linux/macOS) or the `Apps\<id>\Name`
//!    registry value (Windows).
//! 2. Running programs matched against the bundled list of well-known games (`games.json`):
//!    executable names per OS; on Linux the Windows names count too (Proton).
//!
//! A join target is only offered for a game launched with `+connect <host:port>` (Source
//! engine games joined through a server browser), as `steam://connect/<host:port>`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use super::LocalGame;
use super::payload::{STEAM_CONNECT, join_link};

/// The default port of Source engine game servers, used when `+connect` names none.
const SOURCE_PORT: u16 = 27015;

/// The operating system whose executable names apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    Macos,
}

impl Os {
    /// The system this build runs on (`None` elsewhere).
    pub const CURRENT: Option<Os> = if cfg!(target_os = "linux") {
        Some(Os::Linux)
    } else if cfg!(target_os = "windows") {
        Some(Os::Windows)
    } else if cfg!(target_os = "macos") {
        Some(Os::Macos)
    } else {
        None
    };
}

/// One entry of the bundled list.
#[derive(Debug, Clone, Deserialize)]
pub struct KnownGame {
    pub name: String,
    #[serde(default)]
    pub steam: Option<u32>,
    #[serde(default)]
    windows: Vec<String>,
    #[serde(default)]
    linux: Vec<String>,
    #[serde(default)]
    macos: Vec<String>,
}

#[derive(Deserialize)]
struct CatalogFile {
    games: Vec<KnownGame>,
}

/// The bundled list of well-known games.
pub struct Catalog {
    games: Vec<KnownGame>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    match serde_json::from_str::<CatalogFile>(include_str!("games.json")) {
        Ok(file) => Catalog { games: file.games },
        Err(e) => {
            // Covered by a test; never expected in a release.
            tracing::error!(error = %e, "the bundled game list is invalid");
            Catalog { games: Vec::new() }
        }
    }
});

/// The bundled list, parsed once.
pub fn catalog() -> &'static Catalog {
    &CATALOG
}

impl Catalog {
    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    pub fn by_steam_id(&self, id: u32) -> Option<&KnownGame> {
        self.games.iter().find(|g| g.steam == Some(id))
    }

    /// The first listed game one of `processes` belongs to, by executable name (any case).
    pub fn find_running(&self, os: Os, processes: &[Process]) -> Option<&KnownGame> {
        let mut by_name: HashMap<String, &KnownGame> = HashMap::new();
        for game in &self.games {
            let names: &[&Vec<String>] = match os {
                Os::Windows => &[&game.windows],
                // Windows games under Proton keep their `.exe` name.
                Os::Linux => &[&game.linux, &game.windows],
                Os::Macos => &[&game.macos],
            };
            for name in names.iter().flat_map(|n| n.iter()) {
                by_name.entry(name.to_lowercase()).or_insert(game);
            }
        }
        processes
            .iter()
            .find_map(|p| by_name.get(&p.name.to_lowercase()).copied())
    }
}

/// A running program: its executable file name and, where readable, its arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Process {
    pub name: String,
    pub args: Vec<String>,
}

/// The file name of a program path; Windows paths (also from Wine/Proton) count too.
pub fn exe_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The app id of Steam's `reaper SteamLaunch AppId=<id> …` game wrapper (Linux).
pub fn steam_launch_app_id(process: &Process) -> Option<u32> {
    if !process.args.iter().any(|a| a == "SteamLaunch") {
        return None;
    }
    process
        .args
        .iter()
        .find_map(|a| a.strip_prefix("AppId="))
        .and_then(|id| id.parse().ok())
        .filter(|id| *id > 0)
}

/// `steam://connect/<host:port>` for a game started with `+connect <address>`.
pub fn connect_target(process: &Process) -> Option<String> {
    let at = process.args.iter().position(|a| a == "+connect")?;
    let address = process.args.get(at + 1)?;
    let link = if address.contains(':') {
        format!("{STEAM_CONNECT}{address}")
    } else {
        format!("{STEAM_CONNECT}{address}:{SOURCE_PORT}")
    };
    join_link(&link)
}

/// Picks the game from what the shell read: the Steam game (id and name) wins over a listed
/// program. The join target comes from any process started with `+connect`.
pub fn pick(
    catalog: &Catalog,
    os: Os,
    processes: &[Process],
    steam: Option<(u32, String)>,
) -> Option<LocalGame> {
    let (name, steam_app_id) = match steam {
        Some((id, name)) => (name, Some(id)),
        None => {
            let known = catalog.find_running(os, processes)?;
            (known.name.clone(), known.steam)
        }
    };
    let join = processes.iter().find_map(connect_target);
    Some(LocalGame {
        name,
        steam_app_id,
        join,
    })
}

#[derive(Debug, PartialEq)]
enum Token {
    Text(String),
    Open,
    Close,
}

fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '"' => {
                let mut value = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(escaped @ ('\\' | '"')) => value.push(escaped),
                            Some(other) => {
                                value.push('\\');
                                value.push(other);
                            }
                            None => {}
                        },
                        c => value.push(c),
                    }
                }
                out.push(Token::Text(value));
            }
            // `//` comments run to the end of the line.
            '/' => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Every value stored under `key` (any case, any depth) in a Valve KeyValues (`.vdf`/`.acf`)
/// text, in file order.
pub fn vdf_values(text: &str, key: &str) -> Vec<String> {
    let tokens = tokens(text);
    let mut values = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match (&tokens[i], tokens.get(i + 1)) {
            (Token::Text(k), Some(Token::Text(v))) => {
                if k.eq_ignore_ascii_case(key) {
                    values.push(v.clone());
                }
                i += 2;
            }
            // A section name, or something malformed: step over it.
            _ => i += 1,
        }
    }
    values
}

/// `RunningAppID` from Steam's `registry.vdf` (`None`: no game, or not recorded).
pub fn registry_running_app_id(registry_vdf: &str) -> Option<u32> {
    vdf_values(registry_vdf, "RunningAppID")
        .first()
        .and_then(|v| v.trim().parse().ok())
        .filter(|id| *id > 0)
}

/// The library folders listed in Steam's `libraryfolders.vdf`.
pub fn library_paths(libraryfolders_vdf: &str) -> Vec<String> {
    vdf_values(libraryfolders_vdf, "path")
}

/// The game name from an `appmanifest_<id>.acf`.
pub fn manifest_name(acf: &str) -> Option<String> {
    vdf_values(acf, "name")
        .into_iter()
        .next()
        .filter(|n| !n.trim().is_empty())
}

/// A `REG_DWORD` value from `reg query … /v <name>` output (Windows).
pub fn reg_dword(output: &str, name: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if !parts.next()?.eq_ignore_ascii_case(name) || parts.next()? != "REG_DWORD" {
            return None;
        }
        let hex = parts.next()?;
        u32::from_str_radix(hex.trim_start_matches("0x"), 16).ok()
    })
}

/// A `REG_SZ` value from `reg query … /v <name>` output (Windows).
pub fn reg_string(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let rest = line.trim_start();
        let (key, rest) = rest.split_once(char::is_whitespace)?;
        if !key.eq_ignore_ascii_case(name) {
            return None;
        }
        let value = rest.trim_start().strip_prefix("REG_SZ")?.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// Program names from `tasklist /fo csv /nh` output (Windows).
pub fn tasklist_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix('"')?.split('"').next())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str, args: &[&str]) -> Process {
        Process {
            name: name.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn bundled_list_is_valid() {
        let catalog = catalog();
        assert!(catalog.len() >= 50, "{} games", catalog.len());
        let mut ids = std::collections::HashSet::new();
        for game in &catalog.games {
            assert!(!game.name.trim().is_empty());
            assert!(
                !(game.windows.is_empty() && game.linux.is_empty() && game.macos.is_empty()),
                "{} has no executable",
                game.name
            );
            if let Some(id) = game.steam {
                assert!(ids.insert(id), "steam id {id} listed twice");
            }
        }
        assert_eq!(
            catalog.by_steam_id(730).map(|g| g.name.as_str()),
            Some("Counter-Strike 2")
        );
    }

    #[test]
    fn matches_programs_by_name_per_os() {
        let catalog = catalog();
        let running = [process("bash", &[]), process("CS2.EXE", &[])];
        let found = catalog.find_running(Os::Windows, &running).expect("cs2");
        assert_eq!(found.name, "Counter-Strike 2");
        assert_eq!(found.steam, Some(730));
        // Proton keeps the Windows name on Linux; macOS has no such entry.
        assert!(catalog.find_running(Os::Linux, &running).is_some());
        assert!(catalog.find_running(Os::Macos, &running).is_none());
        assert_eq!(
            catalog
                .find_running(Os::Linux, &[process("factorio", &[])])
                .map(|g| g.name.as_str()),
            Some("Factorio")
        );
        assert!(
            catalog
                .find_running(Os::Linux, &[process("firefox", &[])])
                .is_none()
        );
    }

    #[test]
    fn exe_names_from_unix_and_windows_paths() {
        assert_eq!(exe_name("/usr/bin/factorio"), "factorio");
        assert_eq!(
            exe_name(r"Z:\games\ELDEN RING\Game\eldenring.exe"),
            "eldenring.exe"
        );
        assert_eq!(exe_name("cs2"), "cs2");
    }

    #[test]
    fn reads_the_steam_launch_wrapper() {
        let reaper = process(
            "reaper",
            &[
                "/home/u/.local/share/Steam/ubuntu12_32/reaper",
                "SteamLaunch",
                "AppId=227300",
                "--",
                "/home/u/.local/share/Steam/steamapps/common/Euro Truck Simulator 2/bin/linux_x64/eurotrucks2",
            ],
        );
        assert_eq!(steam_launch_app_id(&reaper), Some(227_300));
        assert_eq!(
            steam_launch_app_id(&process("game", &["AppId=227300"])),
            None
        );
        assert_eq!(
            steam_launch_app_id(&process("reaper", &["SteamLaunch", "AppId=0"])),
            None
        );
    }

    #[test]
    fn connect_arguments_become_join_targets() {
        let game = process("cs2", &["cs2", "-steam", "+connect", "192.0.2.10:27016"]);
        assert_eq!(
            connect_target(&game).as_deref(),
            Some("steam://connect/192.0.2.10:27016")
        );
        let no_port = process("hl2_linux", &["+connect", "play.example.org"]);
        assert_eq!(
            connect_target(&no_port).as_deref(),
            Some("steam://connect/play.example.org:27015")
        );
        assert_eq!(connect_target(&process("x", &["+connect"])), None);
        assert_eq!(
            connect_target(&process("x", &["+connect", "a;rm -rf:1"])),
            None
        );
    }

    #[test]
    fn steam_wins_over_listed_programs() {
        let catalog = catalog();
        let running = [
            process("factorio", &[]),
            process("cs2", &["+connect", "192.0.2.10:27015"]),
        ];
        let steam = pick(
            catalog,
            Os::Linux,
            &running,
            Some((730, "Counter-Strike 2".into())),
        )
        .expect("game");
        assert_eq!(steam.steam_app_id, Some(730));
        assert_eq!(
            steam.join.as_deref(),
            Some("steam://connect/192.0.2.10:27015")
        );
        let listed = pick(catalog, Os::Linux, &running[..1], None).expect("game");
        assert_eq!(listed.name, "Factorio");
        assert_eq!(listed.steam_app_id, Some(427_520));
        assert_eq!(listed.join, None);
        assert_eq!(pick(catalog, Os::Linux, &[], None), None);
    }

    const REGISTRY: &str = include_str!("fixtures/registry.vdf");
    const LIBRARIES: &str = include_str!("fixtures/libraryfolders.vdf");
    const MANIFEST: &str = include_str!("fixtures/appmanifest_227300.acf");

    #[test]
    fn parses_steam_files() {
        assert_eq!(registry_running_app_id(REGISTRY), Some(570));
        assert_eq!(
            registry_running_app_id(r#""Registry" { "RunningAppID" "0" }"#),
            None
        );
        assert_eq!(registry_running_app_id(r#""Registry" { }"#), None);
        assert_eq!(
            library_paths(LIBRARIES),
            vec![
                "/home/u/.local/share/Steam".to_owned(),
                r"D:\SteamLibrary".to_owned()
            ]
        );
        assert_eq!(
            manifest_name(MANIFEST).as_deref(),
            Some("Euro Truck Simulator 2")
        );
    }

    #[test]
    fn parses_windows_tool_output() {
        let reg = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    RunningAppID    REG_DWORD    0x2da\r\n\r\n";
        assert_eq!(reg_dword(reg, "RunningAppID"), Some(730));
        assert_eq!(reg_dword(reg, "Other"), None);
        let name = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\\Apps\\730\r\n    Name    REG_SZ    Counter-Strike 2\r\n";
        assert_eq!(
            reg_string(name, "Name").as_deref(),
            Some("Counter-Strike 2")
        );
        let tasks = "\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"\r\n\"cs2.exe\",\"4242\",\"Console\",\"1\",\"1.234.567 K\"\r\n";
        assert_eq!(
            tasklist_names(tasks),
            vec!["System Idle Process".to_owned(), "cs2.exe".to_owned()]
        );
    }
}
