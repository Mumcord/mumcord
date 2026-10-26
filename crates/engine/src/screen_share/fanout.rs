//! One encoded stream, many viewers (FR-2.44): every viewer has a small queue in front of its
//! stream writer. A viewer whose queue backs up loses temporal layers first (30 → 15 → 7.5 fps
//! with L1T3) and gets them back once it keeps up; a queue that overflows is cleared and the
//! viewer waits for the next keyframe, so a slow viewer never holds up the others.
//!
//! Sound packets have their own short queue: they are small, go out before waiting pictures and
//! never count as a video backlog. When even they back up, the oldest are dropped.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_screenshare::TEMPORAL_LAYERS;
use tokio::sync::Notify;

use super::wire::{Frame, FrameKind};
use crate::session::lock;

/// Frames waiting for one viewer before it counts as overflowing (about a quarter second).
const CAPACITY: usize = 8;
/// A backlog this long means the viewer does not keep up at its current frame rate.
const LAG_AT: usize = 3;
/// Time between two layer drops, so one burst does not cost every layer at once.
const DROP_PAUSE: Duration = Duration::from_millis(500);
/// How long a viewer has to keep up before it gets the next layer back.
const RAISE_AFTER: Duration = Duration::from_secs(3);
const TOP_LAYER: u8 = TEMPORAL_LAYERS - 1;
/// Sound packets waiting for one viewer (half a second of 20 ms packets).
const AUDIO_CAPACITY: usize = 25;

/// What the encoder has to do after a frame was queued.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Pushed {
    pub need_keyframe: bool,
}

/// The queue and layer choice of one viewer, without any I/O.
pub(crate) struct Policy {
    frames: VecDeque<Arc<Frame>>,
    audio: VecDeque<Arc<Frame>>,
    /// Highest temporal layer this viewer gets.
    max_layer: u8,
    /// After a join or an overflow nothing decodes until the next keyframe.
    wait_keyframe: bool,
    last_drop: Option<Instant>,
    keeping_up_since: Option<Instant>,
}

impl Policy {
    pub fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(CAPACITY),
            audio: VecDeque::with_capacity(AUDIO_CAPACITY),
            max_layer: TOP_LAYER,
            wait_keyframe: true,
            last_drop: None,
            keeping_up_since: None,
        }
    }

    pub fn push(&mut self, frame: Arc<Frame>, now: Instant) -> Pushed {
        if frame.header.kind == FrameKind::Audio {
            if self.audio.len() >= AUDIO_CAPACITY {
                self.audio.pop_front();
            }
            self.audio.push_back(frame);
            return Pushed::default();
        }
        let keyframe = frame.header.keyframe;
        if keyframe {
            // Nothing queued before a keyframe is needed any more.
            self.frames.clear();
            self.wait_keyframe = false;
        } else if self.wait_keyframe || frame.header.temporal_layer > self.max_layer {
            return Pushed::default();
        }
        self.adapt(now);
        if self.frames.len() >= CAPACITY {
            // Far behind: restart at the lowest frame rate and climb back from there.
            self.frames.clear();
            self.wait_keyframe = true;
            self.max_layer = 0;
            self.last_drop = Some(now);
            return Pushed {
                need_keyframe: true,
            };
        }
        self.frames.push_back(frame);
        Pushed::default()
    }

    fn adapt(&mut self, now: Instant) {
        let backlog = self.frames.len();
        if backlog >= LAG_AT {
            self.keeping_up_since = None;
            let paused = self
                .last_drop
                .is_some_and(|at| now.duration_since(at) < DROP_PAUSE);
            if self.max_layer > 0 && !paused {
                self.max_layer -= 1;
                self.last_drop = Some(now);
            }
        } else if backlog <= 1 {
            let since = *self.keeping_up_since.get_or_insert(now);
            if self.max_layer < TOP_LAYER && now.duration_since(since) >= RAISE_AFTER {
                self.max_layer += 1;
                self.keeping_up_since = Some(now);
            }
        }
    }

    pub fn pop(&mut self) -> Option<Arc<Frame>> {
        self.audio.pop_front().or_else(|| self.frames.pop_front())
    }

    pub fn max_layer(&self) -> u8 {
        self.max_layer
    }

    /// Lost at least one layer.
    pub fn lagging(&self) -> bool {
        self.max_layer < TOP_LAYER
    }

    /// Lost every layer it can and still does not keep up: only a lower quality step helps.
    pub fn struggling(&self) -> bool {
        self.max_layer == 0 && (self.wait_keyframe || self.frames.len() >= LAG_AT)
    }
}

/// A viewer's queue shared between the encode thread and the viewer's writer task.
pub(crate) struct ViewerQueue {
    policy: Mutex<Policy>,
    ready: Notify,
    /// Bytes per second for this viewer only (test hook).
    pub limit: Mutex<Option<RateLimit>>,
}

impl ViewerQueue {
    pub fn new() -> Self {
        Self {
            policy: Mutex::new(Policy::new()),
            ready: Notify::new(),
            limit: Mutex::new(None),
        }
    }

    pub fn push(&self, frame: Arc<Frame>, now: Instant) -> Pushed {
        let pushed = lock(&self.policy).push(frame, now);
        self.ready.notify_one();
        pushed
    }

    /// The next frame to send, waiting for one.
    pub async fn next(&self) -> Arc<Frame> {
        loop {
            if let Some(frame) = lock(&self.policy).pop() {
                return frame;
            }
            self.ready.notified().await;
        }
    }

    pub fn with_policy<T>(&self, read: impl FnOnce(&Policy) -> T) -> T {
        read(&lock(&self.policy))
    }
}

/// A token bucket standing in for a slow upload, so tests can throttle without root.
pub(crate) struct RateLimit {
    bytes_per_second: u64,
    /// May go negative: a frame larger than the bucket is sent and paid off afterwards.
    tokens: f64,
    last: Instant,
}

impl RateLimit {
    #[cfg(any(test, feature = "test-backend"))]
    pub fn new(kbps: u32) -> Self {
        let bytes_per_second = u64::from(kbps.max(1)) * 1000 / 8;
        Self {
            bytes_per_second,
            tokens: 0.0,
            last: Instant::now(),
        }
    }

    /// Takes `bytes` and says how long the sender has to wait before sending them.
    pub fn take(&mut self, bytes: usize, now: Instant) -> Duration {
        let rate = self.bytes_per_second as f64;
        let refill = now.duration_since(self.last).as_secs_f64() * rate;
        // At most a tenth of a second of burst.
        self.tokens = (self.tokens + refill).min(rate / 10.0) - bytes as f64;
        self.last = now;
        if self.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-self.tokens / rate)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::wire::{FrameHeader, FrameKind};
    use super::*;

    fn frame(keyframe: bool, temporal_layer: u8) -> Arc<Frame> {
        Arc::new(Frame {
            header: FrameHeader {
                kind: FrameKind::Video,
                keyframe,
                temporal_layer,
                timestamp_us: 0,
                width: 16,
                height: 16,
                payload_len: 1,
            },
            payload: vec![0],
        })
    }

    /// The L1T3 layer of the `n`th frame after a keyframe.
    fn layer(n: usize) -> u8 {
        [0, 2, 1, 2][n % 4]
    }

    #[test]
    fn a_new_viewer_starts_at_a_keyframe() {
        let now = Instant::now();
        let mut policy = Policy::new();
        policy.push(frame(false, 0), now);
        assert!(policy.pop().is_none(), "nothing before the keyframe");
        policy.push(frame(true, 0), now);
        policy.push(frame(false, 2), now);
        assert!(policy.pop().unwrap().header.keyframe);
        assert_eq!(policy.pop().unwrap().header.temporal_layer, 2);
    }

    #[test]
    fn a_slow_viewer_loses_layers_then_gets_them_back() {
        let start = Instant::now();
        let mut policy = Policy::new();
        policy.push(frame(true, 0), start);
        // Nobody drains the queue: layer 2 goes first, layer 1 half a second later.
        let mut now = start;
        for n in 1..6 {
            now = start + Duration::from_millis(33 * n as u64);
            policy.push(frame(false, layer(n)), now);
        }
        assert_eq!(policy.max_layer(), 1);
        assert!(policy.lagging() && !policy.struggling());
        now += DROP_PAUSE;
        policy.push(frame(false, 0), now);
        assert_eq!(policy.max_layer(), 0);
        policy.push(frame(false, 1), now);
        assert_eq!(policy.pop().unwrap().header.temporal_layer, 0);

        // Drained again: one layer back per three seconds of keeping up.
        while policy.pop().is_some() {}
        policy.push(frame(false, 0), now);
        now += RAISE_AFTER;
        policy.pop();
        policy.push(frame(false, 0), now);
        assert_eq!(policy.max_layer(), 1);
        now += RAISE_AFTER;
        policy.pop();
        policy.push(frame(false, 0), now);
        assert_eq!(policy.max_layer(), TOP_LAYER);
        assert!(!policy.lagging());
    }

    #[test]
    fn an_overflow_waits_for_the_next_keyframe() {
        let now = Instant::now();
        let mut policy = Policy::new();
        policy.push(frame(true, 0), now);
        let mut asked = false;
        for _ in 0..CAPACITY + 2 {
            asked |= policy.push(frame(false, 0), now).need_keyframe;
        }
        assert!(asked, "overflow asks for a keyframe");
        assert!(policy.struggling());
        assert_eq!(policy.max_layer(), 0);
        policy.push(frame(false, 0), now);
        assert!(policy.pop().is_none(), "no frames until the keyframe");
        policy.push(frame(true, 0), now);
        assert!(policy.pop().unwrap().header.keyframe);
    }

    fn sound(timestamp_us: u64) -> Arc<Frame> {
        Arc::new(Frame {
            header: FrameHeader {
                kind: FrameKind::Audio,
                keyframe: false,
                temporal_layer: 0,
                timestamp_us,
                width: 0,
                height: 0,
                payload_len: 1,
            },
            payload: vec![0],
        })
    }

    #[test]
    fn sound_goes_first_and_never_counts_as_video_backlog() {
        let now = Instant::now();
        let mut policy = Policy::new();
        policy.push(sound(0), now);
        assert_eq!(
            policy.pop().unwrap().header.kind,
            FrameKind::Audio,
            "before a keyframe too"
        );
        policy.push(frame(true, 0), now);
        for n in 0..AUDIO_CAPACITY + 5 {
            policy.push(sound(n as u64), now);
        }
        assert_eq!(policy.max_layer(), TOP_LAYER);
        assert!(!policy.lagging());
        let first = policy.pop().unwrap();
        assert_eq!(first.header.timestamp_us, 5, "the oldest sound was dropped");
        for _ in 1..AUDIO_CAPACITY {
            assert_eq!(policy.pop().unwrap().header.kind, FrameKind::Audio);
        }
        assert!(policy.pop().unwrap().header.keyframe);
    }

    #[test]
    fn the_rate_limit_spaces_out_sends() {
        let now = Instant::now();
        let mut limit = RateLimit::new(80); // 10 000 bytes per second
        assert_eq!(limit.take(500, now), Duration::from_millis(50));
        let later = now + Duration::from_millis(50);
        assert_eq!(limit.take(0, later), Duration::ZERO);
        let wait = limit.take(10_000, later);
        assert!(wait >= Duration::from_millis(999) && wait <= Duration::from_millis(1001));
    }
}
