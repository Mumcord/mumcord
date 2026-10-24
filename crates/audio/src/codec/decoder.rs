use opus::{Channels, Decoder};

use super::CodecError;
use crate::SAMPLE_RATE;

/// Longest packet Opus can carry (120 ms at 48 kHz); size decode buffers with this.
pub const MAX_DECODED_SAMPLES: usize = 5760;

/// Opus decoder for one remote speaker (48 kHz mono output).
pub struct OpusDecoder {
    decoder: Decoder,
}

impl OpusDecoder {
    pub fn new() -> Result<Self, CodecError> {
        Ok(Self {
            decoder: Decoder::new(SAMPLE_RATE, Channels::Mono)?,
        })
    }

    /// Decodes a packet into `out` and returns the number of samples written
    /// (a multiple of 120; packets of 10 ms or more yield whole frames).
    pub fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, CodecError> {
        let needed = opus::packet::get_nb_samples(packet, SAMPLE_RATE)?;
        check_len(needed, out)?;
        Ok(self
            .decoder
            .decode_float(packet, &mut out[..needed], false)?)
    }

    /// Packet-loss concealment: synthesizes `out.len()` samples (a multiple of
    /// 120, typically one [`FRAME_SAMPLES`](crate::FRAME_SAMPLES) frame or the lost packet's length).
    pub fn conceal(&mut self, out: &mut [f32]) -> Result<usize, CodecError> {
        Ok(self.decoder.decode_float(&[], out, false)?)
    }

    /// Recovers a lost packet of `out.len()` samples from the in-band FEC data
    /// carried by the packet that followed it.
    pub fn recover_from_next(
        &mut self,
        next_packet: &[u8],
        out: &mut [f32],
    ) -> Result<usize, CodecError> {
        Ok(self.decoder.decode_float(next_packet, out, true)?)
    }

    /// Duration in samples of the last decoded or concealed packet.
    pub fn last_packet_samples(&mut self) -> Result<usize, CodecError> {
        Ok(self.decoder.get_last_packet_duration()? as usize)
    }

    pub fn reset(&mut self) -> Result<(), CodecError> {
        Ok(self.decoder.reset_state()?)
    }
}

fn check_len(needed: usize, out: &[f32]) -> Result<(), CodecError> {
    if out.len() < needed {
        return Err(CodecError::BufferTooSmall {
            needed,
            got: out.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{MAX_PACKET_BYTES, OpusEncoder, PacketDuration};
    use crate::{FRAME_SAMPLES, Frame};

    fn sine_frames(count: usize, freq: f32) -> Vec<Frame> {
        (0..count)
            .map(|f| {
                let mut frame = [0.0; FRAME_SAMPLES];
                for (i, s) in frame.iter_mut().enumerate() {
                    let t = (f * FRAME_SAMPLES + i) as f32 / SAMPLE_RATE as f32;
                    *s = 0.5 * (2.0 * std::f32::consts::PI * freq * t).sin();
                }
                frame
            })
            .collect()
    }

    /// SNR in dB of `decoded` against `reference`, after compensating the
    /// codec delay by searching the best alignment.
    fn snr_db(reference: &[f32], decoded: &[f32]) -> f32 {
        let window = &reference[4800..reference.len() - 4800];
        (0..1000)
            .map(|delay| {
                let signal: f32 = window.iter().map(|s| s * s).sum();
                let noise: f32 = window
                    .iter()
                    .zip(&decoded[4800 + delay..])
                    .map(|(r, d)| (r - d) * (r - d))
                    .sum();
                10.0 * (signal / noise).log10()
            })
            .fold(f32::MIN, f32::max)
    }

    fn round_trip(duration: PacketDuration) -> (Vec<f32>, Vec<f32>) {
        let frames = sine_frames(200, 440.0);
        let mut encoder = OpusEncoder::new(64_000, duration).expect("encoder");
        let mut decoder = OpusDecoder::new().expect("decoder");
        let mut packet = [0u8; MAX_PACKET_BYTES];
        let mut pcm = [0f32; MAX_DECODED_SAMPLES];
        let mut decoded = Vec::new();
        for frame in &frames {
            if let Some(len) = encoder.push_frame(frame, &mut packet).expect("encode") {
                let n = decoder.decode(&packet[..len], &mut pcm).expect("decode");
                assert_eq!(n, duration.frames() * FRAME_SAMPLES);
                decoded.extend_from_slice(&pcm[..n]);
            }
        }
        (frames.concat(), decoded)
    }

    #[test]
    fn sine_survives_round_trip_with_20ms_packets() {
        let (reference, decoded) = round_trip(PacketDuration::Ms20);
        assert_eq!(decoded.len(), reference.len());
        let snr = snr_db(&reference, &decoded);
        assert!(snr > 15.0, "SNR {snr} dB too low");
    }

    #[test]
    fn sine_survives_round_trip_with_10ms_packets() {
        let (reference, decoded) = round_trip(PacketDuration::Ms10);
        let snr = snr_db(&reference, &decoded);
        assert!(snr > 15.0, "SNR {snr} dB too low");
    }

    #[test]
    fn flush_emits_partial_packet_and_conceal_fills_gaps() {
        let mut encoder = OpusEncoder::new(32_000, PacketDuration::Ms20).expect("encoder");
        let mut packet = [0u8; MAX_PACKET_BYTES];
        let frame = sine_frames(1, 440.0)[0];
        assert!(
            encoder
                .push_frame(&frame, &mut packet)
                .expect("encode")
                .is_none()
        );
        let len = encoder.flush(&mut packet).expect("flush").expect("packet");
        assert!(encoder.flush(&mut packet).expect("flush").is_none());

        let mut decoder = OpusDecoder::new().expect("decoder");
        let mut pcm = [0f32; MAX_DECODED_SAMPLES];
        assert_eq!(
            decoder.decode(&packet[..len], &mut pcm).expect("decode"),
            960
        );
        assert_eq!(
            decoder.conceal(&mut pcm[..FRAME_SAMPLES]).expect("conceal"),
            FRAME_SAMPLES
        );
        assert!(decoder.decode(&packet[..len], &mut pcm[..100]).is_err());
    }

    #[test]
    fn bitrate_is_configurable() {
        let mut encoder = OpusEncoder::new(24_000, PacketDuration::Ms20).expect("encoder");
        assert_eq!(encoder.bitrate().expect("bitrate"), 24_000);
        encoder.set_bitrate(72_000).expect("set");
        assert_eq!(encoder.bitrate().expect("bitrate"), 72_000);
    }
}
