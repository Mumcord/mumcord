//! Finds the game being played for "Now gaming" (FR-1.59) and hands it to the engine, which
//! shares it. Sources are asked best first (see `mumcord_engine::now_gaming::detect` for what
//! each platform reads); the Mumble Link context adds the join target to [`SystemSource`]'s game in
//! [`sources`]. Nothing is read while the setting is off; reading runs off the UI thread every
//! `POLL_INTERVAL`, and the engine decides what changed. Game names never go into the logs.

mod link;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod tool;
#[cfg(target_os = "windows")]
mod windows;

use std::collections::HashMap;

use mumcord_engine::now_gaming::detect::{self, Os, Process};
use mumcord_engine::now_gaming::{POLL_INTERVAL, detect_first};
use mumcord_engine::{Engine, GameSource, LocalGame};

/// Whether this platform can look for games. Linux and Windows fully; macOS best effort (the
/// bundled list has few macOS entries and Steam rarely records the running game there).
pub const SUPPORTED: bool = Os::CURRENT.is_some();

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

/// Running programs and the Steam game, as this platform reports them.
#[derive(Default)]
struct SystemSource {
    /// Steam app id → name (`None`: unknown), so Steam's files are read once per game.
    names: HashMap<u32, Option<String>>,
}

impl GameSource for SystemSource {
    fn detect(&mut self) -> Option<LocalGame> {
        self.detect_in(&processes())
    }
}

impl SystemSource {
    /// Split from [`GameSource::detect`] so tests can hand in a fixed process list.
    fn detect_in(&mut self, processes: &[Process]) -> Option<LocalGame> {
        let os = Os::CURRENT?;
        let steam = running_steam_app(processes).and_then(|id| {
            let name = self
                .names
                .entry(id)
                .or_insert_with(|| steam_name(id))
                .clone();
            name.map(|name| (id, name))
        });
        detect::pick(detect::catalog(), os, processes, steam)
    }
}

/// The bundled list's name for a Steam game, else Steam's own.
fn steam_name(id: u32) -> Option<String> {
    detect::catalog()
        .by_steam_id(id)
        .map(|g| g.name.clone())
        .or_else(|| steam_app_name(id))
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn processes() -> Vec<Process> {
    platform::processes()
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn running_steam_app(processes: &[Process]) -> Option<u32> {
    platform::running_steam_app(processes)
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn steam_app_name(id: u32) -> Option<String> {
    platform::steam_app_name(id)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn processes() -> Vec<Process> {
    Vec::new()
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn running_steam_app(_processes: &[Process]) -> Option<u32> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn steam_app_name(_id: u32) -> Option<String> {
    None
}

/// The name in `appmanifest_<id>.acf` from any library of the Steam installs at `roots`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn manifest_name(roots: &[std::path::PathBuf], id: u32) -> Option<String> {
    let file = format!("appmanifest_{id}.acf");
    let mut libraries: Vec<std::path::PathBuf> = roots.to_vec();
    for root in roots {
        if let Ok(text) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            libraries.extend(detect::library_paths(&text).into_iter().map(Into::into));
        }
    }
    libraries
        .iter()
        .filter_map(|lib| std::fs::read_to_string(lib.join("steamapps").join(&file)).ok())
        .find_map(|acf| detect::manifest_name(&acf))
}

/// The game sources, best first.
fn sources() -> Vec<Box<dyn GameSource>> {
    vec![Box::new(link::LinkJoinSource::new(SystemSource::default()))]
}

/// Looks for games for the engine as long as the app runs.
pub fn start(engine: Engine) {
    tauri::async_runtime::spawn(async move {
        let mut sources = sources();
        let mut tick = tokio::time::interval(POLL_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let game = if engine.now_gaming_enabled() {
                let task = tokio::task::spawn_blocking(move || {
                    let game = detect_first(&mut sources);
                    (sources, game)
                });
                match task.await {
                    Ok((returned, game)) => {
                        sources = returned;
                        game
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "looking for a game failed");
                        return;
                    }
                }
            } else {
                None
            };
            engine.now_gaming_update(game);
        }
    });
}
