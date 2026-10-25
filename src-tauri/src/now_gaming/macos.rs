//! macOS (best effort): programs from `ps`, the Steam game from `RunningAppID` in Steam's
//! `registry.vdf` (not always recorded) and names from the libraries' app manifests.

use std::path::PathBuf;

use mumcord_engine::now_gaming::detect::{self, Process};

use super::tool;

fn steam_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/Steam"))
}

pub fn processes() -> Vec<Process> {
    let Some(list) = tool::output("ps", &["-axo", "comm="]) else {
        tracing::warn!("ps failed; cannot look for games");
        return Vec::new();
    };
    list.lines()
        .map(|line| detect::exe_name(line.trim()))
        .filter(|name| !name.is_empty())
        .map(|name| Process {
            name: name.to_owned(),
            args: Vec::new(),
        })
        .collect()
}

pub fn running_steam_app(_processes: &[Process]) -> Option<u32> {
    let text = std::fs::read_to_string(steam_root()?.join("registry.vdf")).ok()?;
    detect::registry_running_app_id(&text)
}

pub fn steam_app_name(id: u32) -> Option<String> {
    super::manifest_name(&[steam_root()?], id)
}
