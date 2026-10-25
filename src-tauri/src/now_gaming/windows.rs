//! Windows: programs from `tasklist`, the Steam game from the registry value
//! `HKCU\Software\Valve\Steam\RunningAppID` (Steam keeps it current while a game runs) and its
//! name from `HKCU\Software\Valve\Steam\Apps\<id>\Name`.

use mumcord_engine::now_gaming::detect::{self, Process};

use super::tool;

const STEAM_KEY: &str = r"HKCU\Software\Valve\Steam";

pub fn processes() -> Vec<Process> {
    let Some(list) = tool::output("tasklist", &["/fo", "csv", "/nh"]) else {
        tracing::warn!("tasklist failed; cannot look for games");
        return Vec::new();
    };
    detect::tasklist_names(&list)
        .into_iter()
        .map(|name| Process {
            name,
            args: Vec::new(),
        })
        .collect()
}

pub fn running_steam_app(_processes: &[Process]) -> Option<u32> {
    let out = tool::output("reg", &["query", STEAM_KEY, "/v", "RunningAppID"])?;
    detect::reg_dword(&out, "RunningAppID").filter(|id| *id > 0)
}

pub fn steam_app_name(id: u32) -> Option<String> {
    let key = format!(r"{STEAM_KEY}\Apps\{id}");
    let out = tool::output("reg", &["query", key.as_str(), "/v", "Name"])?;
    detect::reg_string(&out, "Name")
}
