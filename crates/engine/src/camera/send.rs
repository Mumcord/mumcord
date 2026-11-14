//! The camera's capture → effects → encode pipeline (spec 08, FR-8.43 – FR-8.46).
//!
//! The capture backend pushes NV12 frames from its own thread to the background effects thread
//! (`mumcord_effects`, FR-8.38), which hands them on into a small lock-free queue; one encode
//! thread takes them, offers them to the preview ([`PreviewTap`]), scales to the size the viewers
//! want and encodes natural video in L1T3 with the screen-sharing encoder selection. Encoded
//! frames go to the relay publisher ([`CameraFrames`], a [`FrameSource`]).
//!
//! The engine steers the thread only through atomics in [`Control`]: on air or not, how many
//! viewers hold the keys, the largest tile height they want, VP9-only viewers and keyframe
//! requests. The frame path takes no lock and does not log. The encoders allocate their output
//! (as for screen sharing); everything else reuses its buffers.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use crossbeam_queue::ArrayQueue;
use mumcord_camera::{
    CameraBackend, CameraError, CameraFrame, CaptureRequest, CaptureSession, DeviceId, FrameSink,
};
#[cfg(feature = "camera-effects")]
use mumcord_effects::{Effects, EffectsConfig};
use mumcord_p2p::media::Description;
use mumcord_screenshare::{EncoderConfig, I420Frame, VideoEncoder};
use mumcord_store::{CameraSendQuality, ScreenOptimize};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use super::preview::PreviewTap;
use crate::screen_share::EncoderFactory;
use crate::screen_share::ViewerQueue;
use crate::screen_share::broadcast::link::FrameSource;
use crate::screen_share::encoder::monitor::{FrameLoad, LoadMonitor};
use crate::screen_share::wire::{Codec, Frame, FrameHeader, FrameKind};
use crate::session::lock;

/// Tile heights a viewer reports and the sender encodes (FR-8.45).
pub const HEIGHTS: [u32; 4] = [180, 360, 540, 720];

/// The height a viewer that holds the keys but has not reported its tile yet counts with:
/// the room's usual tile; its first `want` follows within a second (FR-8.55).
pub const DEFAULT_WANT: u32 = 360;

/// The encoder changes its size at most this often (FR-8.45).
pub const RESIZE_PAUSE: Duration = Duration::from_secs(2);

/// Hard cap per camera (FR-8.46, relay `camera_max_kbps`).
pub const MAX_KBPS: u32 = 2500;

/// The relay's keyframe requests are honoured at most this often.
const KEYFRAME_REQUEST_PAUSE: Duration = Duration::from_secs(1);

/// Captured frames waiting for the encode thread; older ones are dropped, never queued up.
const CAPTURE_QUEUE: usize = 2;

/// Encoded frames waiting for the publisher.
const SEND_QUEUE: usize = 16;

/// How long the encode thread sleeps when no frame came (it is woken on each frame).
const IDLE_WAIT: Duration = Duration::from_millis(100);

/// Share of a frame's time the encoder may use before it counts as busy (as for screens).
const ENCODE_BUDGET: f64 = 0.8;

/// The upper bound of the send quality setting (FR-8.45).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendCeiling {
    pub height: u32,
    pub fps: u32,
}

impl SendCeiling {
    pub fn from_setting(quality: CameraSendQuality) -> Self {
        let (height, fps) = match quality {
            CameraSendQuality::Auto => (720, 30),
            CameraSendQuality::P720F15 => (720, 15),
            CameraSendQuality::P360F30 => (360, 30),
        };
        Self { height, fps }
    }
}

/// A reported tile height rounded up to the next encoded height (180 … 720).
pub fn round_want(height: u16) -> u32 {
    let height = u32::from(height);
    HEIGHTS
        .into_iter()
        .find(|h| *h >= height)
        .unwrap_or(HEIGHTS[HEIGHTS.len() - 1])
}

/// The height to encode: the largest tile any viewer wants, within the setting.
pub fn target_height(max_want: Option<u16>, ceiling: SendCeiling) -> u32 {
    max_want
        .map_or(DEFAULT_WANT, round_want)
        .min(ceiling.height)
}

/// Bitrate for a size and frame rate (FR-8.46): AV1 / VP9 at 30 fps are 720p 1.2 / 1.5,
/// 540p 0.8 / 1.0, 360p 0.45 / 0.6, 180p 0.18 / 0.225 Mbit/s; 15 fps takes two thirds (the
/// spec's 360p15 0.3 / 0.4 and 180p15 0.12 / 0.15).
pub fn bitrate_kbps(codec: Codec, height: u32, fps: u32) -> u32 {
    let (av1, vp9) = match height {
        h if h > 540 => (1200, 1500),
        h if h > 360 => (800, 1000),
        h if h > 180 => (450, 600),
        _ => (180, 225),
    };
    let kbps = match codec {
        Codec::Av1 => av1,
        Codec::Vp9 => vp9,
    };
    let kbps = if fps <= 15 { kbps * 2 / 3 } else { kbps };
    kbps.min(MAX_KBPS)
}

/// Picture size for an encoded height: 16:9 like every captured frame (FR-8.26), never larger
/// than the capture, widths in eights and even heights as every encoder takes them.
pub fn encode_size(capture: (u32, u32), height: u32) -> (u32, u32) {
    let height = height.min(capture.1) & !1;
    let width = (height * 16 / 9).min(capture.0) & !7;
    (width.max(8), height.max(2))
}

/// When the encoded height follows the viewers: at most once per [`RESIZE_PAUSE`], and only
/// together with a keyframe (the caller forces one when this says so).
#[derive(Debug)]
pub struct Resize {
    current: u32,
    changed: Option<Instant>,
}

impl Resize {
    pub fn new(height: u32) -> Self {
        Self {
            current: height,
            changed: None,
        }
    }

    pub fn current(&self) -> u32 {
        self.current
    }

    /// Moves to `target` if the pause allows; true when the size changed.
    pub fn step(&mut self, target: u32, now: Instant) -> bool {
        if target == self.current
            || self
                .changed
                .is_some_and(|at| now.saturating_duration_since(at) < RESIZE_PAUSE)
        {
            return false;
        }
        self.current = target;
        self.changed = Some(now);
        true
    }
}

/// The stages between capture and encoding.
#[derive(Default)]
pub(crate) struct Stages {
    /// The background effects; `None` passes frames unchanged (and the background can't be
    /// changed while the camera runs).
    #[cfg(feature = "camera-effects")]
    pub effects: Option<EffectsConfig>,
    pub preview: Option<Arc<PreviewTap>>,
}

/// Why the pipeline stopped on its own; read by the stop rules (C-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendEnded {
    /// The camera was unplugged or vanished (FR-8.21 "device lost").
    DeviceGone,
    /// The backend failed.
    CameraFailed,
    /// No encoder could be opened.
    EncoderFailed,
    /// More than 30 frames in a row could not be decoded (FR-8.26).
    Unreadable,
}

impl SendEnded {
    const fn code(self) -> u8 {
        match self {
            Self::DeviceGone => 1,
            Self::CameraFailed => 2,
            Self::EncoderFailed => 3,
            Self::Unreadable => 4,
        }
    }

    const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::DeviceGone),
            2 => Some(Self::CameraFailed),
            3 => Some(Self::EncoderFailed),
            4 => Some(Self::Unreadable),
            _ => None,
        }
    }
}

/// What the engine and the publisher tell the encode thread, and what it reports back.
#[derive(Debug)]
pub(crate) struct Control {
    stop: AtomicBool,
    /// The broadcast is live; before that (preview) nothing is encoded.
    on_air: AtomicBool,
    /// Viewers holding the keys; none pauses the encoder (FR-8.45).
    viewers: AtomicUsize,
    /// Largest wanted tile height, 0 while nobody reported one.
    max_want: AtomicU16,
    /// A viewer that cannot play AV1 is admitted (FR-7.58 for cameras).
    vp9_only: AtomicBool,
    /// The next frame must be a keyframe (a new key, FR-7.44).
    keyframe: AtomicBool,
    /// The relay asked for a keyframe (rate-limited by [`KEYFRAME_REQUEST_PAUSE`]).
    keyframe_requested: AtomicBool,
    /// Frame rate of the camera session, for the stream description (FR-7.95).
    fps: AtomicU32,
    ended: AtomicU8,
    /// The encode thread, woken for each captured frame.
    thread: OnceLock<Thread>,
}

impl Control {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            on_air: AtomicBool::new(false),
            viewers: AtomicUsize::new(0),
            max_want: AtomicU16::new(0),
            vp9_only: AtomicBool::new(false),
            keyframe: AtomicBool::new(true),
            keyframe_requested: AtomicBool::new(false),
            fps: AtomicU32::new(0),
            ended: AtomicU8::new(0),
            thread: OnceLock::new(),
        }
    }

    /// Nothing to encode: not on air yet, or nobody holds the keys.
    fn paused(&self) -> bool {
        !self.on_air.load(Ordering::Relaxed) || self.viewers.load(Ordering::Relaxed) == 0
    }

    fn max_want(&self) -> Option<u16> {
        match self.max_want.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h),
        }
    }

    fn end(&self, why: SendEnded) {
        // The first reason stays.
        let _ = self
            .ended
            .compare_exchange(0, why.code(), Ordering::Relaxed, Ordering::Relaxed);
        self.wake();
    }

    fn wake(&self) {
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
    }
}

/// The capture backend's sink: hands frames to the encode thread without blocking.
struct QueueSink {
    queue: Arc<ArrayQueue<CameraFrame>>,
    control: Arc<Control>,
}

impl FrameSink for QueueSink {
    fn frame(&mut self, frame: CameraFrame) {
        // A full queue drops the oldest frame, whose buffer goes back to the pool.
        drop(self.queue.force_push(frame));
        self.control.wake();
    }

    fn ended(&mut self, reason: CameraError) {
        self.control.end(match reason {
            CameraError::DeviceGone | CameraError::NoCamera => SendEnded::DeviceGone,
            CameraError::Unreadable => SendEnded::Unreadable,
            CameraError::PermissionDenied | CameraError::Backend(_) => SendEnded::CameraFailed,
        });
    }
}

/// The running camera pipeline; dropping it stops the camera (the light goes off) and the
/// encode thread.
pub(crate) struct CameraSend {
    control: Arc<Control>,
    capture: Option<Box<dyn CaptureSession>>,
    #[cfg(feature = "camera-effects")]
    effects: Option<Effects>,
    frames: Option<mpsc::Receiver<Arc<Frame>>>,
    thread: Option<JoinHandle<()>>,
}

impl CameraSend {
    /// Opens `device` and starts the encode thread, which stays idle until
    /// [`Self::set_on_air`] and a viewer.
    pub fn start(
        backend: &dyn CameraBackend,
        device: &DeviceId,
        ceiling: SendCeiling,
        encoders: EncoderFactory,
        stages: Stages,
    ) -> Result<Self, CameraError> {
        let control = Arc::new(Control::new());
        control.fps.store(ceiling.fps, Ordering::Relaxed);
        let queue = Arc::new(ArrayQueue::new(CAPTURE_QUEUE));
        let (tx, rx) = mpsc::channel(SEND_QUEUE);
        let encode = EncodeLoop {
            control: control.clone(),
            queue: queue.clone(),
            ceiling,
            encoders,
            preview: stages.preview,
            out: tx,
        };
        let thread = thread::Builder::new()
            .name("camera-encode".into())
            .spawn(move || encode.run())
            .map_err(|e| CameraError::Backend(format!("encode thread: {e}")))?;
        let _ = control.thread.set(thread.thread().clone());
        let sink: Box<dyn FrameSink> = Box::new(QueueSink {
            queue,
            control: control.clone(),
        });
        // Without the effects thread the camera still works, with the background unchanged.
        #[cfg(feature = "camera-effects")]
        let (effects, sink) = match stages.effects {
            Some(config) => match Effects::start(config, sink) {
                Ok((started, input)) => (Some(started), input),
                Err(e) => {
                    control.stop.store(true, Ordering::Relaxed);
                    control.wake();
                    return Err(CameraError::Backend(format!("effects thread: {e}")));
                }
            },
            None => (None, sink),
        };
        let request = CaptureRequest {
            max_fps: ceiling.fps.max(1),
            ..CaptureRequest::default()
        };
        let capture = match backend.open(device, request, sink) {
            Ok(capture) => capture,
            Err(e) => {
                control.stop.store(true, Ordering::Relaxed);
                control.wake();
                return Err(e);
            }
        };
        Ok(Self {
            control,
            capture: Some(capture),
            #[cfg(feature = "camera-effects")]
            effects,
            frames: Some(rx),
            thread: Some(thread),
        })
    }

    /// Starts or stops encoding for the broadcast.
    pub fn set_on_air(&self, on: bool) {
        self.control.keyframe.store(true, Ordering::Relaxed);
        self.control.on_air.store(on, Ordering::Relaxed);
    }

    /// A VP9-only viewer is admitted (or none is any more); the camera switches codecs with the
    /// next frame.
    pub fn set_vp9_only(&self, vp9_only: bool) {
        self.control.vp9_only.store(vp9_only, Ordering::Relaxed);
    }

    /// The encoded frames on their way to every path; there is one taker.
    pub fn take_frames(&mut self) -> Option<Arc<CameraHub>> {
        let rx = self.frames.take()?;
        Some(Arc::new(CameraHub {
            control: self.control.clone(),
            rx: Mutex::new(Some(rx)),
            relay: Mutex::new(None),
            direct: Mutex::new(BTreeMap::new()),
        }))
    }

    /// The background effects of this camera, when they run.
    #[cfg(feature = "camera-effects")]
    pub fn effects(&self) -> Option<&Effects> {
        self.effects.as_ref()
    }

    /// Why the pipeline stopped on its own, if it did.
    pub fn ended(&self) -> Option<SendEnded> {
        SendEnded::from_code(self.control.ended.load(Ordering::Relaxed))
    }
}

impl Drop for CameraSend {
    fn drop(&mut self) {
        // The capture first: its drop joins the capture thread and turns the camera light off.
        // The effects thread then sees no more frames and stops when its handle drops.
        self.capture.take();
        #[cfg(feature = "camera-effects")]
        self.effects.take();
        self.control.stop.store(true, Ordering::Relaxed);
        self.control.wake();
        // Not joined: an encode in progress finishes on its own within a frame time.
        self.thread.take();
    }
}

/// The publisher's frame source (relay broadcast, FR-8.42).
pub(crate) struct CameraFrames {
    queue: Arc<ViewerQueue>,
    hub: Arc<CameraHub>,
    /// The camera session id, carried in the stream description (FR-8.92).
    session: String,
}

impl FrameSource for CameraFrames {
    async fn next(&self) -> Arc<Frame> {
        self.queue.next().await
    }

    fn request_keyframe(&self) {
        self.hub.request_keyframe();
    }

    fn force_keyframe(&self) {
        self.hub.force_keyframe();
    }

    fn viewers_changed(&self, viewers: usize, max_want: Option<u16>) {
        self.hub.relay_viewers_changed(viewers, max_want);
    }

    fn description(&self, video: &FrameHeader) -> Option<Description> {
        // The same description the direct streams send, so a relay viewer can tell this camera
        // session from an older one (FR-8.92, AC-8.34).
        super::direct::stream_description(*video, self.hub.fps(), &self.session)
    }
}

/// One consumer of the encoded frames, with the tile height it last asked for (camera `want`,
/// FR-8.45).
struct Consumer {
    queue: Arc<ViewerQueue>,
    want: Option<u16>,
}

/// The relay publisher's queue and what its control task reports about it.
struct Relay {
    queue: Arc<ViewerQueue>,
    /// Viewers that hold the share keys.
    viewers: usize,
    /// Largest tile height those viewers want.
    want: Option<u16>,
}

/// Fans the encoded frames out to the relay publisher and every direct viewer stream, and tells
/// the encode thread how many viewers hold the keys and the largest tile any of them wants
/// (FR-8.45, FR-8.93). One encoder and one frame queue feed every path of a camera.
pub(crate) struct CameraHub {
    control: Arc<Control>,
    /// The encode thread's frames; the pump takes it once.
    rx: Mutex<Option<mpsc::Receiver<Arc<Frame>>>>,
    relay: Mutex<Option<Relay>>,
    /// One queue per direct viewer stream (FR-8.92).
    direct: Mutex<BTreeMap<u32, Consumer>>,
}

impl CameraHub {
    /// Hands every encoded frame to all queues until the encode thread ends. Called once, when
    /// the camera goes on air.
    pub(crate) fn pump(self: &Arc<Self>, rt: &Handle) {
        let Some(mut rx) = lock(&self.rx).take() else {
            return;
        };
        let hub = self.clone();
        rt.spawn(async move {
            while let Some(frame) = rx.recv().await {
                hub.distribute(frame);
            }
        });
    }

    fn distribute(&self, frame: Arc<Frame>) {
        let top = frame.header.temporal_layer;
        let now = Instant::now();
        let mut need_keyframe = false;
        if let Some(relay) = lock(&self.relay).as_ref() {
            need_keyframe |= relay.queue.push(frame.clone(), top, now).need_keyframe;
        }
        for consumer in lock(&self.direct).values() {
            need_keyframe |= consumer.queue.push(frame.clone(), top, now).need_keyframe;
        }
        if need_keyframe {
            self.request_keyframe();
        }
    }

    /// The relay publisher's source; it takes the share once, like one more viewer. Calling it
    /// again returns a source on the same queue, so the viewer counts stay. `session` is the
    /// camera session id the stream description carries (FR-8.92).
    pub(crate) fn relay(self: &Arc<Self>, session: &str) -> CameraFrames {
        let queue = {
            let mut relay = lock(&self.relay);
            match relay.as_mut() {
                Some(relay) => relay.queue.clone(),
                None => {
                    let queue = Arc::new(ViewerQueue::new());
                    *relay = Some(Relay {
                        queue: queue.clone(),
                        viewers: 0,
                        want: None,
                    });
                    queue
                }
            }
        };
        self.update_viewers();
        CameraFrames {
            queue,
            hub: self.clone(),
            session: session.to_owned(),
        }
    }

    /// A queue for a direct viewer stream, or `None` when that viewer already has one.
    pub(crate) fn add_direct(&self, session: u32) -> Option<Arc<ViewerQueue>> {
        let queue = Arc::new(ViewerQueue::new());
        let previous = lock(&self.direct).insert(
            session,
            Consumer {
                queue: queue.clone(),
                want: None,
            },
        );
        if previous.is_some() {
            return None;
        }
        self.update_viewers();
        Some(queue)
    }

    pub(crate) fn remove_direct(&self, session: u32) {
        let removed = lock(&self.direct).remove(&session).is_some();
        if removed {
            self.update_viewers();
        }
    }

    /// The tile height one direct viewer asked for (camera `want`, FR-8.93).
    pub(crate) fn set_direct_want(&self, session: u32, height: u16) {
        let changed = {
            let mut direct = lock(&self.direct);
            match direct.get_mut(&session) {
                Some(consumer) if consumer.want != Some(height) => {
                    consumer.want = Some(height);
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.update_viewers();
        }
    }

    pub(crate) fn fps(&self) -> u32 {
        self.control.fps.load(Ordering::Relaxed)
    }

    pub(crate) fn request_keyframe(&self) {
        self.control
            .keyframe_requested
            .store(true, Ordering::Relaxed);
    }

    pub(crate) fn force_keyframe(&self) {
        self.control.keyframe.store(true, Ordering::Relaxed);
    }

    fn relay_viewers_changed(&self, viewers: usize, max_want: Option<u16>) {
        if let Some(relay) = lock(&self.relay).as_mut() {
            relay.viewers = viewers;
            relay.want = max_want;
        }
        self.update_viewers();
    }

    /// Tells the encode thread what every path together needs: no key holder pauses the encoder,
    /// and the largest wanted tile decides the height (FR-8.45).
    fn update_viewers(&self) {
        let relay = lock(&self.relay)
            .as_ref()
            .map_or((0, None), |r| (r.viewers, r.want));
        let direct = lock(&self.direct);
        let want = direct.values().filter_map(|c| c.want).chain(relay.1).max();
        self.control
            .viewers
            .store(relay.0 + direct.len(), Ordering::Relaxed);
        self.control
            .max_want
            .store(want.unwrap_or(0), Ordering::Relaxed);
    }
}

/// The encoder that runs now.
struct Running {
    encoder: Box<dyn VideoEncoder>,
    software_av1: bool,
}

struct EncodeLoop {
    control: Arc<Control>,
    queue: Arc<ArrayQueue<CameraFrame>>,
    ceiling: SendCeiling,
    encoders: EncoderFactory,
    preview: Option<Arc<PreviewTap>>,
    out: mpsc::Sender<Arc<Frame>>,
}

impl EncodeLoop {
    fn run(self) {
        let control = self.control.clone();
        let mut resize = Resize::new(target_height(None, self.ceiling));
        let mut running: Option<Running> = None;
        // AV1 stays possible until no AV1 encoder opens or software AV1 cannot keep up.
        let mut av1_possible = self.encoders.offers_av1;
        let mut monitor = LoadMonitor::default();
        let mut picture: Vec<u8> = Vec::with_capacity(I420Frame::buffer_len(
            mumcord_camera::MAX_WIDTH,
            mumcord_camera::MAX_HEIGHT,
        ));
        let frame_time = Duration::from_secs(1) / self.ceiling.fps.max(1);
        let mut last_sent: Option<Duration> = None;
        let mut last_keyframe: Option<Instant> = None;
        let mut encode_avg = 0.0_f64;
        let mut paused = true;
        while !control.stop.load(Ordering::Relaxed) {
            if control.ended.load(Ordering::Relaxed) != 0 {
                return;
            }
            let Some(frame) = self.queue.pop() else {
                thread::park_timeout(IDLE_WAIT);
                continue;
            };
            if let Some(preview) = &self.preview {
                preview.offer(&frame);
            }
            if control.paused() {
                paused = true;
                continue;
            }
            // Below the camera's rate (720p15): skip frames that come too soon. A small margin
            // keeps capture jitter from dropping every other wanted frame.
            if let Some(last) = last_sent
                && frame.timestamp.saturating_sub(last) < frame_time.mul_f32(0.8)
            {
                continue;
            }
            let now = Instant::now();
            let requested = control.keyframe_requested.load(Ordering::Relaxed)
                && last_keyframe
                    .is_none_or(|at| now.saturating_duration_since(at) >= KEYFRAME_REQUEST_PAUSE);
            let mut keyframe =
                control.keyframe.swap(false, Ordering::Relaxed) || requested || paused;
            paused = false;
            keyframe |= resize.step(target_height(control.max_want(), self.ceiling), now);
            let size = encode_size((frame.width, frame.height), resize.current());
            let want_av1 = av1_possible && !control.vp9_only.load(Ordering::Relaxed);
            // A new size opens a new encoder: libvpx corrupts its heap when a running VP9
            // encoder grows beyond the size it was opened at. It happens at most every 2 s and
            // starts with the keyframe the size change needs anyway.
            if running.as_ref().is_some_and(|r| {
                (r.encoder.codec() == Codec::Av1) != want_av1 || {
                    let config = r.encoder.config();
                    (config.width, config.height) != size
                }
            }) {
                running = None;
            }
            if running.is_none() {
                match self.open(size, want_av1) {
                    Some(opened) => {
                        if want_av1 && opened.encoder.codec() != Codec::Av1 {
                            // Every AV1 encoder failed to open; stay on VP9.
                            av1_possible = false;
                        }
                        running = Some(opened);
                        monitor.reset();
                        encode_avg = 0.0;
                        keyframe = true;
                    }
                    None => {
                        control.end(SendEnded::EncoderFailed);
                        return;
                    }
                }
            }
            let Some(run) = running.as_mut() else {
                continue;
            };
            picture.resize(I420Frame::buffer_len(size.0, size.1), 0);
            nv12_to_i420(&frame.data, (frame.width, frame.height), &mut picture, size);
            let pts = i64::try_from(frame.timestamp.as_millis()).unwrap_or(i64::MAX);
            last_sent = Some(frame.timestamp);
            // The capture buffer goes back to the pool before the encode.
            drop(frame);
            let Ok(i420) = I420Frame::new(size.0, size.1, std::mem::take(&mut picture)) else {
                continue;
            };
            let began = Instant::now();
            let encoded = run.encoder.encode(&i420, pts, keyframe);
            let took = began.elapsed();
            picture = i420.into_data();
            let Ok(encoded) = encoded else {
                control.keyframe.store(true, Ordering::Relaxed);
                continue;
            };
            let codec = run.encoder.codec();
            for out in encoded {
                if out.keyframe {
                    last_keyframe = Some(Instant::now());
                    control.keyframe_requested.store(false, Ordering::Relaxed);
                }
                let frame = to_frame(out, size, codec);
                if self.out.try_send(Arc::new(frame)).is_err() {
                    // The upload lags; viewers would miss a reference, so start over cleanly.
                    control.keyframe.store(true, Ordering::Relaxed);
                }
            }
            encode_avg = encode_avg * 0.9 + took.as_secs_f64() * 0.1;
            let load = FrameLoad {
                encode: took,
                budget: frame_time,
                busy: encode_avg > frame_time.as_secs_f64() * ENCODE_BUDGET,
                dropped: 0,
            };
            if run.software_av1 && monitor.frame(load, Instant::now()).is_some() {
                // Software AV1 can't keep up: VP9 for the rest of this camera session (FR-7.57).
                av1_possible = false;
                running = None;
            }
        }
    }

    fn open(&self, (width, height): (u32, u32), av1: bool) -> Option<Running> {
        let codec = if av1 { Codec::Av1 } else { Codec::Vp9 };
        let config = EncoderConfig {
            width,
            height,
            fps: self.ceiling.fps,
            bitrate_kbps: bitrate_kbps(codec, height, self.ceiling.fps),
        };
        match (self.encoders.open)(config, ScreenOptimize::Motion, av1) {
            Ok((mut encoder, active)) => {
                if active.codec != codec {
                    // The selection fell through to VP9; its bitrate differs.
                    let kbps = bitrate_kbps(active.codec, height, self.ceiling.fps);
                    if encoder.set_bitrate(kbps).is_err() {
                        return None;
                    }
                }
                tracing::info!(?active, width, height, "camera encoder opened");
                Some(Running {
                    software_av1: active.codec == Codec::Av1 && active.gpu.is_none(),
                    encoder,
                })
            }
            Err(e) => {
                tracing::error!(error = %e, "no camera encoder opened");
                None
            }
        }
    }
}

fn to_frame(
    out: mumcord_screenshare::EncodedFrame,
    (width, height): (u32, u32),
    codec: Codec,
) -> Frame {
    Frame {
        header: FrameHeader {
            kind: FrameKind::Video(codec),
            keyframe: out.keyframe,
            temporal_layer: out.temporal_layer,
            timestamp_us: u64::try_from(out.pts).unwrap_or_default() * 1000,
            width: u16::try_from(width).unwrap_or(u16::MAX),
            height: u16::try_from(height).unwrap_or(u16::MAX),
            payload_len: u32::try_from(out.data.len()).unwrap_or(u32::MAX),
        },
        payload: out.data,
    }
}

/// Scales an NV12 picture (`src`, no row padding) into I420 at `dst_size` with a box filter:
/// each output pixel is the mean of the source pixels it covers. Never allocates.
pub fn nv12_to_i420(src: &[u8], src_size: (u32, u32), dst: &mut [u8], dst_size: (u32, u32)) {
    let (sw, sh) = (src_size.0 as usize, src_size.1 as usize);
    let (dw, dh) = (dst_size.0 as usize, dst_size.1 as usize);
    let (y_src, uv_src) = src.split_at(sw * sh);
    let (y_dst, chroma) = dst.split_at_mut(dw * dh);
    let (u_dst, v_dst) = chroma.split_at_mut(dw / 2 * (dh / 2));
    let luma = Plane {
        data: y_src,
        width: sw,
        height: sh,
        stride: sw,
        pitch: 1,
    };
    scale_plane(&luma, 0, y_dst, dw, dh);
    let chroma = Plane {
        data: uv_src,
        width: sw / 2,
        height: sh / 2,
        stride: sw,
        pitch: 2,
    };
    scale_plane(&chroma, 0, u_dst, dw / 2, dh / 2);
    scale_plane(&chroma, 1, v_dst, dw / 2, dh / 2);
}

/// One source plane; `pitch` is the byte step between pixels (2 for interleaved UV).
struct Plane<'a> {
    data: &'a [u8],
    width: usize,
    height: usize,
    stride: usize,
    pitch: usize,
}

fn scale_plane(src: &Plane<'_>, offset: usize, dst: &mut [u8], dw: usize, dh: usize) {
    if dw == 0 || dh == 0 || src.width == 0 || src.height == 0 {
        return;
    }
    for (dy, row) in dst.chunks_exact_mut(dw).take(dh).enumerate() {
        let y0 = dy * src.height / dh;
        let y1 = ((dy + 1) * src.height / dh).max(y0 + 1);
        for (dx, out) in row.iter_mut().enumerate() {
            let x0 = dx * src.width / dw;
            let x1 = ((dx + 1) * src.width / dw).max(x0 + 1);
            let mut sum = 0u32;
            for y in y0..y1 {
                let line = y * src.stride + offset;
                for x in x0..x1 {
                    sum += u32::from(src.data.get(line + x * src.pitch).copied().unwrap_or(0));
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u32;
            *out = (sum / count) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use mumcord_camera::{TestCamera, TestCameraMode};

    use super::*;
    use crate::screen_share::encoder::natural_factory;
    use crate::screen_share::encoder::select::{Candidate, Kind};

    #[test]
    fn the_send_quality_setting_is_the_ceiling() {
        let auto = SendCeiling::from_setting(CameraSendQuality::Auto);
        assert_eq!((auto.height, auto.fps), (720, 30));
        let p360 = SendCeiling::from_setting(CameraSendQuality::P360F30);
        assert_eq!((p360.height, p360.fps), (360, 30));
        assert_eq!(
            SendCeiling::from_setting(CameraSendQuality::P720F15).fps,
            15
        );
        // The largest wanted tile, rounded up to the next step and capped by the setting.
        assert_eq!(target_height(Some(700), auto), 720);
        assert_eq!(target_height(Some(400), auto), 540);
        assert_eq!(target_height(Some(180), auto), 180);
        assert_eq!(target_height(Some(1), auto), 180);
        assert_eq!(target_height(Some(2000), auto), 720);
        assert_eq!(target_height(Some(720), p360), 360);
        assert_eq!(target_height(None, auto), DEFAULT_WANT);
    }

    #[test]
    fn bitrates_follow_the_table_and_the_cap() {
        assert_eq!(bitrate_kbps(Codec::Av1, 720, 30), 1200);
        assert_eq!(bitrate_kbps(Codec::Vp9, 720, 30), 1500);
        assert_eq!(bitrate_kbps(Codec::Av1, 540, 30), 800);
        assert_eq!(bitrate_kbps(Codec::Vp9, 360, 30), 600);
        assert_eq!(bitrate_kbps(Codec::Av1, 360, 15), 300);
        assert_eq!(bitrate_kbps(Codec::Vp9, 360, 15), 400);
        assert_eq!(bitrate_kbps(Codec::Av1, 180, 15), 120);
        assert_eq!(bitrate_kbps(Codec::Vp9, 180, 15), 150);
        for h in HEIGHTS {
            for codec in [Codec::Av1, Codec::Vp9] {
                assert!(bitrate_kbps(codec, h, 30) <= MAX_KBPS);
            }
        }
    }

    #[test]
    fn sizes_are_16_9_within_the_capture() {
        assert_eq!(encode_size((1280, 720), 720), (1280, 720));
        assert_eq!(encode_size((1280, 720), 540), (960, 540));
        assert_eq!(encode_size((1280, 720), 360), (640, 360));
        assert_eq!(encode_size((1280, 720), 180), (320, 180));
        assert_eq!(encode_size((640, 360), 720), (640, 360), "never enlarged");
    }

    #[test]
    fn the_size_changes_at_most_every_two_seconds() {
        let t0 = Instant::now();
        let mut resize = Resize::new(360);
        assert!(!resize.step(360, t0));
        assert!(resize.step(720, t0), "the first change is immediate");
        assert_eq!(resize.current(), 720);
        assert!(!resize.step(180, t0 + Duration::from_millis(1999)));
        assert_eq!(resize.current(), 720);
        assert!(resize.step(180, t0 + RESIZE_PAUSE));
        assert_eq!(resize.current(), 180);
    }

    #[test]
    fn scaling_keeps_flat_colour_and_halves_by_averaging() {
        let (w, h) = (8u32, 4u32);
        let mut nv12 = vec![0u8; mumcord_camera::nv12_len(w, h)];
        // Luma: columns alternate 0 and 200; chroma: U 100, V 50.
        for (i, px) in nv12[..32].iter_mut().enumerate() {
            *px = if i % 2 == 0 { 0 } else { 200 };
        }
        for pair in nv12[32..].as_chunks_mut::<2>().0 {
            *pair = [100, 50];
        }
        let mut same = vec![0u8; I420Frame::buffer_len(w, h)];
        nv12_to_i420(&nv12, (w, h), &mut same, (w, h));
        assert_eq!(&same[..32], &nv12[..32], "1:1 copies luma");
        assert!(same[32..40].iter().all(|u| *u == 100));
        assert!(same[40..].iter().all(|v| *v == 50));
        let mut half = vec![0u8; I420Frame::buffer_len(4, 2)];
        nv12_to_i420(&nv12, (w, h), &mut half, (4, 2));
        assert!(half[..8].iter().all(|y| *y == 100), "2×2 boxes average");
        assert_eq!(&half[8..], &[100, 100, 50, 50]);
    }

    fn vp9_only() -> EncoderFactory {
        natural_factory(
            vec![Candidate {
                codec: Codec::Vp9,
                kind: Kind::Software,
            }],
            Arc::default(),
        )
    }

    #[cfg(feature = "vp9")]
    fn wait_for(rt: &tokio::runtime::Runtime, frames: &CameraFrames, what: &str) -> Arc<Frame> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let waited = rt.block_on(async {
                tokio::time::timeout(Duration::from_millis(20), frames.next()).await
            });
            if let Ok(frame) = waited {
                return frame;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
        }
    }

    /// Whether no frame comes within `wait`.
    #[cfg(feature = "vp9")]
    fn nothing(rt: &tokio::runtime::Runtime, frames: &CameraFrames, wait: Duration, what: &str) {
        let waited = rt.block_on(async { tokio::time::timeout(wait, frames.next()).await });
        assert!(waited.is_err(), "unexpected frames: {what}");
    }

    // Opens a real VP9 encoder.
    #[cfg(feature = "vp9")]
    #[test]
    fn the_test_camera_is_encoded_only_on_air_with_viewers_and_at_the_wanted_size() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        let camera = TestCamera::new(TestCameraMode::Present);
        let ceiling = SendCeiling::from_setting(CameraSendQuality::Auto);
        let mut send = CameraSend::start(
            &camera,
            &TestCamera::device_id(),
            ceiling,
            vp9_only(),
            Stages::default(),
        )
        .expect("test camera opens");
        let hub = send.take_frames().expect("one taker");
        hub.pump(&rt.handle().clone());
        let frames = hub.relay("cam");
        // Preview: nothing is encoded.
        nothing(&rt, &frames, Duration::from_millis(300), "in the preview");
        // On air without viewers: still paused.
        send.set_on_air(true);
        nothing(
            &rt,
            &frames,
            Duration::from_millis(300),
            "paused without viewers",
        );
        // A viewer with the keys: frames come, the first a keyframe at the default height.
        hub.relay("cam").viewers_changed(1, None);
        let first = wait_for(&rt, &frames, "the first frame");
        assert!(first.header.keyframe);
        assert_eq!(first.header.kind, FrameKind::Video(Codec::Vp9));
        assert_eq!((first.header.width, first.header.height), (640, 360));
        // A viewer wants 720: the next size is 1280×720, starting with a keyframe.
        hub.relay("cam").viewers_changed(1, Some(720));
        let big = loop {
            let f = wait_for(&rt, &frames, "a 720p frame");
            if f.header.height == 720 {
                break f;
            }
        };
        assert!(big.header.keyframe, "the size changes at a keyframe");
        assert_eq!(big.header.width, 1280);
        // A direct viewer that wants a larger tile counts too (FR-8.93).
        let queue = hub.add_direct(7).expect("a direct viewer queue");
        hub.set_direct_want(7, 540);
        assert_eq!(hub.control.max_want.load(Ordering::Relaxed), 720);
        hub.remove_direct(7);
        drop(queue);
        // The last viewer leaves: the encoder pauses.
        hub.relay("cam").viewers_changed(0, None);
        rt.block_on(async {
            let until = tokio::time::Instant::now() + Duration::from_millis(300);
            while let Ok(Some(_)) = tokio::time::timeout_at(until, frames.next())
                .await
                .map(Some)
            {}
        });
        nothing(
            &rt,
            &frames,
            Duration::from_millis(300),
            "paused again without viewers",
        );
        assert_eq!(send.ended(), None);
    }

    #[cfg(feature = "camera-effects")]
    #[test]
    fn the_preview_gets_pictures_through_the_effects_before_the_camera_is_on_air() {
        use mumcord_effects::Background;
        use mumcord_store::CameraEffectQuality;

        let camera = TestCamera::new(TestCameraMode::Present);
        let ceiling = SendCeiling::from_setting(CameraSendQuality::Auto);
        let tap = Arc::new(PreviewTap::default());
        let (tx, rx) = std::sync::mpsc::channel();
        tap.set_sink(Some(Box::new(
            move |p: super::super::PreviewPicture<'_>| {
                let _ = tx.send((p.width, p.height, p.rgba.len()));
            },
        )));
        let stages = Stages {
            effects: Some(EffectsConfig {
                fps: ceiling.fps,
                quality: CameraEffectQuality::Auto,
                background: Background::None,
                on_battery: Arc::new(|| false),
            }),
            preview: Some(tap.clone()),
        };
        let send = CameraSend::start(
            &camera,
            &TestCamera::device_id(),
            ceiling,
            vp9_only(),
            stages,
        )
        .expect("test camera opens");
        let (w, h, len) = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a preview picture");
        assert_eq!((w, h, len), (640, 360, 640 * 360 * 4));
        assert!(send.effects().is_some());
        // Without a sink nothing is converted any more.
        tap.set_sink(None);
        drop(send);
        while rx.try_recv().is_ok() {}
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn a_refused_camera_reports_the_error() {
        let camera = TestCamera::new(TestCameraMode::Denied);
        let opened = CameraSend::start(
            &camera,
            &TestCamera::device_id(),
            SendCeiling::from_setting(CameraSendQuality::Auto),
            vp9_only(),
            Stages::default(),
        );
        assert_eq!(opened.err(), Some(CameraError::PermissionDenied));
    }
}
