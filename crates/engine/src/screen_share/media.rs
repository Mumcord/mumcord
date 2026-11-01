//! The sharer's media path (FR-2.44): capture → one VP9 encoder → every viewer's queue
//! ([`fanout`]) → one writer task per viewer stream.
//!
//! Encoding runs on its own thread, never on the audio thread, and shares no lock with audio.
//! The quality step (1080p30 → 720p30 → 720p15 → 480p15) applies to every viewer, since there is one
//! encoder; it goes down when several viewers lag, when one viewer lost every layer and still
//! does not keep up, or when encoding itself is too slow, and back up once all is well.
//!
//! The shared app's sound has its own thread and Opus encoder (music tuning) and goes into the
//! same viewer queues. Both threads stamp their frames from one clock, the start of the share.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use mumcord_audio::codec::MAX_PACKET_BYTES;
use mumcord_audio::mixer::share::share_encoder;
use mumcord_audio::{FRAME_SAMPLES, SAMPLE_RATE};
use mumcord_p2p::quinn::{RecvStream, SendStream};
use mumcord_screenshare::{EncodedFrame, Encoder, EncoderConfig, I420Frame};
use tokio::task::JoinHandle;

use super::capture::{CaptureSource, SoundSource};
use super::fanout::{RateLimit, ViewerQueue};
use super::thumbnail::ShareThumbnail;
use super::wire::{self, Frame, FrameHeader, FrameKind};
use crate::session::lock;

/// One quality step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Quality {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

/// Best first; the last is the floor of FR-2.44. Full HD by default; smaller sources are never
/// enlarged, and each step keeps the source's aspect ratio (`convert::fit`).
pub(crate) const STEPS: [Quality; 4] = [
    Quality {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_kbps: 4000,
    },
    Quality {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_kbps: 2000,
    },
    Quality {
        width: 1280,
        height: 720,
        fps: 15,
        bitrate_kbps: 1200,
    },
    Quality {
        width: 854,
        height: 480,
        fps: 15,
        bitrate_kbps: 700,
    },
];

/// Pressure has to last this long before quality goes down.
const DOWN_AFTER: Duration = Duration::from_secs(1);
/// At least this long between two steps down, so the viewers' queues can settle in between.
const DOWN_PAUSE: Duration = Duration::from_secs(2);
/// Everything has to be fine this long before quality goes up again.
const UP_AFTER: Duration = Duration::from_secs(5);
/// A step up that goes down again within this time did not fit the upload limit.
const UP_FAILED_WITHIN: Duration = Duration::from_secs(10);
/// Longest wait before retrying a step up that failed before; with the 5 s wait for the step
/// below, recovery after a lifted limit takes 45 s at worst.
const UP_AFTER_MAX: Duration = Duration::from_secs(40);
/// A step up that held this long counts as fitting, and its wait starts from `UP_AFTER` again.
const UP_STABLE: Duration = Duration::from_secs(60);
/// Encoding slower than this share of the frame time leaves no room for capture and sending.
const ENCODE_BUDGET: f64 = 0.8;
/// At most one keyframe per this time on viewer requests; joins always get one.
const KEYFRAME_REQUEST_PAUSE: Duration = Duration::from_secs(1);
/// How long the sound thread waits for sound before it looks at the stop flag again.
const SOUND_WAIT: Duration = Duration::from_millis(50);

/// Spaces out keyframes that viewers ask for, without losing a request: a viewer that waits for
/// a keyframe drops everything else, so a request that came too early is kept, not dropped.
#[derive(Default)]
struct KeyframeGate {
    last: Option<Instant>,
}

impl KeyframeGate {
    /// Whether the next frame has to be a keyframe: always when `forced` (a join, a restart),
    /// on a viewer request only once the pause since the last keyframe is over.
    fn due(&self, forced: bool, requested: bool, now: Instant) -> bool {
        forced
            || (requested
                && self
                    .last
                    .is_none_or(|at| now.duration_since(at) >= KEYFRAME_REQUEST_PAUSE))
    }

    fn sent(&mut self, now: Instant) {
        self.last = Some(now);
    }
}

/// Picks the quality step from what the viewers and the encoder report, without any I/O.
pub(crate) struct Steps {
    step: usize,
    pressure_since: Option<Instant>,
    fine_since: Option<Instant>,
    last_change: Option<Instant>,
    /// How long everything has to be fine before stepping up to each step; doubles when that
    /// step failed right after being reached.
    up_after: [Duration; STEPS.len()],
    /// The latest step up, until it is judged failed or stable.
    up_at: Option<Instant>,
}

/// One look at how sending goes.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Load {
    /// Viewers that lost at least one temporal layer.
    pub lagging: usize,
    /// Viewers that lost every layer and still do not keep up.
    pub struggling: usize,
    /// Encoding takes too much of the frame time.
    pub encoder_busy: bool,
}

impl Steps {
    pub fn new() -> Self {
        Self {
            step: 0,
            pressure_since: None,
            fine_since: None,
            last_change: None,
            up_after: [UP_AFTER; STEPS.len()],
            up_at: None,
        }
    }

    pub fn quality(&self) -> Quality {
        STEPS[self.step]
    }

    pub fn step(&self) -> usize {
        self.step
    }

    /// Takes in the current load; true when the step changed.
    pub fn observe(&mut self, load: Load, now: Instant) -> bool {
        let pressure = load.lagging >= 2 || load.struggling > 0 || load.encoder_busy;
        let fine = load.lagging == 0 && !load.encoder_busy;
        if let Some(at) = self.up_at
            && now.duration_since(at) >= UP_STABLE
        {
            self.up_after[self.step] = UP_AFTER;
            self.up_at = None;
        }
        if pressure {
            self.fine_since = None;
            let since = *self.pressure_since.get_or_insert(now);
            let rested = self
                .last_change
                .is_none_or(|at| now.duration_since(at) >= DOWN_PAUSE);
            if self.step + 1 < STEPS.len() && now.duration_since(since) >= DOWN_AFTER && rested {
                if self
                    .up_at
                    .take()
                    .is_some_and(|at| now.duration_since(at) < UP_FAILED_WITHIN)
                {
                    self.up_after[self.step] = (self.up_after[self.step] * 2).min(UP_AFTER_MAX);
                }
                self.step += 1;
                self.changed(now);
                return true;
            }
        } else {
            self.pressure_since = None;
            if fine {
                let since = *self.fine_since.get_or_insert(now);
                if self.step > 0 && now.duration_since(since) >= self.up_after[self.step - 1] {
                    self.step -= 1;
                    self.up_at = Some(now);
                    self.changed(now);
                    return true;
                }
            } else {
                self.fine_since = None;
            }
        }
        false
    }

    fn changed(&mut self, now: Instant) {
        self.last_change = Some(now);
        self.pressure_since = None;
        self.fine_since = None;
    }
}

/// How the own share is sent right now (test hook, and the numbers behind AC-2.29).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScreenMediaStats {
    /// Index into the quality steps: 0 is 1080p30, 3 is 480p15.
    pub step: usize,
    pub width: u32,
    pub height: u32,
    /// Frames encoded in the last second.
    pub fps: u32,
    /// Encoded kilobits in the last second.
    pub kbps: u32,
    /// Average encode time per frame in milliseconds.
    pub encode_ms: f64,
    pub keyframes: u32,
    pub viewers: Vec<ViewerMediaStats>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewerMediaStats {
    pub session: u32,
    /// Highest temporal layer this viewer gets (2 is the full frame rate).
    pub max_layer: u8,
    /// What that layer means in frames per second (issue #163).
    pub fps: u32,
    /// It lost every layer it can and still does not keep up: the quality step is lowered for it.
    pub struggling: bool,
}

/// What the encode thread, the writer tasks and the engine share.
struct Hub {
    /// Timestamps of both video and sound frames count from here.
    epoch: Instant,
    stop: AtomicBool,
    /// The capture ended on its own ([`CaptureSource::ended`]).
    ended: AtomicBool,
    paused: AtomicBool,
    /// The sharer has sound on (FR-2.45).
    sound: AtomicBool,
    /// The next frame has to be a keyframe.
    keyframe: AtomicBool,
    /// A viewer lost frames and waits for a keyframe ([`KeyframeGate`]).
    keyframe_requested: AtomicBool,
    viewers: Mutex<BTreeMap<u32, Arc<ViewerQueue>>>,
    /// One upload limit for all viewers together (test hook).
    limit: Mutex<Option<RateLimit>>,
    stats: Mutex<ScreenMediaStats>,
    /// The last keyframe the encoder took, for the picture in the sharer's bar (issue #163).
    thumbnail: Mutex<Option<I420Frame>>,
}

impl Hub {
    fn request_keyframe(&self) {
        self.keyframe_requested.store(true, Ordering::Relaxed);
    }

    fn load(&self) -> Load {
        let viewers = lock(&self.viewers);
        let mut load = Load::default();
        for queue in viewers.values() {
            queue.with_policy(|p| {
                load.lagging += usize::from(p.lagging());
                load.struggling += usize::from(p.struggling());
            });
        }
        load
    }
}

/// The running media path of the own share; dropping it stops encoding and every viewer stream.
pub(crate) struct Media {
    hub: Arc<Hub>,
}

impl Media {
    pub fn start(capture: Box<dyn CaptureSource>) -> Self {
        let hub = Arc::new(Hub {
            epoch: Instant::now(),
            stop: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            sound: AtomicBool::new(false),
            keyframe: AtomicBool::new(true),
            keyframe_requested: AtomicBool::new(false),
            viewers: Mutex::new(BTreeMap::new()),
            limit: Mutex::new(None),
            stats: Mutex::new(ScreenMediaStats::default()),
            thumbnail: Mutex::new(None),
        });
        let thread_hub = hub.clone();
        let spawned = thread::Builder::new()
            .name("screen-encode".into())
            .spawn(move || encode_loop(&thread_hub, capture));
        if let Err(e) = spawned {
            tracing::error!(error = %e, "screen share encode thread did not start");
        }
        Self { hub }
    }

    /// Tells this media apart from a later one of the same server.
    pub fn token(&self) -> usize {
        Arc::as_ptr(&self.hub) as usize
    }

    /// The capture ended on its own; nothing more is sent.
    pub fn ended(&self) -> bool {
        self.hub.ended.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        let was = self.hub.paused.swap(paused, Ordering::Relaxed);
        if was && !paused {
            // Viewers may have dropped frames while nothing came; restart cleanly.
            self.hub.keyframe.store(true, Ordering::Relaxed);
        }
    }

    /// Sends `source`'s sound along while sound is on ([`Self::set_sound`]); at most one source.
    pub fn start_sound(&self, source: Box<dyn SoundSource>) {
        let hub = self.hub.clone();
        let spawned = thread::Builder::new()
            .name("screen-sound".into())
            .spawn(move || sound_loop(&hub, source));
        if let Err(e) = spawned {
            tracing::error!(error = %e, "screen share sound thread did not start");
        }
    }

    /// Turns sending the sound on or off (FR-2.45).
    pub fn set_sound(&self, on: bool) {
        self.hub.sound.store(on, Ordering::Relaxed);
    }

    /// Starts sending to `session` over its screen stream; the returned link ends it on drop.
    pub fn add_viewer(
        &self,
        rt: &tokio::runtime::Handle,
        session: u32,
        send: SendStream,
        recv: RecvStream,
    ) -> ViewerLink {
        let queue = Arc::new(ViewerQueue::new());
        lock(&self.hub.viewers).insert(session, queue.clone());
        self.hub.keyframe.store(true, Ordering::Relaxed);
        let writer = rt.spawn(write_frames(queue, self.hub.clone(), send));
        let hub = self.hub.clone();
        let requests = rt.spawn(read_requests(hub, recv));
        ViewerLink {
            hub: self.hub.clone(),
            session,
            tasks: [writer, requests],
        }
    }

    /// The last keyframe as the small picture the sharer's bar shows (issue #163).
    pub fn thumbnail(&self) -> Option<ShareThumbnail> {
        lock(&self.hub.thumbnail)
            .as_ref()
            .and_then(super::thumbnail::thumbnail)
    }

    pub fn stats(&self) -> ScreenMediaStats {
        let mut stats = lock(&self.hub.stats).clone();
        let base_fps = STEPS[stats.step].fps;
        stats.viewers = lock(&self.hub.viewers)
            .iter()
            .map(|(session, queue)| {
                let layer = queue.with_policy(super::fanout::Policy::max_layer);
                ViewerMediaStats {
                    session: *session,
                    max_layer: layer,
                    // Every layer below the top one halves the frame rate (fanout).
                    fps: base_fps >> (super::fanout::TOP_LAYER - layer),
                    struggling: queue.with_policy(super::fanout::Policy::struggling),
                }
            })
            .collect();
        stats
    }

    /// Throttles sending to `viewer`, or to all viewers together with `None` (test hook).
    #[cfg(feature = "test-backend")]
    pub fn set_upload_limit(&self, viewer: Option<u32>, kbps: Option<u32>) {
        let limit = kbps.map(RateLimit::new);
        match viewer {
            None => *lock(&self.hub.limit) = limit,
            Some(session) => {
                if let Some(queue) = lock(&self.hub.viewers).get(&session) {
                    *lock(&queue.limit) = limit;
                }
            }
        }
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        self.hub.stop.store(true, Ordering::Relaxed);
    }
}

/// A viewer being sent to; dropping it closes the viewer's stream.
pub(crate) struct ViewerLink {
    hub: Arc<Hub>,
    session: u32,
    tasks: [JoinHandle<()>; 2],
}

impl Drop for ViewerLink {
    fn drop(&mut self) {
        lock(&self.hub.viewers).remove(&self.session);
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn write_frames(queue: Arc<ViewerQueue>, hub: Arc<Hub>, mut send: SendStream) {
    loop {
        let frame = queue.next().await;
        let bytes = wire::HEADER_LEN + frame.payload.len();
        let now = Instant::now();
        let wait = [&queue.limit, &hub.limit]
            .iter()
            .filter_map(|limit| lock(limit).as_mut().map(|l| l.take(bytes, now)))
            .max()
            .unwrap_or_default();
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        if let Err(e) = wire::write_frame(&mut send, &frame).await {
            tracing::debug!(error = %e, "screen stream to a viewer ended");
            return;
        }
    }
}

/// Reads what the viewer sends back: keyframe requests after it lost frames.
async fn read_requests(hub: Arc<Hub>, mut recv: RecvStream) {
    let mut byte = [0u8; 1];
    loop {
        match recv.read(&mut byte).await {
            Ok(None) => return,
            Ok(Some(_)) if byte[0] == wire::KEYFRAME_REQUEST => hub.request_keyframe(),
            Ok(Some(_)) => {
                tracing::debug!(byte = byte[0], "ignoring an unknown screen stream request")
            }
            Err(e) => {
                tracing::debug!(error = %e, "screen stream from a viewer ended");
                return;
            }
        }
    }
}

/// Frames and bytes of the last second, for [`ScreenMediaStats`].
struct Meter {
    since: Instant,
    frames: u32,
    bytes: usize,
    encode: Duration,
    encoded: u32,
}

impl Meter {
    fn new(now: Instant) -> Self {
        Self {
            since: now,
            frames: 0,
            bytes: 0,
            encode: Duration::ZERO,
            encoded: 0,
        }
    }
}

fn encode_loop(hub: &Hub, mut capture: Box<dyn CaptureSource>) {
    let start = hub.epoch;
    let mut steps = Steps::new();
    let mut encoder: Option<Encoder> = None;
    let mut meter = Meter::new(start);
    let mut keyframes = KeyframeGate::default();
    let mut encode_avg = 0.0_f64;
    let mut next = start;
    while !hub.stop.load(Ordering::Relaxed) {
        if capture.ended() {
            hub.ended.store(true, Ordering::Relaxed);
            return;
        }
        let quality = steps.quality();
        let frame_time = Duration::from_secs(1) / quality.fps;
        next += frame_time;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else {
            // Behind by more than a frame: skip ahead instead of rushing frames out.
            next = now;
        }
        if hub.paused.load(Ordering::Relaxed) {
            continue;
        }
        let Some(picture) = capture.frame(quality.width, quality.height) else {
            continue;
        };
        let size = (picture.width(), picture.height());
        let config = EncoderConfig {
            width: size.0,
            height: size.1,
            fps: quality.fps,
            bitrate_kbps: quality.bitrate_kbps,
        };
        let mut keyframe = keyframes.due(
            hub.keyframe.swap(false, Ordering::Relaxed),
            hub.keyframe_requested.load(Ordering::Relaxed),
            Instant::now(),
        );
        match encoder.as_mut() {
            Some(enc) => match reconfigure(enc, size, quality) {
                Ok(resized) => keyframe |= resized,
                Err(e) => {
                    tracing::warn!(error = %e, "screen share quality change failed");
                    continue;
                }
            },
            None => match Encoder::new(config) {
                Ok(enc) => {
                    encoder = Some(enc);
                    keyframe = true;
                }
                Err(e) => {
                    tracing::error!(error = %e, "screen share encoder did not start");
                    return;
                }
            },
        }
        let Some(enc) = encoder.as_mut() else {
            continue;
        };
        let pts =
            i64::try_from(Instant::now().duration_since(start).as_millis()).unwrap_or(i64::MAX);
        let began = Instant::now();
        let encoded = match enc.encode(&picture, pts, keyframe) {
            Ok(encoded) => encoded,
            Err(e) => {
                tracing::warn!(error = %e, "screen share frame not encoded");
                hub.keyframe.store(true, Ordering::Relaxed);
                continue;
            }
        };
        let took = began.elapsed();
        encode_avg = encode_avg * 0.9 + took.as_secs_f64() * 0.1;
        meter.encode += took;
        meter.encoded += 1;
        let now = Instant::now();
        for out in encoded {
            meter.frames += 1;
            meter.bytes += out.data.len();
            if out.keyframe {
                keyframes.sent(now);
                hub.keyframe_requested.store(false, Ordering::Relaxed);
                lock(&hub.stats).keyframes += 1;
                // The bar shows the last keyframe, so the picture is only the size the encoder was
                // given; the sharer's own client does not decode anything for it.
                lock(&hub.thumbnail).replace(picture.clone());
            }
            fan_out(hub, to_frame(out, size), now);
        }
        let load = Load {
            encoder_busy: encode_avg > frame_time.as_secs_f64() * ENCODE_BUDGET,
            ..hub.load()
        };
        if steps.observe(load, now) {
            let q = steps.quality();
            tracing::info!(
                width = q.width,
                height = q.height,
                fps = q.fps,
                "screen share quality changed"
            );
        }
        let window = now.duration_since(meter.since);
        if window >= Duration::from_secs(1) {
            // The window closes on the first frame after a full second, so at 15 fps it spans 16
            // frames (1.07 s); per second, not per window, or the rates read about 7% high.
            let seconds = window.as_secs_f64();
            let mut stats = lock(&hub.stats);
            stats.step = steps.step();
            (stats.width, stats.height) = size;
            stats.fps = (f64::from(meter.frames) / seconds).round() as u32;
            stats.kbps = (meter.bytes as f64 * 8.0 / 1000.0 / seconds).round() as u32;
            stats.encode_ms = meter.encode.as_secs_f64() * 1000.0 / f64::from(meter.encoded.max(1));
            drop(stats);
            meter = Meter::new(now);
        }
    }
}

/// Moves the encoder to `size` and the step's bitrate; true when the size changed, which needs
/// a keyframe because not every WebCodecs decoder follows a size change on an inter frame.
fn reconfigure(
    enc: &mut Encoder,
    size: (u32, u32),
    quality: Quality,
) -> mumcord_screenshare::Result<bool> {
    let current = enc.config();
    let resized = (current.width, current.height) != size;
    if resized {
        enc.set_resolution(size.0, size.1)?;
    }
    if current.bitrate_kbps != quality.bitrate_kbps {
        enc.set_bitrate(quality.bitrate_kbps)?;
    }
    Ok(resized)
}

fn sound_loop(hub: &Hub, mut source: Box<dyn SoundSource>) {
    let mut encoder = match share_encoder() {
        Ok(encoder) => encoder,
        Err(e) => {
            tracing::error!(error = %e, "screen share sound encoder did not start");
            return;
        }
    };
    let packet_time = Duration::from_micros(
        (encoder.packet_duration().frames() * FRAME_SAMPLES) as u64 * 1_000_000
            / u64::from(SAMPLE_RATE),
    );
    let mut frame = [0.0; FRAME_SAMPLES];
    let mut out = [0u8; MAX_PACKET_BYTES];
    let mut sending = false;
    while !hub.stop.load(Ordering::Relaxed) {
        if !source.next_frame(&mut frame, SOUND_WAIT) {
            continue;
        }
        if !hub.sound.load(Ordering::Relaxed) || hub.paused.load(Ordering::Relaxed) {
            if sending {
                // The next sound is a new stream; no half packet or old codec state in front.
                if let Err(e) = encoder.reset() {
                    tracing::warn!(error = %e, "screen share sound encoder reset failed");
                }
                sending = false;
            }
            continue;
        }
        sending = true;
        let len = match encoder.push_frame(&frame, &mut out) {
            Ok(Some(len)) => len,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(error = %e, "screen share sound not encoded");
                continue;
            }
        };
        let now = Instant::now();
        // The packet's first sample was captured one packet before its last.
        let captured = now.duration_since(hub.epoch).saturating_sub(packet_time);
        fan_out(hub, sound_frame(&out[..len], captured), now);
    }
}

fn sound_frame(packet: &[u8], captured: Duration) -> Frame {
    Frame {
        header: FrameHeader {
            kind: FrameKind::Audio,
            keyframe: false,
            temporal_layer: 0,
            timestamp_us: u64::try_from(captured.as_micros()).unwrap_or(u64::MAX),
            width: 0,
            height: 0,
            payload_len: u32::try_from(packet.len()).unwrap_or(u32::MAX),
        },
        payload: packet.to_vec(),
    }
}

fn fan_out(hub: &Hub, frame: Frame, now: Instant) {
    let frame = Arc::new(frame);
    let viewers: Vec<Arc<ViewerQueue>> = lock(&hub.viewers).values().cloned().collect();
    for queue in viewers {
        if queue.push(frame.clone(), now).need_keyframe {
            hub.request_keyframe();
        }
    }
}

fn to_frame(out: EncodedFrame, (width, height): (u32, u32)) -> Frame {
    let payload_len = u32::try_from(out.data.len()).unwrap_or(u32::MAX);
    Frame {
        header: FrameHeader {
            kind: FrameKind::Video,
            keyframe: out.keyframe,
            temporal_layer: out.temporal_layer,
            timestamp_us: u64::try_from(out.pts).unwrap_or_default() * 1000,
            width: u16::try_from(width).unwrap_or(u16::MAX),
            height: u16::try_from(height).unwrap_or(u16::MAX),
            payload_len,
        },
        payload: out.data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lowest quality step (480p15).
    const FLOOR: usize = STEPS.len() - 1;

    struct Ending;
    impl CaptureSource for Ending {
        fn frame(&mut self, _: u32, _: u32) -> Option<mumcord_screenshare::I420Frame> {
            None
        }
        fn ended(&self) -> bool {
            true
        }
    }

    #[test]
    fn media_reports_a_capture_that_ended() {
        let media = Media::start(Box::new(Ending));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !media.ended() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(media.ended());
    }

    fn load(lagging: usize, struggling: usize) -> Load {
        Load {
            lagging,
            struggling,
            encoder_busy: false,
        }
    }

    #[test]
    fn one_lagging_viewer_keeps_the_quality() {
        let now = Instant::now();
        let mut steps = Steps::new();
        assert!(!steps.observe(load(1, 0), now));
        assert!(!steps.observe(load(1, 0), now + Duration::from_secs(10)));
        assert_eq!(steps.quality(), STEPS[0]);
    }

    #[test]
    fn quality_steps_down_to_the_floor_and_back_up() {
        let start = Instant::now();
        let mut steps = Steps::new();
        let at = |s: u64| start + Duration::from_secs(s);
        assert!(
            !steps.observe(load(2, 0), at(0)),
            "a moment of pressure is not enough"
        );
        assert!(steps.observe(load(2, 0), at(1)));
        assert_eq!(steps.quality(), STEPS[1]);
        assert!(
            !steps.observe(load(2, 0), at(2)),
            "pressure starts counting again"
        );
        assert!(
            steps.observe(load(0, 1), at(3)),
            "one struggling viewer is enough"
        );
        assert_eq!(steps.quality().fps, 15);
        assert!(!steps.observe(load(0, 1), at(5)));
        assert!(steps.observe(load(0, 1), at(6)));
        assert_eq!(steps.quality(), STEPS[FLOOR]);
        assert!(!steps.observe(load(3, 1), at(10)), "the floor holds");
        assert!(!steps.observe(load(3, 1), at(20)));

        assert!(
            !steps.observe(load(1, 0), at(21)),
            "a lagging viewer is not fine yet"
        );
        assert!(!steps.observe(load(0, 0), at(22)));
        assert!(!steps.observe(load(1, 0), at(26)), "lag resets the wait");
        assert!(!steps.observe(load(0, 0), at(27)));
        assert!(steps.observe(load(0, 0), at(32)));
        assert_eq!(steps.step(), FLOOR - 1);
        assert!(
            !steps.observe(load(0, 0), at(33)),
            "each step up waits again"
        );
        assert!(steps.observe(load(0, 0), at(38)));
        assert!(!steps.observe(load(0, 0), at(39)));
        assert!(steps.observe(load(0, 0), at(44)));
        assert_eq!(steps.quality(), STEPS[0]);
    }

    /// Runs the steps once a second against an upload limit that only fits `fits_from` and
    /// below; returns the number of step-ups and the mean fps over `secs`.
    fn simulate(
        steps: &mut Steps,
        start: Instant,
        from: u64,
        secs: u64,
        fits_from: usize,
    ) -> (usize, f64) {
        let (mut ups, mut fps) = (0, 0.0);
        for s in from..from + secs {
            let before = steps.step();
            let pressure = steps.step() < fits_from;
            let l = if pressure { load(2, 0) } else { load(0, 0) };
            steps.observe(l, start + Duration::from_secs(s));
            if steps.step() < before {
                ups += 1;
            }
            fps += f64::from(steps.quality().fps);
        }
        (ups, fps / secs as f64)
    }

    #[test]
    fn a_step_up_that_keeps_failing_is_tried_less_often() {
        let start = Instant::now();
        let mut steps = Steps::new();
        // Full HD and 720p30 never fit, 720p15 does.
        simulate(&mut steps, start, 0, 20, 2);
        let (ups, fps) = simulate(&mut steps, start, 20, 60, 2);
        assert!(ups <= 2, "{ups} tries in 60 s");
        assert!(fps >= 14.0, "{fps} fps");
    }

    #[test]
    fn recovery_after_a_lifted_limit_stays_under_45_seconds() {
        let start = Instant::now();
        let mut steps = Steps::new();
        simulate(&mut steps, start, 0, 200, 1);
        let mut t = 200;
        while steps.step() > 0 {
            simulate(&mut steps, start, t, 1, 0);
            t += 1;
            assert!(t - 200 <= 45, "still at step {} after 45 s", steps.step());
        }
    }

    #[test]
    fn a_step_up_that_held_resets_its_wait() {
        let start = Instant::now();
        let mut steps = Steps::new();
        simulate(&mut steps, start, 0, 200, 1);
        simulate(&mut steps, start, 200, 100, 0);
        assert_eq!(steps.up_after[0], UP_AFTER);
    }

    #[test]
    fn a_keyframe_request_waits_out_the_pause_but_is_kept() {
        let now = Instant::now();
        let mut gate = KeyframeGate::default();
        assert!(gate.due(false, true, now), "the first request is served");
        gate.sent(now);
        assert!(!gate.due(false, true, now + Duration::from_millis(500)));
        assert!(gate.due(true, false, now), "a join always gets one");
        assert!(gate.due(false, true, now + KEYFRAME_REQUEST_PAUSE));
        assert!(!gate.due(false, false, now + KEYFRAME_REQUEST_PAUSE));
    }

    #[test]
    fn a_busy_encoder_lowers_quality() {
        let now = Instant::now();
        let mut steps = Steps::new();
        let busy = Load {
            encoder_busy: true,
            ..Load::default()
        };
        steps.observe(busy, now);
        assert!(steps.observe(busy, now + DOWN_AFTER));
    }

    /// Encodes the test pattern the way the encode loop does: 10 s at 720p30 (long enough for
    /// rate control to settle), 2 s at 720p15, then `seconds` at the floor; the kbps of each
    /// second at the floor. Full HD before them would only make the test slower.
    fn floor_kbps(seconds: u32) -> Vec<usize> {
        let mut capture = super::super::capture::TestPattern::default();
        let mut encoder: Option<Encoder> = None;
        let mut pts = 0_i64;
        let mut per_second = Vec::new();
        for (step, seconds) in [(FLOOR - 2, 10), (FLOOR - 1, 2), (FLOOR, seconds)] {
            let quality = STEPS[step];
            for _ in 0..seconds {
                let mut bytes = 0;
                for _ in 0..quality.fps {
                    let picture = capture
                        .frame(quality.width, quality.height)
                        .expect("pattern");
                    let size = (picture.width(), picture.height());
                    let keyframe = match encoder.as_mut() {
                        Some(enc) => reconfigure(enc, size, quality).expect("reconfigure"),
                        None => {
                            encoder = Some(
                                Encoder::new(EncoderConfig {
                                    width: size.0,
                                    height: size.1,
                                    fps: quality.fps,
                                    bitrate_kbps: quality.bitrate_kbps,
                                })
                                .expect("encoder"),
                            );
                            true
                        }
                    };
                    let enc = encoder.as_mut().expect("encoder");
                    for out in enc.encode(&picture, pts, keyframe).expect("encode") {
                        bytes += out.data.len();
                    }
                    pts += 1000 / i64::from(quality.fps);
                }
                if step == FLOOR {
                    per_second.push(bytes * 8 / 1000);
                }
            }
        }
        per_second
    }

    /// A throttled viewer's upload is sized for the floor's bitrate, so no second, not even the
    /// one right after the step down, may overshoot it by much.
    #[test]
    fn the_floor_stays_near_its_bitrate() {
        let per_second = floor_kbps(10);
        let target = STEPS[FLOOR].bitrate_kbps as usize;
        assert!(
            per_second.iter().all(|kbps| *kbps <= target * 11 / 10),
            "{per_second:?} kbit/s for a target of {target}"
        );
    }
}
