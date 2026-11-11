//! The viewer's media path (FR-2.44, FR-7.51, FR-7.59): one reader task per watched share hands
//! every video frame to the [`ScreenFrameSink`]. The UI decodes them with WebCodecs; for each codec
//! the webview cannot decode (AV1 without `av01`, VP9 without `vp09`) a decode thread runs dav1d or
//! libvpx here and hands over pictures instead. The viewer's frame-rate choice drops temporal
//! layers before either decoder sees them. Sound packets go to the share's own source of the
//! playback mix ([`ShareSound`]).

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use mumcord_audio::mixer::share::{ShareSoundStats, ShareStats, ShareVolume};
use mumcord_p2p::quinn::{RecvStream, SendStream};
use mumcord_screenshare::{Av1Decoder, Decoder, I420Frame, VideoDecoder};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use mumcord_p2p::Unit;
use mumcord_p2p::broadcast::ControlKind;
use tokio::sync::mpsc as mpsc_async;

use super::broadcast::link::{MaxFps, Subscriber};
use super::sound::ShareSound;
use super::wire::{self, Codec, Frame, FrameHeader, FrameKind};
use crate::ServerId;
use crate::session::lock;

/// Receives the frames of every watched share. Called from engine tasks and threads: forward the
/// frame and return quickly, since a slow sink holds up that share's stream.
pub trait ScreenFrameSink: Send + Sync {
    /// A video frame as it came over the stream (header and payload), for WebCodecs. Not called
    /// for a codec the watch decodes in Rust.
    fn on_encoded(&self, server_id: ServerId, sharer: u32, frame: &Frame);
    /// A decoded picture of a `codec` frame, only for the codecs the watch decodes in Rust
    /// ([`crate::Engine::screen_share_set_decode_codecs`]).
    fn on_decoded(&self, server_id: ServerId, sharer: u32, codec: Codec, frame: &I420Frame);
}

/// Encoded frames waiting for the decode thread; a full queue drops frames until a keyframe.
const DECODE_QUEUE: usize = 4;

/// A keyframe request that brought no keyframe is repeated after this long. The relay drops a
/// subscriber's requests that come within 2 s of its previous one, and a camera sends keyframes
/// only on request, so a lost request would otherwise leave the picture frozen for good.
const KEYFRAME_RETRY: Duration = Duration::from_millis(2500);

struct Control {
    /// Codecs decoded here instead of in the webview, as [`codec_bit`] flags.
    decode_here: AtomicU8,
    /// Codecs always decoded here (debug test hook for AC-7.41), fixed at the start.
    forced_here: u8,
    /// The viewer's frame-rate cap; 0 means the sharer's rate.
    max_fps: AtomicU32,
    keyframe: Notify,
    /// When the keyframe still awaited was last asked for; `None` while none is awaited.
    asked: Mutex<Option<Instant>>,
}

impl Control {
    fn new(decode_here: u8, forced_here: u8) -> Self {
        Self {
            decode_here: AtomicU8::new(decode_here),
            forced_here,
            max_fps: AtomicU32::new(0),
            keyframe: Notify::new(),
            asked: Mutex::new(None),
        }
    }

    fn request_keyframe(&self) {
        *lock(&self.asked) = Some(Instant::now());
        self.keyframe.notify_one();
    }

    /// A video frame arrived at `now`: a keyframe ends the wait for one, any other frame asks
    /// again once the last request is [`KEYFRAME_RETRY`] old.
    fn frame_arrived(&self, keyframe: bool, now: Instant) {
        let mut asked = lock(&self.asked);
        if keyframe {
            *asked = None;
        } else if asked.is_some_and(|at| now.saturating_duration_since(at) >= KEYFRAME_RETRY) {
            *asked = Some(now);
            self.keyframe.notify_one();
        }
    }

    fn decodes_here(&self, codec: Codec) -> bool {
        (self.decode_here.load(Ordering::Relaxed) | self.forced_here) & codec_bit(codec) != 0
    }
}

fn codec_bit(codec: Codec) -> u8 {
    match codec {
        Codec::Vp9 => 1,
        Codec::Av1 => 2,
    }
}

fn codec_bits(codecs: &[Codec]) -> u8 {
    codecs
        .iter()
        .fold(0, |bits, &codec| bits | codec_bit(codec))
}

/// A share being watched; dropping it closes the stream.
pub(crate) struct WatchLink {
    control: Arc<Control>,
    sound: Arc<ShareStats>,
    volume: Arc<ShareVolume>,
    tasks: [JoinHandle<()>; 2],
    /// The subscriber end of a relay broadcast (spec 07); `None` on a direct stream.
    broadcast: Option<Subscriber>,
}

/// Where a watched share's frames come from.
pub(crate) enum FrameSource {
    /// The direct stream from the sharer, in either wire format.
    Stream(Box<wire::FrameReader<RecvStream>>),
    /// Frames decrypted from the relay broadcast.
    Broadcast(mpsc_async::Receiver<Frame>),
}

impl FrameSource {
    async fn next(&mut self) -> Result<Option<Frame>, wire::WireError> {
        match self {
            Self::Stream(reader) => reader.read_frame().await,
            Self::Broadcast(frames) => Ok(frames.recv().await),
        }
    }
}

/// Where keyframe requests go.
pub(crate) enum RequestSink {
    Stream(SendStream),
    /// The broadcast link: a `kf` control unit, which the relay rate-limits and forwards.
    Broadcast(mpsc_async::Sender<Vec<u8>>),
    /// A stream whose writer the caller shares with another sender (the camera's `want`).
    Shared(mpsc_async::Sender<Vec<u8>>),
}

impl WatchLink {
    /// Plays a direct stream. `tap` records every message of it for the packet inspector
    /// (FR-7.101).
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        rt: &tokio::runtime::Handle,
        server_id: ServerId,
        sharer: u32,
        sink: Option<Arc<dyn ScreenFrameSink>>,
        sound: ShareSound,
        send: SendStream,
        recv: RecvStream,
        tap: Option<crate::inspector::MediaTapSink>,
    ) -> Self {
        Self::start_with(
            rt,
            Source { server_id, sharer },
            sink,
            sound,
            FrameSource::Stream(Box::new(wire::FrameReader::auto(recv).with_tap(tap))),
            RequestSink::Stream(send),
            None,
            None,
        )
    }

    /// Plays a stream the caller opened itself: `frames` is a reader it prepared (the camera
    /// checks the description first), `requests` are the reverse messages it shares with its
    /// `want` reports (FR-8.92, FR-8.93), and `on_end` runs when the stream ends by itself — a
    /// dropped link does not (see [`Drop`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_prepared(
        rt: &tokio::runtime::Handle,
        server_id: ServerId,
        sharer: u32,
        sink: Option<Arc<dyn ScreenFrameSink>>,
        sound: ShareSound,
        frames: FrameSource,
        requests: RequestSink,
        on_end: Option<Box<dyn FnOnce() + Send>>,
    ) -> Self {
        Self::start_with(
            rt,
            Source { server_id, sharer },
            sink,
            sound,
            frames,
            requests,
            None,
            on_end,
        )
    }

    /// Plays a share that arrives through the relay broadcast: `frames` are already decrypted,
    /// keyframe requests go out as `kf` units on `units`, and `subscriber` ends with the link.
    #[allow(clippy::too_many_arguments)]
    pub fn start_broadcast(
        rt: &tokio::runtime::Handle,
        server_id: ServerId,
        sharer: u32,
        sink: Option<Arc<dyn ScreenFrameSink>>,
        sound: ShareSound,
        frames: mpsc_async::Receiver<Frame>,
        units: mpsc_async::Sender<Vec<u8>>,
        subscriber: Subscriber,
    ) -> Self {
        Self::start_with(
            rt,
            Source { server_id, sharer },
            sink,
            sound,
            FrameSource::Broadcast(frames),
            RequestSink::Broadcast(units),
            Some(subscriber),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_with(
        rt: &tokio::runtime::Handle,
        source: Source,
        sink: Option<Arc<dyn ScreenFrameSink>>,
        sound: ShareSound,
        frames: FrameSource,
        requests: RequestSink,
        broadcast: Option<Subscriber>,
        on_end: Option<Box<dyn FnOnce() + Send>>,
    ) -> Self {
        crate::crash::set_context("watching a screen share", "yes");
        let sound_stats = sound.stats();
        let volume = sound.volume();
        let forced_here = if crate::test_hooks::no_webcodecs_av1() {
            codec_bit(Codec::Av1)
        } else {
            0
        };
        let control = Arc::new(Control::new(0, forced_here));
        let reader = rt.spawn(read_frames(
            source,
            sink,
            sound,
            control.clone(),
            frames,
            on_end,
        ));
        let requests = rt.spawn(send_requests(control.clone(), requests));
        Self {
            control,
            sound: sound_stats,
            volume,
            tasks: [reader, requests],
            broadcast,
        }
    }

    /// Decodes frames of `codecs` here from now on; the others go to the webview.
    pub fn set_decode_codecs(&self, codecs: &[Codec]) {
        let bits = codec_bits(codecs);
        if self.control.decode_here.swap(bits, Ordering::Relaxed) != bits {
            // Whichever decoder takes over has to start at a keyframe.
            self.control.request_keyframe();
        }
    }

    pub fn decode_codecs(&self) -> Vec<Codec> {
        let bits = self.control.decode_here.load(Ordering::Relaxed);
        [Codec::Vp9, Codec::Av1]
            .into_iter()
            .filter(|&codec| bits & codec_bit(codec) != 0)
            .collect()
    }

    /// The relay broadcast this share arrives through; `None` on a direct stream.
    pub fn broadcast(&self) -> Option<&Subscriber> {
        self.broadcast.as_ref()
    }

    /// Shows at most about `fps` frames per second (FR-7.51) by dropping temporal layers; `None`
    /// shows every frame the sharer sends. Through the relay the cap also becomes the highest
    /// layer the relay sends, so the dropped layers don't cost download either (FR-7.45).
    pub fn set_max_fps(&self, fps: Option<u32>) {
        self.control
            .max_fps
            .store(fps.unwrap_or(0), Ordering::Relaxed);
        if let Some(broadcast) = &self.broadcast {
            broadcast.set_max_fps(MaxFps::from_fps(fps));
        }
    }

    pub fn max_fps(&self) -> Option<u32> {
        Some(self.control.max_fps.load(Ordering::Relaxed)).filter(|&fps| fps > 0)
    }

    pub fn request_keyframe(&self) {
        self.control.request_keyframe();
    }

    /// Plays the share's sound at `percent` (0–200).
    pub fn set_volume(&self, percent: u16) {
        self.volume.set_percent(percent);
    }

    pub fn sound_stats(&self) -> ShareSoundStats {
        self.sound.snapshot()
    }
}

impl Drop for WatchLink {
    fn drop(&mut self) {
        crate::crash::clear_context("watching a screen share");
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[derive(Clone, Copy)]
struct Source {
    server_id: ServerId,
    sharer: u32,
}

async fn read_frames(
    source: Source,
    sink: Option<Arc<dyn ScreenFrameSink>>,
    mut sound: ShareSound,
    control: Arc<Control>,
    mut frames: FrameSource,
    on_end: Option<Box<dyn FnOnce() + Send>>,
) {
    let mut decoder: Option<DecodeThread> = None;
    let mut layers = LayerFilter::default();
    loop {
        let frame = match frames.next().await {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(e) => {
                tracing::debug!(error = %e, "screen stream from a sharer ended");
                break;
            }
        };
        let FrameKind::Video(codec) = frame.header.kind else {
            sound.packet(&frame.payload);
            continue;
        };
        let Some(sink) = &sink else {
            continue;
        };
        if !layers.keep(&frame.header, control.max_fps.load(Ordering::Relaxed)) {
            continue;
        }
        control.frame_arrived(frame.header.keyframe, Instant::now());
        if control.decodes_here(codec) {
            if decoder.as_ref().is_none_or(|d| d.codec != codec) {
                decoder = Some(DecodeThread::start(
                    source,
                    codec,
                    sink.clone(),
                    control.clone(),
                ));
            }
            if let Some(decoder) = &mut decoder {
                decoder.push(frame, &control);
            }
        } else {
            decoder = None;
            sink.on_encoded(source.server_id, source.sharer, &frame);
        }
    }
    drop(decoder);
    if let Some(on_end) = on_end {
        on_end();
    }
}

/// Frames per window for the frame-rate and top-layer estimate: at least one full layer pattern
/// (8 frames with four layers).
const LAYER_WINDOW: u32 = 16;
/// Slack over the cap before a layer is dropped, so a 30 fps share measured at 31 fps keeps all.
const FPS_SLACK: f64 = 1.15;

/// The viewer's frame-rate choice on the direct path (FR-7.51): drops the top temporal layers
/// until the rate fits the cap. Each layer dropped halves the rate, and the frames left still
/// decode. The sharer's rate and top layer are learnt from the frames over the last two windows,
/// so a changed rate or layer count is followed within about a second.
#[derive(Default)]
struct LayerFilter {
    /// Highest layer and rate (frames per second) seen in the last finished window.
    top: u8,
    fps: Option<f64>,
    window_top: u8,
    window_frames: u32,
    window_start_us: Option<u64>,
    /// Highest layer passed on now.
    max_layer: Option<u8>,
}

impl LayerFilter {
    fn keep(&mut self, header: &FrameHeader, cap: u32) -> bool {
        self.observe(header);
        let layer = header.temporal_layer;
        let top = self.top.max(self.window_top);
        let wanted = match (cap, self.fps) {
            (0, _) | (_, None) => None,
            (cap, Some(fps)) => Some(top.saturating_sub(halvings(fps, cap, top))),
        };
        self.max_layer = match (self.max_layer, wanted) {
            // Raised only at a base-layer frame, the one every upper layer builds on.
            (Some(now), Some(wanted)) if wanted > now && layer != 0 => Some(now),
            (Some(_), None) if layer != 0 => self.max_layer,
            (_, wanted) => wanted,
        };
        self.max_layer.is_none_or(|max| layer <= max)
    }

    fn observe(&mut self, header: &FrameHeader) {
        self.window_top = self.window_top.max(header.temporal_layer);
        self.window_frames += 1;
        let start = *self.window_start_us.get_or_insert(header.timestamp_us);
        if self.window_frames < LAYER_WINDOW {
            return;
        }
        let span_us = header.timestamp_us.saturating_sub(start);
        if span_us > 0 {
            #[expect(clippy::cast_precision_loss, reason = "a window spans seconds at most")]
            let fps = f64::from(LAYER_WINDOW - 1) * 1_000_000.0 / span_us as f64;
            self.fps = Some(fps);
        }
        self.top = self.window_top;
        self.window_top = 0;
        self.window_frames = 0;
        self.window_start_us = None;
    }
}

/// How many top layers to drop so that `fps` fits under `cap`.
fn halvings(fps: f64, cap: u32, top: u8) -> u8 {
    let limit = f64::from(cap) * FPS_SLACK;
    let mut rate = fps;
    let mut dropped = 0;
    while rate > limit && dropped < top {
        rate /= 2.0;
        dropped += 1;
    }
    dropped
}

async fn send_requests(control: Arc<Control>, mut requests: RequestSink) {
    loop {
        control.keyframe.notified().await;
        let sent = match &mut requests {
            RequestSink::Stream(send) => send
                .write_all(&[wire::KEYFRAME_REQUEST])
                .await
                .map_err(|e| e.to_string()),
            RequestSink::Broadcast(units) => match Unit::control(ControlKind::KeyframeRequest, &[])
            {
                Ok(unit) => units.send(unit).await.map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            },
            RequestSink::Shared(messages) => messages
                .send(vec![wire::KEYFRAME_REQUEST])
                .await
                .map_err(|e| e.to_string()),
        };
        if let Err(e) = sent {
            tracing::debug!(error = %e, "screen stream to a sharer ended");
            return;
        }
    }
}

/// dav1d or libvpx decoding off the async runtime, for one codec; it ends when dropped.
struct DecodeThread {
    codec: Codec,
    frames: Option<SyncSender<Frame>>,
    skip_to_keyframe: bool,
}

fn new_decoder(codec: Codec) -> mumcord_screenshare::Result<Box<dyn VideoDecoder>> {
    Ok(match codec {
        Codec::Vp9 => Box::new(Decoder::new()?),
        Codec::Av1 => Box::new(Av1Decoder::new()?),
    })
}

impl DecodeThread {
    fn start(
        source: Source,
        codec: Codec,
        sink: Arc<dyn ScreenFrameSink>,
        control: Arc<Control>,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Frame>(DECODE_QUEUE);
        let spawned = thread::Builder::new()
            .name("screen-decode".into())
            .spawn(move || {
                let mut decoder = match new_decoder(codec) {
                    Ok(decoder) => decoder,
                    Err(e) => {
                        tracing::error!(error = %e, ?codec, "screen share decoder did not start");
                        return;
                    }
                };
                let mut wait_keyframe = true;
                for frame in rx {
                    if wait_keyframe && !frame.header.keyframe {
                        continue;
                    }
                    match decoder.decode(&frame.payload) {
                        Ok(Some(picture)) => {
                            wait_keyframe = false;
                            sink.on_decoded(source.server_id, source.sharer, codec, &picture);
                        }
                        Ok(None) => wait_keyframe = false,
                        Err(e) => {
                            tracing::debug!(error = %e, ?codec, "screen share frame not decoded");
                            wait_keyframe = true;
                            control.request_keyframe();
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "screen share decode thread did not start");
        }
        Self {
            codec,
            frames: Some(tx),
            skip_to_keyframe: true,
        }
    }

    fn push(&mut self, frame: Frame, control: &Control) {
        if self.skip_to_keyframe && !frame.header.keyframe {
            return;
        }
        let Some(frames) = &self.frames else {
            return;
        };
        match frames.try_send(frame) {
            Ok(()) => self.skip_to_keyframe = false,
            Err(TrySendError::Full(_)) => {
                // Decoding falls behind: skip ahead instead of showing ever older pictures.
                self.skip_to_keyframe = true;
                control.request_keyframe();
            }
            Err(TrySendError::Disconnected(_)) => self.frames = None,
        }
    }
}

#[cfg(test)]
mod tests {
    use mumcord_screenshare::{Av1Encoder, Av1Options, EncoderConfig};

    use super::*;

    /// The layer of frame `n` with four temporal layers (0-3-2-3-1-3-2-3).
    fn layer4(n: u64) -> u8 {
        match n % 8 {
            0 => 0,
            4 => 1,
            2 | 6 => 2,
            _ => 3,
        }
    }

    /// The layer of frame `n` with three temporal layers (0-2-1-2).
    fn layer3(n: u64) -> u8 {
        match n % 4 {
            0 => 0,
            2 => 1,
            _ => 2,
        }
    }

    fn header(n: u64, fps: u64, layer: u8) -> FrameHeader {
        FrameHeader {
            kind: FrameKind::Video(Codec::Av1),
            keyframe: n == 0,
            temporal_layer: layer,
            timestamp_us: n * 1_000_000 / fps,
            width: 64,
            height: 64,
            payload_len: 0,
        }
    }

    /// Frames kept out of the second second of a share at `fps` with `layer_of`.
    fn kept(fps: u64, layer_of: fn(u64) -> u8, cap: u32) -> usize {
        let mut filter = LayerFilter::default();
        (0..fps * 2)
            .filter(|&n| filter.keep(&header(n, fps, layer_of(n)), cap) && n >= fps)
            .count()
    }

    #[test]
    fn layers_are_dropped_to_fit_the_cap() {
        assert_eq!(kept(60, layer4, 0), 60);
        assert_eq!(kept(60, layer4, 30), 30);
        assert_eq!(kept(60, layer4, 15), 15);
        assert_eq!(kept(30, layer3, 30), 30);
        assert_eq!(kept(30, layer3, 15), 15);
        assert_eq!(kept(15, layer3, 15), 15);
        // A cap below the base layer keeps the base layer.
        assert_eq!(kept(30, layer3, 1), 7, "frames 32, 36 … 56");
    }

    #[test]
    fn halvings_tolerate_a_slightly_fast_clock() {
        assert_eq!(halvings(31.0, 30, 2), 0);
        assert_eq!(halvings(60.0, 30, 3), 1);
        assert_eq!(halvings(60.0, 15, 3), 2);
        assert_eq!(halvings(60.0, 15, 1), 1, "never more than the top layer");
    }

    #[test]
    fn a_raised_cap_waits_for_a_base_layer_frame() {
        let mut filter = LayerFilter::default();
        let mut n = 0;
        while n < 60 {
            filter.keep(&header(n, 60, layer4(n)), 15);
            n += 1;
        }
        // Frame 60 is layer 1 (60 % 8 == 4): the higher cap does not apply yet.
        assert_eq!(layer4(n), 1);
        assert!(filter.keep(&header(n, 60, 1), 0));
        assert!(!filter.keep(&header(n + 1, 60, 3), 0));
        assert!(filter.keep(&header(64, 60, 0), 0));
        assert!(filter.keep(&header(65, 60, 3), 0));
    }

    #[test]
    fn decodes_here_per_codec_and_with_the_test_hook() {
        let control = Control::new(codec_bits(&[Codec::Vp9]), 0);
        assert!(control.decodes_here(Codec::Vp9));
        assert!(!control.decodes_here(Codec::Av1));
        let forced = Control::new(0, codec_bit(Codec::Av1));
        assert!(forced.decodes_here(Codec::Av1));
        assert!(!forced.decodes_here(Codec::Vp9));
    }

    /// The relay drops a request that comes too soon after the previous one; the camera sends
    /// keyframes only on request. Without a keyframe the request is repeated, never sooner than
    /// the retry time; a keyframe ends it.
    #[tokio::test(start_paused = true)]
    async fn an_unanswered_keyframe_request_is_repeated() {
        let control = Control::new(0, 0);
        control.request_keyframe();
        tokio::time::timeout(Duration::from_millis(1), control.keyframe.notified())
            .await
            .expect("the first request");
        let asked = lock(&control.asked).expect("waiting for a keyframe");
        // The clock is paused, so the timeout only expires once nothing else can run: a stored
        // request answers at once, a missing one times out.
        async fn notified(control: &Control) -> bool {
            tokio::time::timeout(Duration::from_millis(1), control.keyframe.notified())
                .await
                .is_ok()
        }
        control.frame_arrived(false, asked + Duration::from_secs(1));
        assert!(!notified(&control).await, "not before the retry time");
        control.frame_arrived(false, asked + KEYFRAME_RETRY);
        assert!(notified(&control).await, "asked again");
        control.frame_arrived(true, asked + KEYFRAME_RETRY * 2);
        control.frame_arrived(false, asked + KEYFRAME_RETRY * 4);
        assert!(!notified(&control).await, "the keyframe came");
    }

    struct Pictures(Mutex<mpsc::Sender<(Codec, u32, u32)>>);

    impl ScreenFrameSink for Pictures {
        fn on_encoded(&self, _: ServerId, _: u32, _: &Frame) {
            unreachable!("every frame is decoded here");
        }

        fn on_decoded(&self, _: ServerId, _: u32, codec: Codec, frame: &I420Frame) {
            let pictures = self.0.lock().expect("sink lock");
            pictures
                .send((codec, frame.width(), frame.height()))
                .expect("test listens");
        }
    }

    /// AC-7.41: an AV1 share plays without WebCodecs, decoded by dav1d on the decode thread.
    #[test]
    fn av1_frames_round_trip_through_the_engine_decoder() {
        let (width, height, fps) = (320, 192, 30);
        let mut encoder = Av1Encoder::new(
            EncoderConfig {
                width,
                height,
                fps,
                bitrate_kbps: 800,
            },
            Av1Options::default(),
        )
        .expect("SVT-AV1 encoder");
        let (tx, rx) = mpsc::channel();
        let control = Arc::new(Control::new(codec_bit(Codec::Av1), 0));
        let mut thread = DecodeThread::start(
            Source {
                server_id: 1,
                sharer: 7,
            },
            Codec::Av1,
            Arc::new(Pictures(Mutex::new(tx))),
            control.clone(),
        );
        let size = I420Frame::buffer_len(width, height);
        let mut pictures = Vec::new();
        for index in 0..10_u8 {
            let picture =
                I420Frame::new(width, height, vec![index.wrapping_mul(20); size]).expect("picture");
            for out in encoder
                .encode(&picture, i64::from(index) * 33, index == 0)
                .expect("encode")
            {
                let frame = Frame {
                    header: FrameHeader {
                        kind: FrameKind::Video(Codec::Av1),
                        keyframe: out.keyframe,
                        temporal_layer: out.temporal_layer,
                        timestamp_us: u64::from(index) * 33_000,
                        width: 320,
                        height: 192,
                        payload_len: u32::try_from(out.data.len()).expect("small frame"),
                    },
                    payload: out.data,
                };
                thread.push(frame, &control);
            }
            // Wait for the decoder to catch up instead of sleeping a fixed time: the queue is
            // bounded, so a slow machine (a CI VM, for example) would drop frames otherwise.
            let deadline = Instant::now() + Duration::from_secs(10);
            while pictures.len() <= usize::from(index) && Instant::now() < deadline {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(picture) => pictures.push(picture),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        }
        drop(thread);
        pictures.extend(rx.iter());
        assert_eq!(pictures.len(), 10, "decoded {} of 10", pictures.len());
        assert!(pictures.iter().all(|&p| p == (Codec::Av1, width, height)));
    }
}
