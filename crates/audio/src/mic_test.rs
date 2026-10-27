//! First-start wizard microphone test (FR-1.4): record a few seconds
//! through the processing chain (noise suppression always on), then play
//! the recording back.
//!
//! Record-then-play instead of live monitoring: live loopback over
//! speakers howls, and the echo canceller would treat the user's own
//! delayed voice as echo.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use mumcord_codec::dsp::{DspChain, DspControls, echo_reference};
use mumcord_codec::{FRAME_SAMPLES, Frame};

use crate::io::{FrameSink, FrameSource};

/// Playback queue the test keeps filled, in samples (30 ms).
const PLAYBACK_QUEUE: usize = 3 * FRAME_SAMPLES;
const POLL: Duration = Duration::from_millis(2);

/// Where a running [`MicTest`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicTestPhase {
    Recording,
    Playing,
    Done,
}

impl MicTestPhase {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Recording,
            1 => Self::Playing,
            _ => Self::Done,
        }
    }
}

struct Shared {
    phase: AtomicU8,
    stop_recording: AtomicBool,
    cancel: AtomicBool,
}

/// A running microphone test on its own (non-real-time) thread.
///
/// While recording, the level meter in the given [`DspControls`] stays live.
/// Device handles are borrowed for the duration of the test and returned
/// by [`MicTest::finish`].
pub struct MicTest {
    shared: Arc<Shared>,
    thread: JoinHandle<(FrameSource, FrameSink)>,
}

impl MicTest {
    /// Starts recording for up to `max_duration`, then plays it back.
    pub fn start(
        source: FrameSource,
        sink: FrameSink,
        controls: Arc<DspControls>,
        max_duration: Duration,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            phase: AtomicU8::new(MicTestPhase::Recording as u8),
            stop_recording: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        });
        let max_frames = (max_duration.as_millis() / 10).max(1) as usize;
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("mic-test".to_owned())
            .spawn(move || run(source, sink, controls, max_frames, &thread_shared))?;
        Ok(Self { shared, thread })
    }

    pub fn phase(&self) -> MicTestPhase {
        MicTestPhase::from_u8(self.shared.phase.load(Ordering::Acquire))
    }

    /// Ends recording early and starts playback.
    pub fn stop_recording(&self) {
        self.shared.stop_recording.store(true, Ordering::Release);
    }

    /// Stops the test wherever it is and hands the device handles back.
    /// `None` only if the test thread panicked.
    pub fn finish(self) -> Option<(FrameSource, FrameSink)> {
        self.shared.cancel.store(true, Ordering::Release);
        self.thread.join().ok()
    }
}

fn run(
    mut source: FrameSource,
    mut sink: FrameSink,
    controls: Arc<DspControls>,
    max_frames: usize,
    shared: &Shared,
) -> (FrameSource, FrameSink) {
    // No playback happens while recording, so there is nothing to cancel.
    let (_producer, consumer) = echo_reference();
    let mut chain = DspChain::new(controls, consumer);
    chain.force_noise_suppression();

    let mut recording: Vec<Frame> = Vec::with_capacity(max_frames);
    let mut input = [0.0; FRAME_SAMPLES];
    let mut output = [0.0; FRAME_SAMPLES];
    while recording.len() < max_frames
        && !shared.stop_recording.load(Ordering::Acquire)
        && !shared.cancel.load(Ordering::Acquire)
    {
        if source.wait_frame(&mut input, Duration::from_millis(50)) {
            chain.process(&input, &mut output);
            recording.push(output);
        }
    }

    shared
        .phase
        .store(MicTestPhase::Playing as u8, Ordering::Release);
    let mut frames = recording.iter();
    let mut next = frames.next();
    while !shared.cancel.load(Ordering::Acquire) {
        // Keep reading capture so no backlog builds up for whoever gets the
        // source back.
        while source.read_frame(&mut input) {}
        if sink.queued_samples() >= PLAYBACK_QUEUE {
            std::thread::sleep(POLL);
            continue;
        }
        let Some(frame) = next else {
            break;
        };
        sink.write_frame(frame);
        next = frames.next();
    }
    shared
        .phase
        .store(MicTestPhase::Done as u8, Ordering::Release);
    (source, sink)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;
    use std::path::Path;
    use std::time::Instant;

    use mumcord_codec::SAMPLE_RATE;

    use super::*;
    use crate::io::{AudioHandles, AudioIo, BackendKind};

    /// A voiced, 4 Hz pulsed signal that the mic test's noise suppression treats as speech.
    fn speech_like(len: usize, amplitude: f32) -> Vec<f32> {
        let mut phase = 0.0f32;
        (0..len)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                let f0 = 140.0 + 20.0 * (2.0 * PI * 0.7 * t).sin();
                phase += 2.0 * PI * f0 / SAMPLE_RATE as f32;
                let voiced: f32 = (1..=20)
                    .map(|h| (h as f32 * phase).sin() / h as f32)
                    .sum::<f32>()
                    / 2.0;
                let envelope = (2.0 * PI * 4.0 * t).sin().max(0.0).sqrt();
                amplitude * envelope * voiced
            })
            .collect()
    }

    fn write_wav(path: &Path, samples: &[f32]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
        for s in samples {
            writer
                .write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)
                .expect("write");
        }
        writer.finalize().expect("finalize");
    }

    fn read_wav(path: &Path) -> Vec<f32> {
        hound::WavReader::open(path)
            .expect("open wav")
            .samples::<i16>()
            .map(|s| f32::from(s.expect("sample")) / 32768.0)
            .collect()
    }

    #[test]
    fn mic_test_records_then_plays_back() {
        let dir = std::env::temp_dir().join(format!("mumcord-dsp-mic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
        write_wav(&input, &speech_like(SAMPLE_RATE as usize, 0.3));

        let kind = BackendKind::File {
            input: input.clone(),
            output: output.clone(),
        };
        let AudioHandles {
            io, source, sink, ..
        } = AudioIo::start_with(kind, None, None).expect("start");
        let controls = Arc::new(DspControls::default());
        controls.set_noise_suppression(false);
        let started = Instant::now();
        let test = MicTest::start(source, sink, controls.clone(), Duration::from_secs(5))
            .expect("start mic test");
        assert_eq!(test.phase(), MicTestPhase::Recording);

        // The meter shows the latest frame and the test speech pulses at 4 Hz,
        // so a single reading can land in a pause; poll until a loud frame shows
        // up (processing may lag real time on a loaded machine).
        let deadline = started + Duration::from_secs(4);
        while controls.level().peak_dbfs <= -30.0 {
            assert!(Instant::now() < deadline, "meter not live");
            assert_eq!(test.phase(), MicTestPhase::Recording);
            std::thread::sleep(Duration::from_millis(2));
        }
        // Record at least 600 ms of the 1 s input before stopping.
        if let Some(rest) = Duration::from_millis(600).checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
        test.stop_recording();
        let deadline = Instant::now() + Duration::from_secs(10);
        while test.phase() != MicTestPhase::Done {
            assert!(Instant::now() < deadline, "mic test never finished");
            std::thread::sleep(Duration::from_millis(10));
        }
        let (_source, mut sink) = test.finish().expect("handles returned");
        // Let the queued tail play out before the backend is closed.
        let deadline = Instant::now() + Duration::from_secs(5);
        while sink.queued_samples() > 0 {
            assert!(Instant::now() < deadline, "playback queue never drained");
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(io);

        let recorded = read_wav(&output);
        std::fs::remove_dir_all(&dir).expect("cleanup");
        let audible = recorded.iter().filter(|s| s.abs() > 0.05).count();
        println!("mic test played back {audible} audible samples");
        assert!(audible >= SAMPLE_RATE as usize / 10, "playback missing");
    }
}
