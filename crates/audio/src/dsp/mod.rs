//! Send-side audio processing (spec §6):
//! capture → echo cancellation → AI noise suppression → automatic gain →
//! voice-activity detection or push-to-talk gate → (Opus encoder).
//!
//! # Wiring
//!
//! - The send thread owns a [`DspChain`] and calls [`DspChain::process`]
//!   for every captured [`Frame`]. Frames for which it returns
//!   [`Gate::Open`] go to the Opus encoder; when the gate closes, the
//!   encoder is flushed so the last packet goes out.
//! - The playback side pushes every frame it writes to the `FrameSink` into
//!   the [`EchoRefProducer`] from [`echo_reference`]; the chain uses those
//!   frames as the echo canceller's far-end reference.
//! - Settings, push-to-talk key state and the level meter live in a shared
//!   [`DspControls`] (atomics only), so UI and hotkey threads never touch
//!   the chain itself.
//! - [`MicTest`] is the first-start wizard's "test playback" (FR-1.4).
//!
//! # Real-time behaviour
//!
//! All state is allocated in [`DspChain::new`]; `process` does not lock,
//! log or do I/O. Echo cancellation and automatic gain use `sonora`
//! (WebRTC APM port), noise suppression and the voice-activity probability
//! come from `nnnoiseless` (RNNoise). Disabled stages are skipped rather
//! than reconfigured, because reconfiguring `sonora` reallocates.

mod controls;
mod echo_ref;
mod gate;
mod mic_test;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use nnnoiseless::DenoiseState;
use sonora::config::{AdaptiveDigital, EchoCanceller, GainController2};
use sonora::{AudioProcessing, Config, StreamConfig};

use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

pub use controls::{
    DspControls, Level, THRESHOLD_DEFAULT_DBFS, THRESHOLD_MAX_DBFS, THRESHOLD_MIN_DBFS, TalkMode,
};
pub use echo_ref::{EchoRefConsumer, EchoRefProducer, echo_reference};
pub use mic_test::{MicTest, MicTestPhase};

use gate::{PttGate, ThresholdGate, VoiceDetector};

/// nnnoiseless expects samples in 16-bit integer range.
const I16_SCALE: f32 = 32_768.0;

/// Decision of the send gate for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Transmit this frame.
    Open,
    /// Do not transmit this frame.
    Closed,
}

/// The send-side processing chain for one microphone stream.
pub struct DspChain {
    controls: Arc<DspControls>,
    echo_ref: EchoRefConsumer,
    aec: AudioProcessing,
    agc: AudioProcessing,
    denoise: Box<DenoiseState<'static>>,
    vad: VoiceDetector,
    ptt: PttGate,
    threshold: ThresholdGate,
    /// Set by [`MicTest`]: the wizard always demonstrates noise suppression.
    force_noise_suppression: bool,
    scratch: Frame,
    render: Frame,
    render_out: Frame,
    ns_in: Frame,
    ns_out: Frame,
}

impl DspChain {
    pub fn new(controls: Arc<DspControls>, echo_ref: EchoRefConsumer) -> Self {
        let stream = StreamConfig::new(SAMPLE_RATE, 1);
        // Two separate APM instances because nnnoiseless sits between echo
        // cancellation and gain control in the chain.
        let aec = AudioProcessing::builder()
            .config(Config {
                echo_canceller: Some(EchoCanceller::default()),
                ..Config::default()
            })
            .capture_config(stream)
            .render_config(stream)
            .build();
        let agc = AudioProcessing::builder()
            .config(Config {
                gain_controller2: Some(GainController2 {
                    input_volume_controller: false,
                    adaptive_digital: Some(AdaptiveDigital::default()),
                    ..GainController2::default()
                }),
                ..Config::default()
            })
            .capture_config(stream)
            .render_config(stream)
            .build();
        Self {
            controls,
            echo_ref,
            aec,
            agc,
            denoise: DenoiseState::new(),
            vad: VoiceDetector::default(),
            ptt: PttGate::default(),
            threshold: ThresholdGate::default(),
            force_noise_suppression: false,
            scratch: [0.0; FRAME_SAMPLES],
            render: [0.0; FRAME_SAMPLES],
            render_out: [0.0; FRAME_SAMPLES],
            ns_in: [0.0; FRAME_SAMPLES],
            ns_out: [0.0; FRAME_SAMPLES],
        }
    }

    /// The shared controls of this chain.
    pub fn controls(&self) -> &Arc<DspControls> {
        &self.controls
    }

    /// Processes one captured frame into `out` and decides whether it is
    /// transmitted. Also updates the level meter and talk state in
    /// [`DspControls`].
    pub fn process(&mut self, input: &Frame, out: &mut Frame) -> Gate {
        let controls = &*self.controls;
        let aec_on = controls.echo_cancellation();

        // Always drain the reference so it never goes stale while echo
        // cancellation is off.
        while self.echo_ref.pop(&mut self.render) {
            if aec_on
                && self
                    .aec
                    .process_render_f32(&[&self.render], &mut [&mut self.render_out])
                    .is_err()
            {
                controls.count_error();
            }
        }

        *out = *input;
        if aec_on {
            self.scratch = *out;
            if self
                .aec
                .process_capture_f32(&[&self.scratch], &mut [out])
                .is_err()
            {
                controls.count_error();
                *out = self.scratch;
            }
        }

        // The model runs even with suppression off: its speech probability
        // drives voice activation.
        for (scaled, &sample) in self.ns_in.iter_mut().zip(out.iter()) {
            *scaled = sample * I16_SCALE;
        }
        let speech_probability = self.denoise.process_frame(&mut self.ns_out, &self.ns_in);
        let music = controls.music_quality();
        if self.force_noise_suppression || (controls.noise_suppression() && !music) {
            for (sample, &denoised) in out.iter_mut().zip(self.ns_out.iter()) {
                *sample = denoised / I16_SCALE;
            }
        }

        if controls.auto_gain() && !music {
            self.scratch = *out;
            if self
                .agc
                .process_capture_f32(&[&self.scratch], &mut [out])
                .is_err()
            {
                controls.count_error();
                *out = self.scratch;
            }
        }

        let mut peak = 0.0f32;
        let mut sum_sq = 0.0f32;
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
            peak = peak.max(sample.abs());
            sum_sq += *sample * *sample;
        }
        let rms = (sum_sq / FRAME_SAMPLES as f32).sqrt();

        // Both detectors run every frame so switching modes starts from a warm state. The manual
        // threshold looks at the processed peak, the same value the settings meter draws, so the
        // line the user sets is exactly the level that opens the gate.
        let automatic = self.vad.update(speech_probability, rms);
        let manual = self.threshold.update(
            if peak > 0.0 {
                20.0 * peak.log10()
            } else {
                f32::NEG_INFINITY
            },
            controls.threshold_dbfs(),
        );
        let voice = if controls.manual_threshold() {
            manual
        } else {
            automatic
        };
        let open = match controls.mode() {
            TalkMode::VoiceActivation => voice,
            TalkMode::PushToTalk => self.ptt.update(controls.ptt_pressed()),
        };
        controls.publish_frame(peak, rms, voice, open);
        if open { Gate::Open } else { Gate::Closed }
    }
}
