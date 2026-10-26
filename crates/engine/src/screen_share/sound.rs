//! The viewer's sound path (FR-2.42): the Opus packets of a watched share are decoded on its
//! reader task and played as their own source of the playback mix
//! ([`mumcord_audio::mixer::share`]).
//!
//! The playback thread comes and goes with the voice pipeline, so watches do not hold on to it:
//! the pipeline attaches its share port to [`ShareSoundHub`] while it runs, and each watch opens
//! a new ring whenever the hub's generation changed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mumcord_audio::mixer::share::{SharePort, ShareSoundWriter, ShareStats, share_channel};

use crate::session::lock;
use crate::voice::VoiceShared;

/// Where watched shares' sound goes: the share port of the running playback thread, if any.
#[derive(Default)]
pub(crate) struct ShareSoundHub {
    port: Mutex<Option<SharePort>>,
    /// Changes whenever the port does; watches compare it on every packet.
    generation: AtomicU64,
}

impl std::fmt::Debug for ShareSoundHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShareSoundHub")
            .field("generation", &self.generation())
            .finish_non_exhaustive()
    }
}

impl ShareSoundHub {
    pub fn attach(&self, port: SharePort) {
        let mut slot = lock(&self.port);
        *slot = Some(port);
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn detach(&self) {
        let mut slot = lock(&self.port);
        *slot = None;
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// A new ring into the current playback thread, with the generation it belongs to.
    fn open(&self, stats: &Arc<ShareStats>) -> (u64, Option<ShareSoundWriter>) {
        let mut slot = lock(&self.port);
        let generation = self.generation();
        let Some(port) = slot.as_mut() else {
            return (generation, None);
        };
        let (writer, source) = match share_channel(stats.clone()) {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "screen share sound decoder did not start");
                return (generation, None);
            }
        };
        match port.add(source) {
            Ok(()) => (generation, Some(writer)),
            Err(_) => {
                tracing::warn!("too many screen shares with sound; this one stays silent");
                (generation, None)
            }
        }
    }
}

/// One watched share's sound, fed from its reader task.
pub(crate) struct ShareSound {
    voice: Arc<VoiceShared>,
    stats: Arc<ShareStats>,
    writer: Option<(u64, Option<ShareSoundWriter>)>,
}

impl ShareSound {
    pub fn new(voice: Arc<VoiceShared>) -> Self {
        Self {
            voice,
            stats: Arc::new(ShareStats::default()),
            writer: None,
        }
    }

    pub fn stats(&self) -> Arc<ShareStats> {
        self.stats.clone()
    }

    /// Plays one Opus packet of the share.
    pub fn packet(&mut self, payload: &[u8]) {
        // Deafened hears nothing, shares included.
        if self.voice.deaf.load(Ordering::Relaxed) {
            return;
        }
        let hub = &self.voice.share_sound;
        if self
            .writer
            .as_ref()
            .is_none_or(|(generation, _)| *generation != hub.generation())
        {
            self.writer = Some(hub.open(&self.stats));
        }
        if let Some((_, Some(writer))) = &mut self.writer
            && let Err(e) = writer.push_packet(payload)
        {
            tracing::debug!(error = %e, "screen share sound packet not decoded");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use mumcord_audio::FRAME_SAMPLES;
    use mumcord_audio::codec::MAX_PACKET_BYTES;
    use mumcord_audio::mixer::share::share_encoder;

    fn packet() -> Vec<u8> {
        let mut encoder = share_encoder().unwrap();
        let mut out = [0u8; MAX_PACKET_BYTES];
        let frame = [0.1; FRAME_SAMPLES];
        encoder.push_frame(&frame, &mut out).unwrap();
        let len = encoder.push_frame(&frame, &mut out).unwrap().unwrap();
        out[..len].to_vec()
    }

    #[test]
    fn packets_wait_for_a_playback_thread_and_follow_it() {
        let voice = Arc::new(VoiceShared::default());
        let mut sound = ShareSound::new(voice.clone());
        sound.packet(&packet());
        assert_eq!(sound.stats.snapshot().packets, 0, "no playback thread yet");

        let (_playback, ports) = mumcord_audio::mixer::Playback::start(
            NullSink,
            mumcord_audio::mixer::PlaybackConfig::default(),
        )
        .unwrap();
        voice.share_sound.attach(ports.shares);
        sound.packet(&packet());
        assert_eq!(sound.stats.snapshot().packets, 1);

        voice.deaf.store(true, Ordering::Relaxed);
        sound.packet(&packet());
        assert_eq!(sound.stats.snapshot().packets, 1, "deafened");
        voice.deaf.store(false, Ordering::Relaxed);

        voice.share_sound.detach();
        sound.packet(&packet());
        assert_eq!(sound.stats.snapshot().packets, 1, "playback thread gone");
    }

    struct NullSink;

    impl mumcord_audio::mixer::PlaybackSink for NullSink {
        fn write_frame(&mut self, _: &mumcord_audio::Frame) -> bool {
            false
        }

        fn queued_samples(&mut self) -> usize {
            0
        }
    }
}
