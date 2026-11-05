//! `audio` (since 1.1, FR-9.65 … FR-9.67): short sounds bundled in the plugin package. Needs
//! `audio.effects`.
//!
//! The host checks the call (permission, declared file, rate) and hands it to a
//! [`SoundPlayer`], which the client glues to the engine's plugin-sound mixer. The player reads
//! and decodes the file on its own worker thread, so a call returns at once. Without a player
//! (`mumcord-plugin test`, no audio) `play` answers `not-permitted`.

use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

use crate::api::v1_1::wit::types::Error;
use crate::manifest::Manifest;

pub const PERMISSION: &str = "audio.effects";
/// At most this many `play` calls per plugin within [`RATE_WINDOW`]; more answer
/// `rate-limited`. Ten short effects a second is plenty for game feedback and keeps a runaway
/// loop from hammering the decoder.
pub const MAX_PLAYS: usize = 10;
pub const RATE_WINDOW: Duration = Duration::from_secs(1);

/// Plays checked sound requests. Runs on the plugin's thread inside its call budget, so it
/// must only queue the work.
pub trait SoundPlayer: Send + Sync + Debug {
    /// Plays `file` (an existing path inside the plugin's package) at `gain` (0.0 … 1.0).
    fn play(&self, plugin: &str, file: PathBuf, gain: f32);
    /// Stops every sound of `plugin`.
    fn stop_all(&self, plugin: &str);
    /// The plugin was stopped or removed: stop its sounds and drop what is kept for it.
    fn forget(&self, plugin: &str);
}

/// The declared sounds of each loaded plugin and the player; lives in
/// [`crate::HostContext`].
#[derive(Debug, Default)]
pub struct Audio {
    player: RwLock<Option<Arc<dyn SoundPlayer>>>,
    plugins: Mutex<HashMap<String, PluginSounds>>,
}

#[derive(Debug, Default)]
struct PluginSounds {
    /// The package folder.
    dir: PathBuf,
    /// The manifest's `audio.sounds`, such as `sounds/win.ogg`.
    declared: Vec<String>,
    /// When the recent `play` calls were made, oldest first.
    recent: VecDeque<Instant>,
}

impl Audio {
    /// Sets where sounds play (the client's engine).
    pub fn set_player(&self, player: Arc<dyn SoundPlayer>) {
        *self.player.write().unwrap_or_else(PoisonError::into_inner) = Some(player);
    }

    fn player(&self) -> Option<Arc<dyn SoundPlayer>> {
        self.player
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn plugins(&self) -> MutexGuard<'_, HashMap<String, PluginSounds>> {
        self.plugins.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A plugin was loaded from `dir`: its manifest's declared sounds become playable.
    pub fn set_sounds(&self, plugin: &str, dir: &Path, manifest: Option<&Manifest>) {
        let declared = manifest
            .and_then(|m| m.audio.as_ref())
            .map(|a| a.sounds.clone())
            .unwrap_or_default();
        let mut plugins = self.plugins();
        let entry = plugins.entry(plugin.to_owned()).or_default();
        entry.dir = dir.to_owned();
        entry.declared = declared;
        drop(plugins);
        // An update may have replaced the files: decoded copies of the old ones go.
        if let Some(player) = self.player() {
            player.forget(plugin);
        }
    }

    /// The plugin stopped or was removed: its sounds stop.
    pub fn forget(&self, plugin: &str) {
        self.plugins().remove(plugin);
        if let Some(player) = self.player() {
            player.forget(plugin);
        }
    }

    /// `audio.play` after the permission check: `not-found` for a sound the manifest does not
    /// declare, `rate-limited` past [`MAX_PLAYS`] per [`RATE_WINDOW`], `invalid` for a gain
    /// that is not a number.
    pub fn play(
        &self,
        plugin: &str,
        sound: &str,
        gain: Option<f32>,
        now: Instant,
    ) -> Result<(), Error> {
        let gain = gain.unwrap_or(1.0);
        if !gain.is_finite() {
            return Err(Error::Invalid);
        }
        let file = {
            let mut plugins = self.plugins();
            let entry = plugins.get_mut(plugin).ok_or(Error::NotFound)?;
            if !entry.declared.iter().any(|d| d == sound) {
                return Err(Error::NotFound);
            }
            while entry
                .recent
                .front()
                .is_some_and(|t| now.duration_since(*t) >= RATE_WINDOW)
            {
                entry.recent.pop_front();
            }
            if entry.recent.len() >= MAX_PLAYS {
                return Err(Error::RateLimited);
            }
            entry.recent.push_back(now);
            // The manifest check (FR-9.65) allows only plain paths below `sounds/`.
            entry.dir.join(sound)
        };
        let player = self.player().ok_or(Error::NotPermitted)?;
        player.play(plugin, file, gain.clamp(0.0, 1.0));
        Ok(())
    }

    /// `audio.stop-all`.
    pub fn stop_all(&self, plugin: &str) {
        if let Some(player) = self.player() {
            player.stop_all(plugin);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl SoundPlayer for Recorder {
        fn play(&self, plugin: &str, file: PathBuf, gain: f32) {
            self.0
                .lock()
                .unwrap()
                .push(format!("play {plugin} {} {gain}", file.display()));
        }
        fn stop_all(&self, plugin: &str) {
            self.0.lock().unwrap().push(format!("stop {plugin}"));
        }
        fn forget(&self, plugin: &str) {
            self.0.lock().unwrap().push(format!("forget {plugin}"));
        }
    }

    fn manifest(sounds: &[&str]) -> Manifest {
        let mut m = Manifest::parse(crate::manifest::tests::EXAMPLE.as_bytes()).unwrap();
        m.audio = Some(crate::manifest::AudioDecl {
            sounds: sounds.iter().map(|s| (*s).to_owned()).collect(),
        });
        m
    }

    fn audio(sounds: &[&str]) -> (Audio, Arc<Recorder>) {
        let audio = Audio::default();
        let player = Arc::new(Recorder::default());
        audio.set_player(player.clone());
        audio.set_sounds("p", Path::new("/pkg"), Some(&manifest(sounds)));
        player.0.lock().unwrap().clear();
        (audio, player)
    }

    #[test]
    fn plays_a_declared_sound_from_the_package() {
        let (audio, player) = audio(&["sounds/win.ogg"]);
        audio
            .play("p", "sounds/win.ogg", Some(0.5), Instant::now())
            .unwrap();
        audio
            .play("p", "sounds/win.ogg", Some(7.0), Instant::now())
            .unwrap();
        audio.stop_all("p");
        let calls = player.0.lock().unwrap().clone();
        let file = Path::new("/pkg").join("sounds/win.ogg");
        assert_eq!(
            calls,
            [
                format!("play p {} 0.5", file.display()),
                format!("play p {} 1", file.display()),
                "stop p".to_owned(),
            ]
        );
    }

    #[test]
    fn undeclared_or_unknown_sounds_are_not_found() {
        let (audio, player) = audio(&["sounds/win.ogg"]);
        for sound in ["sounds/lose.ogg", "manifest.json", "../sounds/win.ogg", ""] {
            assert_eq!(
                audio.play("p", sound, None, Instant::now()),
                Err(Error::NotFound)
            );
        }
        assert_eq!(
            audio.play("other", "sounds/win.ogg", None, Instant::now()),
            Err(Error::NotFound)
        );
        assert!(player.0.lock().unwrap().is_empty());
    }

    #[test]
    fn rate_limit_per_plugin() {
        let (audio, _player) = audio(&["sounds/a.wav"]);
        let start = Instant::now();
        for _ in 0..MAX_PLAYS {
            audio.play("p", "sounds/a.wav", None, start).unwrap();
        }
        assert_eq!(
            audio.play("p", "sounds/a.wav", None, start),
            Err(Error::RateLimited)
        );
        assert!(
            audio
                .play("p", "sounds/a.wav", None, start + RATE_WINDOW)
                .is_ok()
        );
    }

    #[test]
    fn nan_gain_is_invalid_and_no_player_is_not_permitted() {
        let (audio, _player) = audio(&["sounds/a.wav"]);
        assert_eq!(
            audio.play("p", "sounds/a.wav", Some(f32::NAN), Instant::now()),
            Err(Error::Invalid)
        );
        let bare = Audio::default();
        bare.set_sounds("p", Path::new("/pkg"), Some(&manifest(&["sounds/a.wav"])));
        assert_eq!(
            bare.play("p", "sounds/a.wav", None, Instant::now()),
            Err(Error::NotPermitted)
        );
    }

    #[test]
    fn forget_stops_the_plugin_and_drops_its_sounds() {
        let (audio, player) = audio(&["sounds/a.wav"]);
        audio.forget("p");
        assert_eq!(*player.0.lock().unwrap(), ["forget p"]);
        assert_eq!(
            audio.play("p", "sounds/a.wav", None, Instant::now()),
            Err(Error::NotFound)
        );
    }
}
