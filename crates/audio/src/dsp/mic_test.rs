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

use crate::io::{FrameSink, FrameSource};
use crate::{FRAME_SAMPLES, Frame};

use super::{DspChain, DspControls, echo_reference};

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
    chain.force_noise_suppression = true;

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
