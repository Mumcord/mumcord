//! In-game overlay (FR-1.46b), app side: the local IPC server games connect to, the Vulkan
//! layer manifest install, and the scene the overlay shows.
//!
//! Platforms:
//! - **Linux:** a per-user abstract Unix-socket server; games (also inside the Steam Linux
//!   Runtime container) reach it through the Vulkan layer or the OpenGL preload
//!   (`mumcord-overlay-layer`). Turning the overlay on installs the layer manifest, off removes
//!   it.
//! - **Windows:** designed around a hook DLL loaded into detected, allowed games (see
//!   `docs/engine-api.md`); not built on this Linux host. [`SUPPORTED`] is false there until it
//!   is implemented and verified on Windows.
//! - **macOS:** no overlay at all (QA.58); [`SUPPORTED`] is false.

#[cfg(target_os = "linux")]
mod manifest;
mod scene;
#[cfg(target_os = "linux")]
mod server;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use mumcord_engine::Engine;
use mumcord_engine::store::{
    GameKey, GameOverlayEntry, InGameOverlayMode, InGameOverlaySettings, Settings,
};
use mumcord_overlay_ipc::wire::{Config, Corner, Hello, Scene};

/// Whether this platform can draw inside games. Linux only for now (the Windows hook is designed
/// but not yet built here; macOS is never supported, QA.58).
pub const SUPPORTED: bool = cfg!(target_os = "linux");

/// The game-side identity reduced to how settings and the blocklist key a game.
pub fn key_of(hello: &Hello) -> GameKey {
    match hello.steam_app_id {
        Some(id) => GameKey::Steam(id),
        None => GameKey::Exe(hello.exe.clone()),
    }
}

/// Shared state the IPC server reads: the current settings and scene. Guarded by a mutex; the
/// server only holds it briefly to copy what it sends, never across a blocking write.
#[derive(Default)]
pub struct Shared {
    inner: Mutex<State>,
}

#[derive(Default)]
struct State {
    ingame: InGameOverlaySettings,
    scene: Scene,
    talking: HashSet<u32>,
    /// Games that connected since the app started (by key and the name they reported), newest
    /// last. Lets the settings page offer games the user actually ran, even unknown ones.
    seen: Vec<SeenGame>,
}

/// A game that loaded the overlay at least once this session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeenGame {
    pub key: GameKey,
    /// Executable name reported over IPC (empty for a pure Steam launch without one).
    pub exe: String,
}

impl Shared {
    /// Replaces the in-game overlay settings the server answers with.
    pub fn set_settings(&self, ingame: InGameOverlaySettings) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ingame = ingame;
    }

    /// Records that `session` started or stopped talking; returns whether the scene changed.
    pub fn set_talking(&self, session: u32, talking: bool) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if talking {
            state.talking.insert(session)
        } else {
            state.talking.remove(&session)
        }
    }

    /// Records a game that connected (deduplicated by key, most recent name kept).
    pub fn record_seen(&self, game: SeenGame) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.seen.retain(|g| g.key != game.key);
        state.seen.push(game);
        // Bound the list so a session that launches many games cannot grow it without end.
        let len = state.seen.len();
        if len > 32 {
            state.seen.drain(0..len - 32);
        }
    }

    /// The games seen this session, newest last.
    pub fn seen_games(&self) -> Vec<SeenGame> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .seen
            .clone()
    }

    /// Clears the talking set (e.g. on disconnect).
    pub fn clear_talking(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .talking
            .clear();
    }

    /// Rebuilds the scene from a fresh engine snapshot, keeping the current talking set.
    pub fn rebuild(&self, engine: &Engine) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(snapshot) = engine.snapshot() else {
            state.scene = Scene::default();
            return;
        };
        let self_channel = snapshot
            .users
            .iter()
            .find(|u| u.session == snapshot.self_session)
            .map(|u| u.channel_id);
        let Some(channel_id) = self_channel else {
            state.scene = Scene::default();
            return;
        };
        let channel_name = snapshot
            .channels
            .iter()
            .find(|c| c.id == channel_id)
            .map(|c| c.name.as_str())
            .unwrap_or_default();
        let people: Vec<scene::ScenePerson<'_>> = snapshot
            .users
            .iter()
            .filter(|u| u.channel_id == channel_id)
            .map(|u| scene::ScenePerson {
                session: u.session,
                name: &u.name,
                muted: u.self_mute || u.server_mute,
                deafened: u.self_deaf || u.server_deaf,
            })
            .collect();
        let input = scene::SceneInput {
            self_session: snapshot.self_session,
            channel_name,
            people,
        };
        // The stored scene is unfiltered (everyone, with talking flags); each game filters it.
        let all = GameOverlayEntry {
            show_whole_channel: true,
            show_channel_name: true,
            ..GameOverlayEntry::default()
        };
        let built = scene::build(&input, &state.talking, &all);
        state.scene = built;
    }

    /// The config to send a game: the decision plus its per-game look.
    pub fn config_for(&self, key: &GameKey) -> Config {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let decision = mumcord_engine::decide_overlay(&state.ingame, key);
        if !decision.shows() {
            return Config::DISABLED;
        }
        let entry = state.ingame.entry(key).cloned().unwrap_or_default();
        Config {
            enabled: true,
            corner: to_wire_corner(entry.corner),
            size_percent: entry.size_percent,
            opacity_percent: entry.opacity_percent,
            show_whole_channel: entry.show_whole_channel,
            show_channel_name: entry.show_channel_name,
        }
    }

    /// The scene to send a game, filtered by its per-game options.
    pub fn scene_for(&self, key: &GameKey) -> Scene {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = state.ingame.entry(key).cloned().unwrap_or_default();
        filter_scene(&state.scene, &entry)
    }
}

fn filter_scene(scene: &Scene, entry: &GameOverlayEntry) -> Scene {
    Scene {
        channel: if entry.show_channel_name {
            scene.channel.clone()
        } else {
            String::new()
        },
        people: scene
            .people
            .iter()
            .filter(|p| entry.show_whole_channel || p.talking)
            .cloned()
            .collect(),
    }
}

/// What this platform supports, for the settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    pub supported: bool,
    /// i18n key (namespace `settings`) explaining a limitation, or `None`.
    pub explanation_key: Option<&'static str>,
}

/// Platform support (FR-1.46b, QA.58).
#[cfg(target_os = "linux")]
pub fn support() -> Support {
    support_with(library_path())
}

/// Platform support: the Windows hook is designed but not yet shipped; the window overlay is the
/// fallback.
#[cfg(target_os = "windows")]
pub fn support() -> Support {
    Support {
        supported: false,
        explanation_key: Some("overlay.inGame.limits.windowsPending"),
    }
}

/// Platform support: macOS has no overlay at all (QA.58).
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn support() -> Support {
    Support {
        supported: false,
        explanation_key: Some("overlay.inGame.limits.macos"),
    }
}

/// Linux support given the located layer library: without it no game could load the overlay, so
/// the settings say why instead of offering a switch that does nothing.
#[cfg(target_os = "linux")]
fn support_with(library: Option<std::path::PathBuf>) -> Support {
    match library {
        Some(_) => Support {
            supported: true,
            explanation_key: None,
        },
        None => Support {
            supported: false,
            explanation_key: Some("overlay.inGame.limits.libraryMissing"),
        },
    }
}

/// App-wide runtime, managed by Tauri. Holds the shared state the IPC server reads and keeps the
/// server alive while the app runs.
pub struct Runtime {
    pub shared: Arc<Shared>,
    #[cfg(target_os = "linux")]
    _server: Mutex<Option<server::Server>>,
}

/// File name of the Linux layer library (`crates/overlay-layer`).
#[cfg(target_os = "linux")]
const LIBRARY_NAME: &str = "libmumcord_overlay_layer.so";

/// The overlay layer library to point the Vulkan manifest at, if it exists: `MUMCORD_OVERLAY_LAYER`
/// if set, else next to the running executable (`npm run tauri dev`), else in the bundle's
/// `../lib/mumcord` (AppImage and deb, see `src-tauri/tauri.linux.conf.json`).
#[cfg(target_os = "linux")]
pub fn library_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let override_path = std::env::var_os("MUMCORD_OVERLAY_LAYER").map(std::path::PathBuf::from);
    find_library(override_path, exe.parent()?)
}

#[cfg(target_os = "linux")]
fn find_library(
    override_path: Option<std::path::PathBuf>,
    exe_dir: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let candidates = match override_path {
        Some(path) => vec![path],
        None => vec![
            exe_dir.join(LIBRARY_NAME),
            exe_dir.join("../lib/mumcord").join(LIBRARY_NAME),
        ],
    };
    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// Installs or removes the Vulkan layer manifest to match `mode` (Linux). The manifest is present
/// only while the overlay is globally on, so "off" means nothing is loaded into any game.
#[cfg(target_os = "linux")]
pub fn sync_manifest(mode: InGameOverlayMode) {
    let Some(dir) = manifest::layer_dir() else {
        tracing::warn!("no data directory for the Vulkan layer manifest");
        return;
    };
    let result = match mode {
        InGameOverlayMode::On => match library_path() {
            Some(lib) => manifest::install_into(&dir, &lib).map(|_| ()),
            None => {
                tracing::warn!("the in-game overlay library was not found; no layer installed");
                Ok(())
            }
        },
        InGameOverlayMode::Off => manifest::uninstall_from(&dir),
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, "updating the Vulkan layer manifest failed");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn sync_manifest(_mode: InGameOverlayMode) {}

/// Sets up the runtime: starts the IPC server and applies the stored settings. Any failure only
/// means no in-game overlay; the app runs regardless.
pub fn init(shared: Arc<Shared>, ingame: InGameOverlaySettings) -> Runtime {
    shared.set_settings(ingame.clone());
    sync_manifest(ingame.mode);
    #[cfg(target_os = "linux")]
    let server = if SUPPORTED {
        match server::start(shared.clone()) {
            Ok(server) => Some(server),
            Err(e) => {
                tracing::warn!(error = %e, "starting the in-game overlay server failed");
                None
            }
        }
    } else {
        None
    };
    Runtime {
        shared,
        #[cfg(target_os = "linux")]
        _server: Mutex::new(server),
    }
}

/// Applies changed settings: updates what the server answers and the installed manifest.
pub fn apply(runtime: &Runtime, settings: &Settings) {
    runtime.shared.set_settings(settings.ingame_overlay.clone());
    sync_manifest(settings.ingame_overlay.mode);
}

/// Updates the overlay scene from an engine event. Talking changes update the set and rebuild;
/// membership changes (sync, join/leave) rebuild from a fresh snapshot.
pub fn on_engine_event(runtime: &Runtime, engine: &Engine, event: &mumcord_engine::EngineEvent) {
    use mumcord_engine::EngineEvent as E;
    let shared = &runtime.shared;
    match event {
        E::TalkStateChanged { session, talking } => {
            if shared.set_talking(*session, *talking) {
                shared.rebuild(engine);
            }
        }
        E::ServerSynced(_)
        | E::UserUpserted(_)
        | E::UserRemoved(_)
        | E::ChannelUpserted(_)
        | E::ChannelsUpserted(_)
        | E::ChannelRemoved(_) => shared.rebuild(engine),
        E::StateChanged(mumcord_engine::ConnState::Disconnected { .. }) => {
            shared.clear_talking();
            shared.rebuild(engine);
        }
        _ => {}
    }
}

fn to_wire_corner(corner: mumcord_engine::store::OverlayCorner) -> Corner {
    use mumcord_engine::store::OverlayCorner as C;
    match corner {
        C::TopLeft => Corner::TopLeft,
        C::TopRight => Corner::TopRight,
        C::BottomLeft => Corner::BottomLeft,
        C::BottomRight => Corner::BottomRight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_overlay_ipc::wire::Person;

    #[cfg(target_os = "linux")]
    #[test]
    fn library_is_found_next_to_the_exe_or_in_the_bundle_lib_dir() {
        let root = tempfile::tempdir().expect("temp dir");
        let bin = root.path().join("usr/bin");
        let bundle = root.path().join("usr/lib/mumcord");
        std::fs::create_dir_all(&bin).expect("bin dir");
        std::fs::create_dir_all(&bundle).expect("lib dir");
        assert_eq!(find_library(None, &bin), None);

        std::fs::write(bundle.join(LIBRARY_NAME), b"").expect("bundled lib");
        assert_eq!(
            find_library(None, &bin),
            Some(bin.join("../lib/mumcord").join(LIBRARY_NAME))
        );

        std::fs::write(bin.join(LIBRARY_NAME), b"").expect("dev lib");
        assert_eq!(find_library(None, &bin), Some(bin.join(LIBRARY_NAME)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn library_override_must_exist() {
        let dir = tempfile::tempdir().expect("temp dir");
        let custom = dir.path().join("custom.so");
        assert_eq!(find_library(Some(custom.clone()), dir.path()), None);
        std::fs::write(&custom, b"").expect("custom lib");
        assert_eq!(find_library(Some(custom.clone()), dir.path()), Some(custom));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_library_is_explained() {
        let missing = support_with(None);
        assert!(!missing.supported);
        assert_eq!(
            missing.explanation_key,
            Some("overlay.inGame.limits.libraryMissing")
        );
        let found = support_with(Some(std::path::PathBuf::from("/x.so")));
        assert!(found.supported);
        assert_eq!(found.explanation_key, None);
    }

    fn person(name: &str, talking: bool) -> Person {
        Person {
            name: name.into(),
            talking,
            muted: false,
            deafened: false,
            is_self: false,
        }
    }

    #[test]
    fn key_prefers_the_steam_id() {
        let hello = Hello {
            version: 1,
            api: mumcord_overlay_ipc::Api::Vulkan,
            pid: 1,
            steam_app_id: Some(730),
            exe: "cs2".into(),
        };
        assert_eq!(key_of(&hello), GameKey::Steam(730));
        let hello = Hello {
            steam_app_id: None,
            ..hello
        };
        assert_eq!(key_of(&hello), GameKey::Exe("cs2".into()));
    }

    #[test]
    fn config_follows_the_decision_and_per_game_look() {
        let shared = Shared::default();
        {
            let mut state = shared.inner.lock().expect("lock");
            state.ingame.mode = InGameOverlayMode::On;
            state.ingame.games.push(GameOverlayEntry {
                key: GameKey::Exe("mygame.exe".into()),
                opacity_percent: 55,
                show_whole_channel: true,
                ..GameOverlayEntry::default()
            });
        }
        let allowed = shared.config_for(&GameKey::Exe("MyGame.exe".into()));
        assert!(allowed.enabled && allowed.opacity_percent == 55 && allowed.show_whole_channel);
        // A blocklisted game (CS2) is disabled despite the global On.
        assert_eq!(shared.config_for(&GameKey::Steam(730)), Config::DISABLED);
    }

    #[test]
    fn scene_is_filtered_per_game() {
        let shared = Shared::default();
        {
            let mut state = shared.inner.lock().expect("lock");
            state.scene = Scene {
                channel: "Lobby".into(),
                people: vec![person("Ana", true), person("Bob", false)],
            };
        }
        let entry_default = GameKey::Exe("x".into());
        let only_talkers = shared.scene_for(&entry_default);
        assert_eq!(only_talkers.people.len(), 1);
        assert_eq!(only_talkers.channel, "Lobby");
    }
}
