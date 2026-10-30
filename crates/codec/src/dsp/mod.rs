//! Send-side audio processing (spec §6):
//! capture → echo cancellation → noise suppression (selected backend) → automatic gain
//! (with "Music quality": slow loudness leveler instead) → voice-activity detection or push-to-talk gate → (Opus encoder).
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
//!
//! # Real-time behaviour
//!
//! All state is allocated in [`DspChain::new`]; `process` does not lock, log or do I/O. Echo
//! cancellation and automatic gain use `sonora` (WebRTC APM port). Noise suppression runs the
//! backend the user selected (see [`noise_suppression`]); only that one touches the audio. When it
//! reports no speech probability, the `nnnoiseless` model is used as a detector for voice
//! activation, and its audio output is discarded. Disabled stages are skipped rather than
//! reconfigured, because reconfiguring `sonora` reallocates; a backend switch happens in the
//! settings thread and the chain only adopts the finished object.

mod controls;
mod echo_ref;
mod gate;
mod leveler;
mod noise_suppression;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use sonora::config::{AdaptiveDigital, EchoCanceller, GainController2};
use sonora::{AudioProcessing, Config, StreamConfig};

use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

pub use controls::{
    DspControls, Level, THRESHOLD_DEFAULT_DBFS, THRESHOLD_MAX_DBFS, THRESHOLD_MIN_DBFS, TalkMode,
};
pub use echo_ref::{EchoRefConsumer, EchoRefProducer, echo_reference};
pub use noise_suppression::{
    Backend, BackendBuilder, Outcome, SuppressionBackend, SuppressionLevel, build_backend,
};

use gate::{PttGate, ThresholdGate, VoiceDetector};
use leveler::Leveler;

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
    /// The selected noise suppression backend; only this one changes the audio.
    backend: Box<dyn SuppressionBackend>,
    /// Speech detector for voice activation. It runs only when the selected backend reports no
    /// speech probability, and never writes to the audio.
    detector: Box<dyn SuppressionBackend>,
    leveler: Leveler,
    /// Whether the leveler ran on the previous frame; it restarts from unity gain when it
    /// comes back, as the source may have changed meanwhile.
    leveling: bool,
    vad: VoiceDetector,
    ptt: PttGate,
    threshold: ThresholdGate,
    /// Set by the microphone test of `mumcord-audio`: the wizard always demonstrates noise suppression.
    force_noise_suppression: bool,
    scratch: Frame,
    render: Frame,
    render_out: Frame,
    processed: Frame,
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
        let backend = controls.take_backend().unwrap_or_else(|| {
            controls.build(controls.noise_backend(), controls.suppression_level())
        });
        let detector = controls.build(Backend::Nnnoiseless, SuppressionLevel::default());
        Self {
            controls,
            echo_ref,
            aec,
            agc,
            backend,
            detector,
            leveler: Leveler::default(),
            leveling: false,
            vad: VoiceDetector::default(),
            ptt: PttGate::default(),
            threshold: ThresholdGate::default(),
            force_noise_suppression: false,
            scratch: [0.0; FRAME_SAMPLES],
            render: [0.0; FRAME_SAMPLES],
            render_out: [0.0; FRAME_SAMPLES],
            processed: [0.0; FRAME_SAMPLES],
        }
    }

    /// The shared controls of this chain.
    pub fn controls(&self) -> &Arc<DspControls> {
        &self.controls
    }

    /// The backend that is currently processing this chain's frames.
    pub fn backend(&self) -> Backend {
        self.backend.kind()
    }

    /// Keeps noise suppression on regardless of the controls (the wizard's microphone test).
    pub fn force_noise_suppression(&mut self) {
        self.force_noise_suppression = true;
    }

    /// Processes one captured frame into `out` and decides whether it is
    /// transmitted. Also updates the level meter and talk state in
    /// [`DspControls`].
    pub fn process(&mut self, input: &Frame, out: &mut Frame) -> Gate {
        let controls = &*self.controls;
        // A backend switch is only adopted here; the settings thread built it, so nothing is
        // allocated in the frame path.
        if let Some(backend) = controls.take_backend() {
            self.backend = backend;
        }
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

        // The selected backend is the only one that changes the audio. `out` already holds the
        // frame the backend gets, so a rejected frame passes through without an extra copy.
        let music = controls.music_quality();
        let mut probability = None;
        if self.force_noise_suppression || (controls.noise_suppression() && !music) {
            let outcome = self.backend.process(out, &mut self.processed);
            if outcome.failed {
                controls.count_error();
            } else {
                *out = self.processed;
            }
            probability = outcome.probability;
        }
        // The gate still needs a speech probability when suppression is off or the backend has no
        // model of its own: the detector reads the frame and its audio output is discarded.
        if probability.is_none() {
            probability = self.detector.process(out, &mut self.processed).probability;
        }

        let level_music = controls.auto_gain() && music;
        if level_music {
            if !self.leveling {
                self.leveler = Leveler::default();
            }
            self.leveler.process(out);
        }
        self.leveling = level_music;
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
        let automatic = self.vad.update(probability.unwrap_or(0.0), rms);
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
