//! Stable frame-level handles on top of the device rings.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer};

use super::IoError;
use super::resample::Resampler;
use super::ring::Counters;
use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

/// Capture side of an opened input stream, handed to [`FrameSource`].
pub(crate) struct CaptureEndpoint {
    consumer: Consumer<f32>,
    resampler: Option<Resampler>,
    device_rate: u32,
}

impl CaptureEndpoint {
    pub(crate) fn new(consumer: Consumer<f32>, device_rate: u32) -> Result<Self, IoError> {
        let resampler = (device_rate != SAMPLE_RATE)
            .then(|| Resampler::new(device_rate, SAMPLE_RATE))
            .transpose()?;
        Ok(Self {
            consumer,
            resampler,
            device_rate,
        })
    }
}

/// Playback side of an opened output stream, handed to [`FrameSink`]. The ring
/// carries interleaved stereo samples.
pub(crate) struct PlaybackEndpoint {
    producer: Producer<f32>,
    /// Left and right resampler; fed in lockstep, so their outputs stay aligned.
    resampler: Option<(Resampler, Resampler)>,
    device_rate: u32,
}

impl PlaybackEndpoint {
    pub(crate) fn new(producer: Producer<f32>, device_rate: u32) -> Result<Self, IoError> {
        let resampler = (device_rate != SAMPLE_RATE)
            .then(|| {
                Ok::<_, IoError>((
                    Resampler::new(SAMPLE_RATE, device_rate)?,
                    Resampler::new(SAMPLE_RATE, device_rate)?,
                ))
            })
            .transpose()?;
        Ok(Self {
            producer,
            resampler,
            device_rate,
        })
    }
}

/// Captured audio as 48 kHz mono 10 ms frames.
///
/// Non-blocking and allocation-free in steady state; call it from the
/// processing thread (not from a device callback). Capture is the clock of
/// the send path: one frame becomes available every 10 ms.
pub struct FrameSource {
    updates: Receiver<CaptureEndpoint>,
    endpoint: Option<CaptureEndpoint>,
    scratch: Vec<f32>,
}

impl FrameSource {
    pub(crate) fn new(updates: Receiver<CaptureEndpoint>) -> Self {
        Self {
            updates,
            endpoint: None,
            scratch: Vec::with_capacity(SAMPLE_RATE as usize / 10),
        }
    }

    /// Reads the next frame if one is complete; returns `false` otherwise
    /// (also while no input device is open).
    pub fn read_frame(&mut self, frame: &mut Frame) -> bool {
        self.apply_updates();
        let Some(endpoint) = &mut self.endpoint else {
            return false;
        };
        match &mut endpoint.resampler {
            None => {
                let Ok(chunk) = endpoint.consumer.read_chunk(FRAME_SAMPLES) else {
                    return false;
                };
                let (first, second) = chunk.as_slices();
                frame[..first.len()].copy_from_slice(first);
                frame[first.len()..].copy_from_slice(second);
                chunk.commit_all();
                true
            }
            Some(resampler) => {
                if let Ok(chunk) = endpoint.consumer.read_chunk(endpoint.consumer.slots()) {
                    let (first, second) = chunk.as_slices();
                    self.scratch.clear();
                    self.scratch.extend_from_slice(first);
                    self.scratch.extend_from_slice(second);
                    chunk.commit_all();
                    resampler.push(&self.scratch);
                }
                resampler.pop_exact(frame)
            }
        }
    }

    /// Like [`read_frame`](Self::read_frame) but waits up to `timeout`
    /// (polling at 1 ms granularity) for the frame.
    pub fn wait_frame(&mut self, frame: &mut Frame, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.read_frame(frame) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Captured audio waiting to be read, in 48 kHz samples. A growing
    /// backlog means the reader fell behind; dropping frames brings latency down.
    pub fn queued_samples(&mut self) -> usize {
        self.apply_updates();
        self.endpoint.as_ref().map_or(0, |e| {
            let ring = e.consumer.slots() * SAMPLE_RATE as usize / e.device_rate as usize;
            ring + e.resampler.as_ref().map_or(0, Resampler::pending)
        })
    }

    fn apply_updates(&mut self) {
        while let Ok(endpoint) = self.updates.try_recv() {
            self.endpoint = Some(endpoint);
        }
    }
}

/// Playback of 48 kHz 10 ms frames, mono or stereo.
///
/// The caller (the mixer) paces itself: keep [`queued_samples`](Self::queued_samples)
/// around the desired output latency (e.g. 20–30 ms) and write one frame
/// whenever it drops below. Writing faster than the device plays fills the
/// queue and frames are dropped.
pub struct FrameSink {
    updates: Receiver<PlaybackEndpoint>,
    endpoint: Option<PlaybackEndpoint>,
    counters: Arc<Counters>,
}

impl FrameSink {
    pub(crate) fn new(updates: Receiver<PlaybackEndpoint>, counters: Arc<Counters>) -> Self {
        Self {
            updates,
            endpoint: None,
            counters,
        }
    }

    /// Queues a mono frame for playback (same signal on both channels). Returns
    /// `false` if it was dropped because the queue is full or no output device is open.
    pub fn write_frame(&mut self, frame: &Frame) -> bool {
        self.write_stereo(frame, frame)
    }

    /// Queues a stereo frame for playback; see [`write_frame`](Self::write_frame).
    pub fn write_stereo(&mut self, left: &Frame, right: &Frame) -> bool {
        self.apply_updates();
        let Some(endpoint) = &mut self.endpoint else {
            return false;
        };
        let interleave = |l: f32, r: f32| [l, r];
        let written = match &mut endpoint.resampler {
            None => match endpoint.producer.write_chunk_uninit(2 * FRAME_SAMPLES) {
                Ok(chunk) => {
                    chunk.fill_from_iter(
                        left.iter().zip(right).flat_map(|(&l, &r)| interleave(l, r)),
                    );
                    true
                }
                Err(_) => false,
            },
            Some((resample_left, resample_right)) => {
                if resample_left.pending() > FRAME_SAMPLES * 2 {
                    false
                } else {
                    resample_left.push(left);
                    resample_right.push(right);
                    let n = resample_left
                        .pending()
                        .min(resample_right.pending())
                        .min(endpoint.producer.slots() / 2);
                    if let Ok(chunk) = endpoint.producer.write_chunk_uninit(2 * n) {
                        chunk.fill_from_iter(
                            resample_left
                                .drain(n)
                                .zip(resample_right.drain(n))
                                .flat_map(|(l, r)| interleave(l, r)),
                        );
                    }
                    true
                }
            }
        };
        if !written {
            self.counters.playback_frame_dropped();
        }
        written
    }

    /// Audio queued but not yet handed to the device, in 48 kHz samples (per channel).
    pub fn queued_samples(&mut self) -> usize {
        self.apply_updates();
        self.endpoint.as_ref().map_or(0, |e| {
            let ring = (e.producer.buffer().capacity() - e.producer.slots()) / 2;
            let pending = e.resampler.as_ref().map_or(0, |(l, _)| l.pending());
            (ring + pending) * SAMPLE_RATE as usize / e.device_rate as usize
        })
    }

    fn apply_updates(&mut self) {
        while let Ok(endpoint) = self.updates.try_recv() {
            self.endpoint = Some(endpoint);
        }
    }
}
