use opus::{Application, Bitrate, Channels, Encoder, Signal};

use super::CodecError;
use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

/// Largest possible Opus packet for a single frame (RFC 6716, 3.2.1).
pub const MAX_PACKET_BYTES: usize = 1275;

/// Largest Opus payload that still fits one Mumble voice packet: the server accepts at most
/// 1024 bytes of plaintext (`Mumble::Protocol::MAX_UDP_PACKET_SIZE`), and the voice header
/// (legacy or protobuf format, with positional data and the largest sequence number) takes up
/// to 34 bytes. The rest is a safety margin. `mumcord_protocol::MAX_VOICE_PAYLOAD` is the same
/// budget seen from the packet side; the engine checks at compile time that both agree.
pub const MAX_VOICE_PAYLOAD_BYTES: usize = 958;

/// Audio duration carried by one outgoing Opus packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PacketDuration {
    Ms10,
    #[default]
    Ms20,
}

impl PacketDuration {
    /// Number of internal 10 ms frames per packet.
    pub fn frames(self) -> usize {
        match self {
            PacketDuration::Ms10 => 1,
            PacketDuration::Ms20 => 2,
        }
    }

    fn samples(self) -> usize {
        self.frames() * FRAME_SAMPLES
    }
}

/// Opus voice encoder that collects 10 ms frames into packets.
pub struct OpusEncoder {
    encoder: Encoder,
    duration: PacketDuration,
    pending: [f32; 2 * FRAME_SAMPLES],
    pending_len: usize,
}

impl OpusEncoder {
    /// Creates a VBR voice encoder with the given bitrate in bits per second.
    /// In-band FEC is enabled; it only costs bits once
    /// [`set_packet_loss_percent`](Self::set_packet_loss_percent) is above 0.
    pub fn new(bitrate_bps: i32, duration: PacketDuration) -> Result<Self, CodecError> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)?;
        encoder.set_bitrate(Bitrate::Bits(bitrate_bps))?;
        encoder.set_vbr(true)?;
        encoder.set_inband_fec(true)?;
        Ok(Self {
            encoder,
            duration,
            pending: [0.0; 2 * FRAME_SAMPLES],
            pending_len: 0,
        })
    }

    /// Adds one frame. Returns `Some(len)` when a packet was written to
    /// `out[..len]`; `out` should hold [`MAX_PACKET_BYTES`].
    pub fn push_frame(
        &mut self,
        frame: &Frame,
        out: &mut [u8],
    ) -> Result<Option<usize>, CodecError> {
        self.pending[self.pending_len..self.pending_len + FRAME_SAMPLES].copy_from_slice(frame);
        self.pending_len += FRAME_SAMPLES;
        if self.pending_len < self.duration.samples() {
            return Ok(None);
        }
        self.encode_pending(out).map(Some)
    }

    /// Encodes a partially filled packet, padded with silence, so the end of
    /// a talk spurt is not lost. Returns `None` if nothing was pending.
    pub fn flush(&mut self, out: &mut [u8]) -> Result<Option<usize>, CodecError> {
        if self.pending_len == 0 {
            return Ok(None);
        }
        let total = self.duration.samples();
        self.pending[self.pending_len..total].fill(0.0);
        self.pending_len = total;
        self.encode_pending(out).map(Some)
    }

    fn encode_pending(&mut self, out: &mut [u8]) -> Result<usize, CodecError> {
        // libopus treats the output size as a hard per-packet limit and spends fewer bits on
        // a frame that would not fit, so VBR peaks (music, transients) can never produce a
        // packet the server would refuse, whatever bitrate is set. Plain VBR keeps its
        // quality otherwise; CBR or constrained VBR would only cost quality and still need
        // this limit to be certain.
        let cap = out.len().min(MAX_VOICE_PAYLOAD_BYTES);
        let len = self
            .encoder
            .encode_float(&self.pending[..self.pending_len], &mut out[..cap])?;
        self.pending_len = 0;
        Ok(len)
    }

    pub fn set_bitrate(&mut self, bitrate_bps: i32) -> Result<(), CodecError> {
        Ok(self.encoder.set_bitrate(Bitrate::Bits(bitrate_bps))?)
    }

    pub fn bitrate(&mut self) -> Result<i32, CodecError> {
        Ok(match self.encoder.get_bitrate()? {
            Bitrate::Bits(bits) => bits,
            // Not set by this type, but reported by libopus if it ever is.
            Bitrate::Max | Bitrate::Auto => -1,
        })
    }

    /// Switches between the voice tuning (VoIP, voice signal) and the music tuning (audio
    /// application, music signal) used by "Music quality" (QA.37).
    pub fn set_music(&mut self, music: bool) -> Result<(), CodecError> {
        let (application, signal) = if music {
            (Application::Audio, Signal::Music)
        } else {
            (Application::Voip, Signal::Voice)
        };
        self.encoder.set_application(application)?;
        self.encoder.set_signal(signal)?;
        Ok(())
    }

    /// Whether the music tuning is active.
    pub fn is_music(&mut self) -> Result<bool, CodecError> {
        Ok(self.encoder.get_application()? == Application::Audio
            && self.encoder.get_signal()? == Signal::Music)
    }

    /// Expected packet loss (0–100); makes in-band FEC spend bits on redundancy.
    pub fn set_packet_loss_percent(&mut self, percent: i32) -> Result<(), CodecError> {
        Ok(self.encoder.set_packet_loss_perc(percent.clamp(0, 100))?)
    }

    pub fn packet_duration(&self) -> PacketDuration {
        self.duration
    }

    /// Changes the packet duration; any partially collected packet is dropped.
    pub fn set_packet_duration(&mut self, duration: PacketDuration) {
        self.duration = duration;
        self.pending_len = 0;
    }

    /// Starts a fresh stream (new talk spurt): drops pending audio and codec state.
    pub fn reset(&mut self) -> Result<(), CodecError> {
        self.pending_len = 0;
        Ok(self.encoder.reset_state()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_tuning_switches_both_ways() {
        let mut encoder = OpusEncoder::new(64_000, PacketDuration::Ms20).expect("encoder");
        assert!(!encoder.is_music().expect("read"));
        encoder.set_music(true).expect("music on");
        assert!(encoder.is_music().expect("read"));
        encoder.set_bitrate(500_000).expect("high bitrate");
        assert_eq!(encoder.bitrate().expect("read"), 500_000);
        encoder.set_music(false).expect("music off");
        assert!(!encoder.is_music().expect("read"));
    }

    /// Issue #7: music at the Opus maximum used to produce 20 ms packets of up to 1275 bytes,
    /// which the voice sender had to drop.
    #[test]
    fn packets_fit_a_voice_packet_even_at_the_opus_maximum() {
        let mut noise = 0x1234_5678_u32;
        let mut frame: Frame = [0.0; FRAME_SAMPLES];
        let mut out = [0u8; MAX_PACKET_BYTES];
        for duration in [PacketDuration::Ms10, PacketDuration::Ms20] {
            let mut encoder = OpusEncoder::new(510_000, duration).expect("encoder");
            encoder.set_music(true).expect("music on");
            for _ in 0..200 {
                for s in &mut frame {
                    noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *s = (noise >> 8) as f32 / (1 << 23) as f32 - 1.0;
                }
                if let Some(len) = encoder.push_frame(&frame, &mut out).expect("encode") {
                    assert!(len <= MAX_VOICE_PAYLOAD_BYTES, "{duration:?}: {len} bytes");
                }
            }
        }
    }
}
