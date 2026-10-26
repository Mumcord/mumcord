//! The viewer's media path (FR-2.44): one reader task per watched share hands every video frame
//! to the [`ScreenFrameSink`]. The UI decodes them with WebCodecs; only when it switches a watch
//! to the fallback does a decode thread run libvpx here and hand over pictures instead. Sound
//! packets go to the share's own source of the playback mix ([`ShareSound`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;

use mumcord_audio::mixer::share::{ShareSoundStats, ShareStats};
use mumcord_p2p::quinn::{RecvStream, SendStream};
use mumcord_screenshare::{Decoder, I420Frame};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::sound::ShareSound;
use super::wire::{self, Frame, FrameKind};
use crate::ServerId;

/// Receives the frames of every watched share. Called from engine tasks and threads: forward the
/// frame and return quickly, since a slow sink holds up that share's stream.
pub trait ScreenFrameSink: Send + Sync {
    /// A VP9 frame as it came over the stream (header and payload), for WebCodecs. Not called
    /// while the watch decodes in Rust.
    fn on_encoded(&self, server_id: ServerId, sharer: u32, frame: &Frame);
    /// A decoded picture, only while the watch decodes in Rust
    /// ([`crate::Engine::screen_share_set_decode_fallback`]).
    fn on_decoded(&self, server_id: ServerId, sharer: u32, frame: &I420Frame);
}

/// Encoded frames waiting for the decode thread; a full queue drops frames until a keyframe.
const DECODE_QUEUE: usize = 4;

struct Control {
    fallback: AtomicBool,
    keyframe: Notify,
}

impl Control {
    fn request_keyframe(&self) {
        self.keyframe.notify_one();
    }
}

/// A share being watched; dropping it closes the stream.
pub(crate) struct WatchLink {
    control: Arc<Control>,
    sound: Arc<ShareStats>,
    tasks: [JoinHandle<()>; 2],
}

impl WatchLink {
    pub fn start(
        rt: &tokio::runtime::Handle,
        server_id: ServerId,
        sharer: u32,
        sink: Option<Arc<dyn ScreenFrameSink>>,
        sound: ShareSound,
        send: SendStream,
        recv: RecvStream,
    ) -> Self {
        let sound_stats = sound.stats();
        let control = Arc::new(Control {
            fallback: AtomicBool::new(false),
            keyframe: Notify::new(),
        });
        let reader = rt.spawn(read_frames(
            Source { server_id, sharer },
            sink,
            sound,
            control.clone(),
            recv,
        ));
        let requests = rt.spawn(send_requests(control.clone(), send));
        Self {
            control,
            sound: sound_stats,
            tasks: [reader, requests],
        }
    }

    pub fn set_decode_fallback(&self, on: bool) {
        if self.control.fallback.swap(on, Ordering::Relaxed) != on {
            // Either decoder has to start at a keyframe.
            self.control.request_keyframe();
        }
    }

    pub fn request_keyframe(&self) {
        self.control.request_keyframe();
    }

    pub fn sound_stats(&self) -> ShareSoundStats {
        self.sound.snapshot()
    }
}

impl Drop for WatchLink {
    fn drop(&mut self) {
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
    mut recv: RecvStream,
) {
    let mut decoder: Option<DecodeThread> = None;
    loop {
        let frame = match wire::read_frame(&mut recv).await {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(e) => {
                tracing::debug!(error = %e, "screen stream from a sharer ended");
                return;
            }
        };
        if frame.header.kind == FrameKind::Audio {
            sound.packet(&frame.payload);
            continue;
        }
        let Some(sink) = &sink else {
            continue;
        };
        if control.fallback.load(Ordering::Relaxed) {
            decoder
                .get_or_insert_with(|| DecodeThread::start(source, sink.clone(), control.clone()))
                .push(frame, &control);
        } else {
            decoder = None;
            sink.on_encoded(source.server_id, source.sharer, &frame);
        }
    }
}

async fn send_requests(control: Arc<Control>, mut send: SendStream) {
    loop {
        control.keyframe.notified().await;
        if let Err(e) = send.write_all(&[wire::KEYFRAME_REQUEST]).await {
            tracing::debug!(error = %e, "screen stream to a sharer ended");
            return;
        }
    }
}

/// libvpx decoding off the async runtime; it ends when dropped.
struct DecodeThread {
    frames: Option<SyncSender<Frame>>,
    skip_to_keyframe: bool,
}

impl DecodeThread {
    fn start(source: Source, sink: Arc<dyn ScreenFrameSink>, control: Arc<Control>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Frame>(DECODE_QUEUE);
        let spawned = thread::Builder::new()
            .name("screen-decode".into())
            .spawn(move || {
                let mut decoder = match Decoder::new() {
                    Ok(decoder) => decoder,
                    Err(e) => {
                        tracing::error!(error = %e, "screen share decoder did not start");
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
                            sink.on_decoded(source.server_id, source.sharer, &picture);
                        }
                        Ok(None) => wait_keyframe = false,
                        Err(e) => {
                            tracing::debug!(error = %e, "screen share frame not decoded");
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
