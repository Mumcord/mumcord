//! Glue between the plugin host's `audio` API and the engine's plugin-sound mixer (FR-9.65 …
//! FR-9.68).

use std::path::PathBuf;
use std::sync::Arc;

use mumcord_engine::plugin_sounds::PluginSounds;
use mumcord_engine::sounds::SoundHub;
use mumcord_plugins::api::audio::{Audio, SoundPlayer};

/// Hands checked play requests to the engine; the hub owns volume, mute and the meter.
pub struct EngineSoundPlayer {
    hub: Arc<SoundHub>,
}

impl EngineSoundPlayer {
    pub fn new(hub: Arc<SoundHub>) -> Self {
        Self { hub }
    }

    fn sounds(&self) -> &PluginSounds {
        self.hub.plugin_sounds()
    }
}

impl std::fmt::Debug for EngineSoundPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineSoundPlayer").finish_non_exhaustive()
    }
}

impl SoundPlayer for EngineSoundPlayer {
    fn play(&self, plugin: &str, file: PathBuf, gain: f32) {
        self.sounds().play(plugin, file, gain);
    }

    fn stop_all(&self, plugin: &str) {
        self.sounds().stop_all(plugin);
    }

    fn forget(&self, plugin: &str) {
        self.sounds().forget(plugin);
    }
}

/// Lets plugins play sounds through `hub`.
pub fn register(audio: &Audio, hub: Arc<SoundHub>) {
    audio.set_player(Arc::new(EngineSoundPlayer::new(hub)));
}

/// Applies the stored plugin-sound volume (percent) and mute to the mixer.
pub fn apply_settings(hub: &SoundHub, volume: u8, muted: bool) {
    let sounds = hub.plugin_sounds();
    sounds.set_volume(volume);
    sounds.set_muted(muted);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use mumcord_engine::store::SoundSettings;
    use mumcord_plugins::manifest::{AudioDecl, Manifest};

    use super::*;

    const MANIFEST: &str = r#"{
      "manifest": 1, "id": "chime", "name": "Chime", "description": "Plays a chime.",
      "version": "1.0.0", "api": "^1.0", "mumcord": ">=0.7.0", "authors": ["Test"],
      "license": "AGPL-3.0-only", "categories": ["fun"], "icon": "assets/icon.svg",
      "permissions": { "required": ["ui.panel"] }
    }"#;

    fn hub() -> Arc<SoundHub> {
        SoundHub::new(SoundSettings::default)
    }

    #[test]
    fn play_is_refused_until_the_player_is_registered() {
        let audio = Audio::default();
        let mut manifest = Manifest::parse(MANIFEST.as_bytes()).unwrap();
        manifest.audio = Some(AudioDecl {
            sounds: vec!["sounds/win.ogg".to_owned()],
        });
        audio.set_sounds("p", Path::new("/pkg"), Some(&manifest));
        assert!(
            audio
                .play("p", "sounds/win.ogg", None, Instant::now())
                .is_err()
        );
        register(&audio, hub());
        assert!(
            audio
                .play("p", "sounds/win.ogg", None, Instant::now())
                .is_ok()
        );
    }

    #[test]
    fn settings_reach_the_mixer() {
        let hub = hub();
        apply_settings(&hub, 35, true);
        assert_eq!(hub.plugin_sounds().volume(), 35);
        assert!(hub.plugin_sounds().muted());
        apply_settings(&hub, 100, false);
        assert_eq!(hub.plugin_sounds().volume(), 100);
        assert!(!hub.plugin_sounds().muted());
    }
}
