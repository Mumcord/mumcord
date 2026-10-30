//! Mouth-to-ear latency estimate (issue #156): the sum of what one 10 ms slice of sound waits
//! for on its way from the speaker's microphone to the listener's ear, assuming both ends run in
//! the same mode. Device periods are measured, the network half of the measured ping, the rest
//! follows from the mode.

use std::time::Duration;

use mumcord_audio::SAMPLE_RATE;
use mumcord_audio::io::{DevicePeriods, LOW_LATENCY_BUFFER_FRAMES};
use mumcord_audio::mixer::{LOW_LATENCY_QUEUE_MS, PlaybackConfig};

/// Opus look-ahead: SILK and hybrid modes (VoIP and audio application) need 6.5 ms, the
/// CELT-only restricted low-delay application 2.5 ms.
const LOOKAHEAD_MS: f32 = 6.5;
const LOW_DELAY_LOOKAHEAD_MS: f32 = 2.5;
/// Device period assumed for normal mode while none has been measured.
const ASSUMED_PERIOD_MS: f32 = 10.0;
/// Headroom of the output queue above one device period: the playback thread polls every 2 ms
/// and must refill before the device takes the next period.
const QUEUE_MARGIN_MS: f32 = 3.0;
/// The output queue holds between its target and one 10 ms frame more, so on average half a
/// frame above the target.
const QUEUE_HALF_FRAME_MS: f32 = 5.0;

/// The estimate and its parts, in ms. `network_ms` is `None` until a ping is measured; the
/// totals then leave the network out.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct LatencyEstimateView {
    pub low_latency: bool,
    /// Audio per packet: 10 ms in low-latency mode, 20 ms otherwise.
    pub packet_ms: u32,
    /// Microphone device buffer (one period).
    pub input_buffer_ms: f32,
    /// One packet of audio plus the encoder look-ahead.
    pub encode_ms: f32,
    /// Half the round trip to the server.
    pub network_ms: Option<f32>,
    /// Jitter-buffer target for a clean stream (one packet).
    pub jitter_ms: f32,
    /// Measured processing per frame: DSP and encoding on the way out, decoding and mixing in.
    pub processing_ms: f32,
    /// Output queue and speaker device buffer.
    pub output_buffer_ms: f32,
    pub total_ms: f32,
    /// The same estimate for the other mode, for comparison.
    pub other_mode_total_ms: f32,
}

/// What the estimate is computed from.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LatencyInputs {
    pub low_latency: bool,
    pub periods: DevicePeriods,
    pub rtt_ms: Option<f32>,
    pub capture_time_avg_us: u64,
    pub mix_time_avg_us: u64,
}

/// Output queue target of the playback thread: the normal default, or in low-latency mode one
/// output period plus headroom, at least [`LOW_LATENCY_QUEUE_MS`]. Stays at the default until
/// the period is known, so a device that refused the small buffer never runs dry.
pub(crate) fn queue_target_ms(low_latency: bool, output_period: Option<Duration>) -> u32 {
    let normal = PlaybackConfig::default().target_queue_ms;
    match output_period {
        Some(period) if low_latency => {
            let wanted = (ms(period) + QUEUE_MARGIN_MS).ceil() as u32;
            wanted.clamp(LOW_LATENCY_QUEUE_MS, normal)
        }
        _ => normal,
    }
}

pub(crate) fn estimate(inputs: &LatencyInputs) -> LatencyEstimateView {
    let current = mode_estimate(inputs, inputs.low_latency, inputs.periods);
    // The other mode's device buffers are not measured: assume the requested small buffer, or
    // at least the common default.
    let small =
        Duration::from_secs_f64(f64::from(LOW_LATENCY_BUFFER_FRAMES) / f64::from(SAMPLE_RATE));
    let other_period = |measured: Option<Duration>| {
        Some(if inputs.low_latency {
            measured.map_or(Duration::from_secs_f32(ASSUMED_PERIOD_MS / 1000.0), |m| {
                m.max(Duration::from_secs_f32(ASSUMED_PERIOD_MS / 1000.0))
            })
        } else {
            measured.map_or(small, |m| m.min(small))
        })
    };
    let other_periods = DevicePeriods {
        input: other_period(inputs.periods.input),
        output: other_period(inputs.periods.output),
    };
    let other = mode_estimate(inputs, !inputs.low_latency, other_periods);
    LatencyEstimateView {
        other_mode_total_ms: other.total_ms,
        ..current
    }
}

fn mode_estimate(
    inputs: &LatencyInputs,
    low_latency: bool,
    periods: DevicePeriods,
) -> LatencyEstimateView {
    let assumed = |period: Option<Duration>| period.map_or(ASSUMED_PERIOD_MS, ms);
    let packet_ms: u32 = if low_latency { 10 } else { 20 };
    let lookahead = if low_latency {
        LOW_DELAY_LOOKAHEAD_MS
    } else {
        LOOKAHEAD_MS
    };
    let input_buffer_ms = assumed(periods.input);
    let encode_ms = packet_ms as f32 + lookahead;
    let network_ms = inputs.rtt_ms.map(|rtt| rtt / 2.0);
    let jitter_ms = packet_ms as f32;
    let processing_ms = (inputs.capture_time_avg_us + inputs.mix_time_avg_us) as f32 / 1000.0;
    let queue = queue_target_ms(
        low_latency,
        Some(Duration::from_secs_f32(assumed(periods.output) / 1000.0)),
    );
    let output_buffer_ms = queue as f32 + QUEUE_HALF_FRAME_MS + assumed(periods.output);
    let total_ms = input_buffer_ms
        + encode_ms
        + network_ms.unwrap_or(0.0)
        + jitter_ms
        + processing_ms
        + output_buffer_ms;
    LatencyEstimateView {
        low_latency,
        packet_ms,
        input_buffer_ms,
        encode_ms,
        network_ms,
        jitter_ms,
        processing_ms,
        output_buffer_ms,
        total_ms,
        other_mode_total_ms: 0.0,
    }
}

fn ms(duration: Duration) -> f32 {
    duration.as_secs_f32() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn period(frames: u32) -> Option<Duration> {
        Some(Duration::from_secs_f64(f64::from(frames) / 48_000.0))
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn low_latency_parts_add_up() {
        let view = estimate(&LatencyInputs {
            low_latency: true,
            periods: DevicePeriods {
                input: period(128),
                output: period(128),
            },
            rtt_ms: Some(18.0),
            capture_time_avg_us: 300,
            mix_time_avg_us: 200,
        });
        assert!(view.low_latency);
        assert_eq!(view.packet_ms, 10);
        assert!(close(view.input_buffer_ms, 2.667));
        assert!(close(view.encode_ms, 12.5));
        assert_eq!(view.network_ms, Some(9.0));
        assert!(close(view.jitter_ms, 10.0));
        assert!(close(view.processing_ms, 0.5));
        // Queue target 6 ms (2.7 ms period + 3 ms headroom), half a frame above it on average,
        // plus the device period.
        assert!(close(view.output_buffer_ms, 6.0 + 5.0 + 2.667));
        let sum = view.input_buffer_ms
            + view.encode_ms
            + 9.0
            + view.jitter_ms
            + view.processing_ms
            + view.output_buffer_ms;
        assert!(close(view.total_ms, sum));
        assert!(view.total_ms < 50.0, "{view:?}");
        assert!(view.other_mode_total_ms > view.total_ms + 30.0, "{view:?}");
    }

    #[test]
    fn normal_mode_uses_20ms_packets_and_the_default_queue() {
        let view = estimate(&LatencyInputs {
            low_latency: false,
            periods: DevicePeriods {
                input: period(480),
                output: period(480),
            },
            rtt_ms: Some(18.0),
            ..LatencyInputs::default()
        });
        assert_eq!(view.packet_ms, 20);
        assert!(close(view.encode_ms, 26.5));
        assert!(close(view.jitter_ms, 20.0));
        assert!(close(view.output_buffer_ms, 25.0 + 5.0 + 10.0));
        assert!(view.other_mode_total_ms < view.total_ms);
    }

    #[test]
    fn unknown_ping_and_periods_fall_back() {
        let view = estimate(&LatencyInputs {
            low_latency: true,
            ..LatencyInputs::default()
        });
        assert_eq!(view.network_ms, None);
        assert!(close(view.input_buffer_ms, ASSUMED_PERIOD_MS));
        assert!(view.total_ms > 0.0);
    }

    #[test]
    fn queue_target_follows_the_output_period() {
        assert_eq!(queue_target_ms(false, period(128)), 25);
        assert_eq!(queue_target_ms(true, None), 25, "unknown period: stay safe");
        assert_eq!(queue_target_ms(true, period(64)), LOW_LATENCY_QUEUE_MS);
        assert_eq!(queue_target_ms(true, period(128)), 6);
        assert_eq!(queue_target_ms(true, period(480)), 13);
        // A device that kept a large default buffer gets no less than normal mode.
        assert_eq!(queue_target_ms(true, period(2048)), 25);
    }
}
