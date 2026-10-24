//! Far-end (playback) reference for echo cancellation.
//!
//! Whatever goes to the speakers can leak back into the microphone. The
//! echo canceller needs a copy of those frames to subtract them, so the
//! playback side pushes every frame it writes to the `FrameSink` into an
//! [`EchoRefProducer`], and the processing chain drains the matching
//! [`EchoRefConsumer`] before each capture frame. The two sides run on
//! different threads; the hand-over is a lock-free SPSC ring.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::{FRAME_SAMPLES, Frame};

/// Reference audio buffered between playback and capture. Generous because
/// the capture thread may lag playback by a few frames; beyond this the
/// reference is stale anyway.
const CAPACITY_FRAMES: usize = 20;

/// Creates a connected producer/consumer pair.
pub fn echo_reference() -> (EchoRefProducer, EchoRefConsumer) {
    let (producer, consumer) = RingBuffer::new(CAPACITY_FRAMES * FRAME_SAMPLES);
    let dropped = Arc::new(AtomicU64::new(0));
    (
        EchoRefProducer {
            producer,
            dropped: dropped.clone(),
        },
        EchoRefConsumer { consumer, dropped },
    )
}

/// Playback-side end: push every frame that is written to the `FrameSink`.
pub struct EchoRefProducer {
    producer: Producer<f32>,
    dropped: Arc<AtomicU64>,
}

impl EchoRefProducer {
    /// Never blocks or allocates. Returns `false` and counts a drop when the
    /// ring is full (the capture side is not draining, e.g. while it is
    /// stopped).
    pub fn push(&mut self, frame: &Frame) -> bool {
        match self.producer.write_chunk_uninit(FRAME_SAMPLES) {
            Ok(chunk) => {
                chunk.fill_from_iter(frame.iter().copied());
                true
            }
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Frames dropped because the ring was full, for diagnostics.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Capture-side end, owned by the [`DspChain`](super::DspChain).
pub struct EchoRefConsumer {
    consumer: Consumer<f32>,
    dropped: Arc<AtomicU64>,
}

impl EchoRefConsumer {
    /// Pops the oldest reference frame, if a complete one is queued.
    pub(super) fn pop(&mut self, frame: &mut Frame) -> bool {
        let Ok(chunk) = self.consumer.read_chunk(FRAME_SAMPLES) else {
            return false;
        };
        let (first, second) = chunk.as_slices();
        frame[..first.len()].copy_from_slice(first);
        frame[first.len()..].copy_from_slice(second);
        chunk.commit_all();
        true
    }

    /// Frames dropped on the producer side, for diagnostics.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}
