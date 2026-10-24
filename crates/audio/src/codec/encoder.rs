use opus::{Application, Bitrate, Channels, Encoder};

use super::CodecError;
use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

/// Largest possible Opus packet for a single frame (RFC 6716, 3.2.1).
pub const MAX_PACKET_BYTES: usize = 1275;

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
        let len = self
            .encoder
            .encode_float(&self.pending[..self.pending_len], out)?;
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
