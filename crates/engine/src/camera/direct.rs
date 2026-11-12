//! The own camera on a direct connection: one `STREAM_CAMERA` per admitted viewer that reads the
//! media profile (spec 08, FR-8.92, FR-8.93).
//!
//! The sharer opens the stream after the viewer's `cam-join` and writes one endless Bovnar media
//! document (FR-7.89) with the video channels 1–4 only: no audio, no plugin data. The description
//! on channel 0 carries `.kind = "camera"` and the camera session id — the `id` of the camera's
//! `on` message — so a viewer refuses a stream of an older session. The viewer's direction carries
//! `KEYFRAME_REQUEST = 1` and `WANT = 2` plus one big-endian u16 tile height, both fed into the
//! encoder exactly as a relay viewer's (`want` through the camera hub, FR-8.45).
//!
//! Every encoded frame arrives through the camera hub ([`CameraHub`]): one encoder feeds the relay
//! broadcast and every direct stream. Dropping a [`View`] closes its stream, so leaving, being
//! removed, being blocked or the camera stopping all end it the same way.

use std::collections::BTreeMap;
use std::sync::Arc;

use mumcord_p2p::media::{self, Description, StreamKind, Track, VideoCodec, VideoTrack};
use mumcord_p2p::quinn::{RecvStream, SendStream};
use mumcord_screenshare::TEMPORAL_LAYERS;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use super::send::CameraHub;
use crate::screen_share::ViewerQueue;
use crate::screen_share::wire::{self, FrameHeader, FrameKind};

/// Viewers one camera serves over the direct path (FR-8.93); the relay broadcast keeps 25.
pub(crate) const MAX_DIRECT_VIEWERS: usize = 8;

/// Viewer to sender: my tile is this high; one big-endian u16 follows (FR-8.93).
pub(super) const WANT: u8 = 2;

/// What one reverse-stream message from a viewer asks for (FR-8.93).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    Keyframe,
    Want,
}

/// The request a reverse-stream byte names; `None` for a byte outside the profile.
const fn request(first: u8) -> Option<Request> {
    match first {
        wire::KEYFRAME_REQUEST => Some(Request::Keyframe),
        WANT => Some(Request::Want),
        _ => None,
    }
}

/// Whether a direct path with `open` streams can take one more viewer (FR-8.93): a viewer without
/// a stream fits only below [`MAX_DIRECT_VIEWERS`], one that already has a stream keeps it.
const fn room_for(open: usize, keeps_stream: bool) -> bool {
    keeps_stream || open < MAX_DIRECT_VIEWERS
}

/// The direct streams of one camera session.
pub(crate) struct DirectViewers {
    /// The camera session id: the `id` of the `on` message and of the stream description.
    id: String,
    hub: Arc<CameraHub>,
    views: BTreeMap<u32, View>,
}

impl DirectViewers {
    pub(crate) fn new(id: String, hub: Arc<CameraHub>) -> Self {
        Self {
            id,
            hub,
            views: BTreeMap::new(),
        }
    }

    /// Whether `viewer` can be served over the direct path: it keeps the stream it has, and a
    /// new one fits while the path serves fewer than [`MAX_DIRECT_VIEWERS`] (FR-8.93).
    pub(crate) fn has_room(&self, viewer: u32) -> bool {
        room_for(self.views.len(), self.views.contains_key(&viewer))
    }

    /// Sends the camera to `viewer` from now on; a stream it had already closes.
    pub(crate) fn add(&mut self, rt: &Handle, viewer: u32, send: SendStream, recv: RecvStream) {
        // Closed before it is replaced: the old stream's tasks end with it.
        self.views.remove(&viewer);
        let Some(queue) = self.hub.add_direct(viewer) else {
            return;
        };
        let writer = rt.spawn(write_stream(
            self.hub.clone(),
            viewer,
            queue,
            self.id.clone(),
            send,
        ));
        let requests = rt.spawn(read_requests(self.hub.clone(), viewer, recv));
        self.views.insert(
            viewer,
            View {
                hub: self.hub.clone(),
                session: viewer,
                tasks: [writer, requests],
            },
        );
    }

    /// `viewer` left or was removed; its stream closes.
    pub(crate) fn remove(&mut self, viewer: u32) {
        self.views.remove(&viewer);
    }
}

/// One viewer's stream; dropping it closes the stream and leaves the camera's fan-out.
struct View {
    hub: Arc<CameraHub>,
    session: u32,
    tasks: [JoinHandle<()>; 2],
}

impl Drop for View {
    fn drop(&mut self) {
        self.hub.remove_direct(self.session);
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// What a description depends on; comparing this every frame avoids building (and allocating) a
/// new track list unless the encoder actually changed something (FR-7.95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DescriptionKey {
    width: u16,
    height: u16,
    codec: wire::Codec,
    fps: u32,
}

impl DescriptionKey {
    fn of(video: FrameHeader, fps: u32) -> Option<Self> {
        let FrameKind::Video(codec) = video.kind else {
            return None;
        };
        Some(Self {
            width: video.width,
            height: video.height,
            codec,
            fps,
        })
    }
}

/// The description of what one camera stream carries: one video track per temporal layer, no
/// audio and nothing on channel 6 (FR-7.95, FR-8.92).
fn camera_description(video: FrameHeader, key: DescriptionKey, id: &str) -> Option<Description> {
    let FrameKind::Video(codec) = video.kind else {
        return None;
    };
    let codec = match codec {
        wire::Codec::Vp9 => VideoCodec::Vp9,
        wire::Codec::Av1 => VideoCodec::Av1,
    };
    let rate = if key.fps == 0 {
        30.0
    } else {
        f32::from(u16::try_from(key.fps).unwrap_or(u16::MAX))
    };
    let tracks = (0..TEMPORAL_LAYERS)
        .map(|layer| {
            Track::Video(VideoTrack {
                channel: media::CHANNEL_VIDEO_BASE + u16::from(layer),
                codec,
                layer,
                width: key.width.max(1),
                height: key.height.max(1),
                rate,
            })
        })
        .collect();
    Some(Description {
        kind: StreamKind::Camera,
        timebase: 1,
        id: Some(id.to_owned()),
        tracks,
    })
}

/// Writes the camera's media document to one viewer until the stream breaks or its queue ends.
/// A stream that ended stops counting as a viewer, so the encoder pauses when nobody is left.
async fn write_stream(
    hub: Arc<CameraHub>,
    session: u32,
    queue: Arc<ViewerQueue>,
    id: String,
    mut send: SendStream,
) {
    let mut writer = wire::FrameWriter::new(&mut send, wire::Format::Media);
    let mut video: Option<FrameHeader> = None;
    let mut described: Option<DescriptionKey> = None;
    loop {
        let frame = queue.next().await;
        if matches!(frame.header.kind, FrameKind::Video(_)) {
            video = Some(frame.header);
        }
        if let Some(header) = video
            && let Some(key) = DescriptionKey::of(header, hub.fps())
            && described != Some(key)
            && let Some(wanted) = camera_description(header, key, &id)
        {
            if let Err(e) = writer.write_description(&wanted).await {
                tracing::debug!(error = %e, "camera stream description to a viewer failed");
                break;
            }
            described = Some(key);
            // The viewer applies a new description at a keyframe.
            hub.force_keyframe();
        }
        if let Err(e) = writer.write_frame(&frame).await {
            tracing::debug!(error = %e, "camera stream to a viewer ended");
            break;
        }
    }
    hub.remove_direct(session);
}

/// Reads what the viewer sends back: keyframe requests and the tile height it draws (FR-8.93).
async fn read_requests(hub: Arc<CameraHub>, session: u32, mut recv: RecvStream) {
    let mut first = [0u8; 1];
    loop {
        match recv.read(&mut first).await {
            Ok(Some(_)) => match request(first[0]) {
                Some(Request::Keyframe) => hub.request_keyframe(),
                Some(Request::Want) => {
                    let mut height = [0u8; 2];
                    if recv.read_exact(&mut height).await.is_err() {
                        return;
                    }
                    hub.set_direct_want(session, u16::from_be_bytes(height));
                }
                None => {
                    tracing::debug!(byte = first[0], "ignoring an unknown camera stream request")
                }
            },
            Ok(None) => return,
            Err(e) => {
                tracing::debug!(error = %e, "camera stream from a viewer ended");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef";

    fn video(layer: u8, width: u16, height: u16, codec: wire::Codec) -> FrameHeader {
        FrameHeader {
            kind: FrameKind::Video(codec),
            keyframe: layer == 0,
            temporal_layer: layer,
            timestamp_us: 0,
            width,
            height,
            payload_len: 1,
        }
    }

    #[test]
    fn the_reverse_stream_kinds_are_the_spec_bytes() {
        // `KEYFRAME_REQUEST = 1` stays one byte; `WANT = 2` is followed by a big-endian u16
        // (FR-8.93). Everything else is ignored.
        assert_eq!(request(wire::KEYFRAME_REQUEST), Some(Request::Keyframe));
        assert_eq!(request(WANT), Some(Request::Want));
        assert_eq!((wire::KEYFRAME_REQUEST, WANT), (1, 2));
        for byte in [0u8, 3, 4, 0x80, 0xff] {
            assert_eq!(request(byte), None, "{byte}");
        }
        assert_eq!(u16::from_be_bytes([0x02, 0xd0]), 720);
    }

    #[test]
    fn the_direct_path_serves_at_most_eight_viewers() {
        // FR-8.93: 8 over the direct path, 25 over a relay broadcast.
        assert_eq!(MAX_DIRECT_VIEWERS, 8);
        assert!(MAX_DIRECT_VIEWERS < usize::from(crate::camera::admit::MAX_VIEWERS));
    }

    #[test]
    fn the_ninth_direct_viewer_is_full_and_an_admitted_one_keeps_its_stream() {
        // The 8 seats of FR-8.93 are taken by open streams; a viewer that already has one is not
        // turned away by its own seat, and blocked, hidden or left viewers close the stream and
        // free it again (`DirectViewers::remove`).
        for open in 0..MAX_DIRECT_VIEWERS {
            assert!(room_for(open, false), "{open} open streams leave a seat");
        }
        assert!(
            !room_for(MAX_DIRECT_VIEWERS, false),
            "the ninth viewer is full"
        );
        assert!(
            room_for(MAX_DIRECT_VIEWERS, true),
            "a viewer keeps its own stream"
        );
        assert!(
            room_for(MAX_DIRECT_VIEWERS + 8, true),
            "even after a double admit"
        );
    }

    #[test]
    fn the_description_names_the_camera_and_its_session() {
        let header = video(0, 1280, 720, wire::Codec::Vp9);
        let key = DescriptionKey::of(header, 30).expect("video key");
        let description = camera_description(header, key, ID).expect("description");
        assert_eq!(description.kind, StreamKind::Camera);
        assert_eq!(description.id.as_deref(), Some(ID));
        assert_eq!(description.timebase, 1);
        // Video channels 1–4 only: no audio, nothing on channel 6 (FR-8.92).
        assert_eq!(description.tracks.len(), usize::from(TEMPORAL_LAYERS));
        for (index, track) in description.tracks.iter().enumerate() {
            let Track::Video(track) = track else {
                panic!("a camera stream carries video only");
            };
            assert_eq!(track.channel, media::CHANNEL_VIDEO_BASE + index as u16);
            assert_eq!(usize::from(track.layer), index);
            assert_eq!(track.codec, VideoCodec::Vp9);
            assert_eq!((track.width, track.height), (1280, 720));
            assert_eq!(track.rate, 30.0);
        }
        // The profile's own validator accepts it, and the id must be the session id.
        assert!(description.encode().is_ok());
    }

    #[test]
    fn the_description_follows_the_encoder_and_keeps_the_session_id() {
        let small = video(0, 640, 360, wire::Codec::Vp9);
        let large = video(0, 1280, 720, wire::Codec::Vp9);
        let key = |header| DescriptionKey::of(header, 15).expect("video key");
        assert_ne!(key(small), key(large));
        let description = camera_description(large, key(large), ID).expect("description");
        assert_eq!(description.id.as_deref(), Some(ID));
        let Track::Video(track) = &description.tracks[0] else {
            panic!("a camera stream carries video only");
        };
        assert_eq!(track.rate, 15.0);
        // Audio frames carry no description: the camera has no audio track.
        let audio = FrameHeader {
            kind: FrameKind::Audio,
            ..large
        };
        assert_eq!(DescriptionKey::of(audio, 30), None);
        assert_eq!(camera_description(audio, key(large), ID), None);
    }

    #[test]
    fn a_description_with_an_odd_size_is_still_valid() {
        let header = video(0, 0, 0, wire::Codec::Av1);
        let key = DescriptionKey::of(header, 0).expect("video key");
        let description = camera_description(header, key, ID).expect("description");
        let Track::Video(track) = &description.tracks[0] else {
            panic!("a camera stream carries video only");
        };
        // A zero size never reaches the profile; the default rate is the camera's usual one.
        assert_eq!((track.width, track.height), (1, 1));
        assert_eq!(track.rate, 30.0);
        assert_eq!(track.codec, VideoCodec::Av1);
        assert!(description.encode().is_ok());
    }
}
