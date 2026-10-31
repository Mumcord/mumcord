//! Audio profiles per channel (issue #171, FR-3.62–FR-3.64). A profile overrides Music quality,
//! low-latency mode and the bitrate cap while the user is in its channel on the server that
//! carries voice. The global settings stay stored as they are: the profile is a layer on top,
//! so leaving the channel (or removing the profile) brings the global values back. The server's
//! bandwidth limit and the per-packet limit still win, as the bitrate controller applies them
//! after the user cap.

use mumcord_store::{ChannelAudioProfile, ServerId, Settings, resolve_channel_audio};

use super::convert::{channel_path, join_path};
use super::{
    Core, Engine, EngineError, EngineEvent, EngineResult, Inner, local, lock, store_error,
};

/// Lowest bitrate cap a profile may hold (the slider's minimum, FR-2.64).
const BITRATE_MIN_KBPS: u32 = 16;
/// Opus' own limit.
const BITRATE_MAX_KBPS: u32 = 510;

/// The profile in effect: the one of the own channel on the server with the voice pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveAudioProfile {
    pub server_id: ServerId,
    pub channel_id: u32,
    pub profile: ChannelAudioProfile,
}

/// A channel of a connected server that has a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelAudioView {
    pub channel_id: u32,
    pub profile: ChannelAudioProfile,
}

/// The settings the audio runs with: every value the profile sets wins over the global one
/// (channel profile > global setting, FR-3.63).
pub(crate) fn effective_settings(
    global: &Settings,
    profile: Option<&ChannelAudioProfile>,
) -> Settings {
    let mut settings = global.clone();
    if let Some(profile) = profile {
        if let Some(on) = profile.music_quality {
            settings.music_quality = on;
        }
        if let Some(on) = profile.low_latency {
            settings.low_latency = on;
        }
        if let Some(kbps) = profile.bitrate_kbps {
            settings.bitrate_kbps = Some(kbps);
        }
    }
    settings
}

impl Core {
    /// Switches low-latency mode live, without reconnecting (issue #156): the capture thread
    /// follows the DSP controls; devices and the receive side are switched here.
    pub(super) fn set_low_latency(&mut self, on: bool) {
        if let Some(audio) = &self.audio {
            audio.set_low_latency(on);
        }
        if let Some(pipeline) = self.pipeline.as_mut() {
            pipeline.set_low_latency(on);
        }
        // Back to the safe queue at once; a lower one follows once the new device period is
        // measured.
        self.sync_queue_target(on);
    }
}

impl Inner {
    /// Applies the profile of the channel the user is in now, or the global settings when it
    /// has none. Called whenever the own channel, the voice server or a profile may have changed;
    /// does nothing while the profile in effect stays the same.
    pub(super) fn sync_channel_audio(&self, core: &mut Core) {
        let target = self.channel_audio_target(core);
        if target == core.channel_audio {
            return;
        }
        let global = match lock(&self.store).settings() {
            Ok(settings) => settings,
            Err(e) => {
                tracing::warn!(error = %e, "reading the settings for a channel audio profile failed");
                return;
            }
        };
        let before = effective_settings(&global, core.channel_audio.as_ref().map(|a| &a.profile));
        let after = effective_settings(&global, target.as_ref().map(|a| &a.profile));
        core.channel_audio = target;
        local::apply_dsp(&self.controls, &after);
        if before.low_latency != after.low_latency {
            core.set_low_latency(after.low_latency);
        }
        self.events
            .emit(EngineEvent::ActiveAudioProfileChanged(target));
    }

    fn channel_audio_target(&self, core: &Core) -> Option<ActiveAudioProfile> {
        let server_id = core.audio_server?;
        let session = core.session(server_id).ok()?;
        let (channel_id, path) = session.handle.with_state(|s| {
            let own = s.users.get(&s.session?)?;
            Some((own.channel_id, channel_path(s, own.channel_id)?))
        })?;
        let profile = lock(&self.store)
            .channel_audio_profile(server_id, channel_id, &join_path(&path))
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "reading a channel audio profile failed");
                None
            })?;
        Some(ActiveAudioProfile {
            server_id,
            channel_id,
            profile,
        })
    }
}

impl Engine {
    /// The channels of a connected server that have an audio profile, matched by id and path.
    pub fn channel_audio_profiles(
        &self,
        server_id: ServerId,
    ) -> EngineResult<Vec<ChannelAudioView>> {
        let ctx = self.session_ctx_for(server_id)?;
        let saved = lock(&self.inner.store)
            .channel_audio_profiles(server_id)
            .map_err(store_error)?;
        if saved.is_empty() {
            return Ok(Vec::new());
        }
        let mut views: Vec<ChannelAudioView> = ctx.handle.with_state(|s| {
            s.channels
                .keys()
                .filter_map(|&id| {
                    let path = join_path(&channel_path(s, id)?);
                    resolve_channel_audio(&saved, id, &path).map(|found| ChannelAudioView {
                        channel_id: id,
                        profile: found.profile,
                    })
                })
                .collect()
        });
        views.sort_by_key(|v| v.channel_id);
        Ok(views)
    }

    /// Saves (`Some`) or removes (`None`, "Remove profile") the audio profile of a channel. The
    /// change applies at once when the user is in that channel.
    pub fn channel_audio_profile_set(
        &self,
        server_id: ServerId,
        channel_id: u32,
        profile: Option<ChannelAudioProfile>,
    ) -> EngineResult<()> {
        if let Some(kbps) = profile.and_then(|p| p.bitrate_kbps)
            && !(BITRATE_MIN_KBPS..=BITRATE_MAX_KBPS).contains(&kbps)
        {
            return Err(EngineError::invalid(format!(
                "bitrate must be {BITRATE_MIN_KBPS}–{BITRATE_MAX_KBPS} kbit/s"
            )));
        }
        let ctx = self.session_ctx_for(server_id)?;
        let path = ctx
            .handle
            .with_state(|s| channel_path(s, channel_id))
            .ok_or_else(|| EngineError::invalid("unknown channel"))?;
        let path = join_path(&path);
        {
            let store = lock(&self.inner.store);
            match &profile {
                Some(profile) => {
                    store.set_channel_audio_profile(server_id, channel_id, &path, profile)
                }
                None => store.remove_channel_audio_profile(server_id, channel_id, &path),
            }
            .map_err(store_error)?;
        }
        ctx.events.emit(EngineEvent::ChannelAudioProfilesChanged(
            self.channel_audio_profiles(server_id)?,
        ));
        self.inner.sync_channel_audio(&mut lock(&self.inner.core));
        Ok(())
    }

    /// The channel profile in effect now, if any.
    pub fn active_audio_profile(&self) -> Option<ActiveAudioProfile> {
        lock(&self.inner.core).channel_audio
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_audio::dsp::DspControls;

    const MUSIC: ChannelAudioProfile = ChannelAudioProfile {
        music_quality: Some(true),
        low_latency: None,
        bitrate_kbps: Some(256),
    };

    fn global() -> Settings {
        Settings {
            music_quality: false,
            low_latency: false,
            bitrate_kbps: Some(96),
            ..Settings::default()
        }
    }

    #[test]
    fn the_profile_wins_and_default_keeps_the_global_value() {
        let jam = ChannelAudioProfile {
            music_quality: Some(false),
            low_latency: Some(true),
            bitrate_kbps: None,
        };
        let global = Settings {
            music_quality: true,
            ..global()
        };
        let effective = effective_settings(&global, Some(&jam));
        assert!(!effective.music_quality);
        assert!(effective.low_latency);
        assert_eq!(effective.bitrate_kbps, Some(96), "default keeps the cap");

        let effective = effective_settings(&global, Some(&MUSIC));
        assert!(effective.music_quality);
        assert!(!effective.low_latency, "default keeps the global switch");
        assert_eq!(effective.bitrate_kbps, Some(256));
    }

    #[test]
    fn a_profile_only_touches_its_three_values() {
        let global = Settings {
            noise_suppression: false,
            input_gain_percent: 140,
            ..global()
        };
        let effective = effective_settings(&global, Some(&MUSIC));
        assert_eq!(
            Settings {
                music_quality: global.music_quality,
                bitrate_kbps: global.bitrate_kbps,
                ..effective
            },
            global
        );
        assert_eq!(effective_settings(&global, None), global);
        assert_eq!(
            effective_settings(&global, Some(&ChannelAudioProfile::default())),
            global,
            "an all-default profile changes nothing"
        );
    }

    #[test]
    fn joining_applies_and_leaving_restores_the_global_controls() {
        let controls = DspControls::default();
        let global = global();
        local::apply_dsp(&controls, &effective_settings(&global, None));
        assert!(!controls.music_quality());
        assert_eq!(controls.bitrate_cap_kbps(), Some(96));

        local::apply_dsp(&controls, &effective_settings(&global, Some(&MUSIC)));
        assert!(controls.music_quality());
        assert_eq!(controls.bitrate_cap_kbps(), Some(256));

        local::apply_dsp(&controls, &effective_settings(&global, None));
        assert!(!controls.music_quality());
        assert_eq!(controls.bitrate_cap_kbps(), Some(96));
    }
}
