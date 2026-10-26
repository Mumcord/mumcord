//! In-game overlay (FR-1.46b), app side: the local IPC server games connect to, the Vulkan
//! layer manifest install, and the scene the overlay shows.
//!
//! Platforms:
//! - **Linux:** a per-user abstract Unix-socket server; games (also inside the Steam Linux
//!   Runtime container) reach it through the Vulkan layer or the OpenGL preload
//!   (`mumcord-overlay-layer`). Turning the overlay on installs the layer manifest, off removes
//!   it.
//! - **Windows:** a separate click-through window over the game in the foreground
//!   (`windows_window`, FR-2.28); nothing is loaded into games, so the anti-cheat blocklist does
//!   not apply there ([`BLOCKLIST_APPLIES`]).
//! - **macOS:** no overlay at all (QA.58); [`SUPPORTED`] is false.

#[cfg(any(target_os = "windows", test))]
mod follow;
#[cfg(target_os = "linux")]
mod manifest;
mod scene;
#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "windows")]
mod windows_window;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use mumcord_engine::store::{
    GameKey, GameOverlayEntry, InGameOverlayMode, InGameOverlaySettings, Settings,
};
use mumcord_engine::{Engine, OverlayDecision};
#[cfg(any(target_os = "linux", test))]
use mumcord_overlay_ipc::wire::Hello;
use mumcord_overlay_ipc::wire::Scene;
#[cfg(any(target_os = "linux", target_os = "windows", test))]
use mumcord_overlay_ipc::wire::{Config, Corner};

/// Whether the IPC server for the in-game layer runs: Linux only (Windows uses the overlay
/// window instead; macOS has no overlay, QA.58).
#[cfg(target_os = "linux")]
pub const SUPPORTED: bool = cfg!(target_os = "linux");

/// Whether the bundled anti-cheat blocklist (QA.56) applies. Not on Windows: the overlay is a
/// separate window there and never touches the game (FR-2.28).
pub const BLOCKLIST_APPLIES: bool = !cfg!(target_os = "windows");

/// Whether the overlay is drawn for `key` on this platform: the engine's decision, but without
/// the blocklist where it does not apply.
pub fn decide(settings: &InGameOverlaySettings, key: &GameKey) -> OverlayDecision {
    if BLOCKLIST_APPLIES {
        return mumcord_engine::decide_overlay(settings, key);
    }
    if settings.mode == InGameOverlayMode::Off {
        return OverlayDecision::GlobalOff;
    }
    match settings.entry(key).and_then(|e| e.enabled) {
        Some(false) => OverlayDecision::GameOff,
        Some(true) | None => OverlayDecision::Show,
    }
}

/// The game-side identity reduced to how settings and the blocklist key a game.
#[cfg(any(target_os = "linux", test))]
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

    /// The current in-game overlay settings.
    #[cfg(target_os = "windows")]
    pub fn ingame_settings(&self) -> InGameOverlaySettings {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ingame
            .clone()
    }

    /// Records a game that connected (deduplicated by key, most recent name kept).
    #[cfg(any(target_os = "linux", target_os = "windows"))]
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
    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub fn config_for(&self, key: &GameKey) -> Config {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let decision = decide(&state.ingame, key);
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
    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    pub fn scene_for(&self, key: &GameKey) -> Scene {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = state.ingame.entry(key).cloned().unwrap_or_default();
        filter_scene(&state.scene, &entry)
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
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
    /// i18n key (namespace `settings`) explaining a limitation, or `None`. Shown as a hint when
    /// the overlay is supported.
    pub explanation_key: Option<&'static str>,
}

/// Platform support (FR-1.46b, QA.58).
#[cfg(target_os = "linux")]
pub fn support() -> Support {
    support_with(library_path())
}

/// Platform support: the overlay window works for every game, but cannot cover one in exclusive
/// fullscreen, so the settings carry that hint (FR-2.28).
#[cfg(target_os = "windows")]
pub fn support() -> Support {
    Support {
        supported: true,
        explanation_key: Some("overlay.inGame.limits.windowsFullscreen"),
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

/// App-wide runtime, managed by Tauri. Holds the shared state the IPC server (Linux) or the
/// overlay window (Windows) reads and keeps it alive while the app runs.
pub struct Runtime {
    pub shared: Arc<Shared>,
    #[cfg(target_os = "linux")]
    _server: Mutex<Option<server::Server>>,
    #[cfg(target_os = "windows")]
    window: Option<windows_window::Overlay>,
}

impl Runtime {
    /// Tells the overlay window to redraw (the IPC server instead pushes scenes on its own).
    fn scene_changed(&self) {
        #[cfg(target_os = "windows")]
        if let Some(window) = &self.window {
            window.scene_changed();
        }
    }
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

/// The Steam launch option that loads the layer into a native OpenGL game (FR-2.57), or `None`
/// when there is no layer library. It names the stable `current` path, which `sync_manifest` keeps
/// pointing at the installed version, never the AppImage mount.
#[cfg(target_os = "linux")]
pub fn opengl_launch_option() -> Option<String> {
    let name = library_path()?.file_name()?.to_owned();
    let path = manifest::current_library_path(&manifest::layer_dir()?, &name);
    Some(launch_option_for(&path))
}

/// `LD_PRELOAD=<path> %command%`; the path is quoted when it contains spaces, as Steam hands the
/// option to a shell.
#[cfg(target_os = "linux")]
fn launch_option_for(path: &std::path::Path) -> String {
    let path = path.to_string_lossy();
    if path.contains(char::is_whitespace) {
        format!("LD_PRELOAD=\"{path}\" %command%")
    } else {
        format!("LD_PRELOAD={path} %command%")
    }
}

#[cfg(not(target_os = "linux"))]
pub fn opengl_launch_option() -> Option<String> {
    None
}

/// Installs or removes the Vulkan layer manifest to match `mode` (Linux). The manifest is present
/// only while the overlay is globally on, so "off" means nothing is loaded into any game.
#[cfg(target_os = "linux")]
pub fn sync_manifest(mode: InGameOverlayMode) {
    let dirs = manifest::layer_dirs();
    if dirs.is_empty() {
        tracing::warn!("no data directory for the Vulkan layer manifest");
        return;
    }
    let lib = match mode {
        InGameOverlayMode::On => {
            let lib = library_path();
            if lib.is_none() {
                tracing::warn!("the in-game overlay library was not found; no layer installed");
            }
            lib
        }
        InGameOverlayMode::Off => None,
    };
    // Native and Flatpak Steam folders are independent: one failing must not skip the other.
    for dir in dirs {
        if let Err(e) = manifest::refresh_in(&dir, lib.as_deref()) {
            tracing::warn!(error = %e, "updating the Vulkan layer manifest failed");
        }
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
    #[cfg(target_os = "windows")]
    let window = windows_window::start(shared.clone());
    Runtime {
        shared,
        #[cfg(target_os = "linux")]
        _server: Mutex::new(server),
        #[cfg(target_os = "windows")]
        window,
    }
}

/// Applies changed settings: updates what the server answers, the installed manifest and the
/// overlay window.
pub fn apply(runtime: &Runtime, settings: &Settings) {
    runtime.shared.set_settings(settings.ingame_overlay.clone());
    sync_manifest(settings.ingame_overlay.mode);
    #[cfg(target_os = "windows")]
    if let Some(window) = &runtime.window {
        window.settings_changed();
    }
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
                runtime.scene_changed();
            }
        }
        E::ServerSynced(_)
        | E::UserUpserted(_)
        | E::UserRemoved(_)
        | E::ChannelUpserted(_)
        | E::ChannelsUpserted(_)
        | E::ChannelRemoved(_) => {
            shared.rebuild(engine);
            runtime.scene_changed();
        }
        E::StateChanged(mumcord_engine::ConnState::Disconnected { .. }) => {
            shared.clear_talking();
            shared.rebuild(engine);
            runtime.scene_changed();
        }
        _ => {}
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
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
    #[cfg(target_os = "linux")]
    #[test]
    fn launch_option_quotes_paths_with_spaces() {
        assert_eq!(
            launch_option_for(std::path::Path::new("/a/b.so")),
            "LD_PRELOAD=/a/b.so %command%"
        );
        assert_eq!(
            launch_option_for(std::path::Path::new("/a b/c.so")),
            "LD_PRELOAD=\"/a b/c.so\" %command%"
        );
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
        // A blocklisted game (CS2) is disabled despite the global On, where the blocklist applies.
        let blocklisted = shared.config_for(&GameKey::Steam(730));
        assert_eq!(blocklisted.enabled, !BLOCKLIST_APPLIES);
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
