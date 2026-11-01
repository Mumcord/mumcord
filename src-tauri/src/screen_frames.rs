//! Watched screen shares to the webview (FR-2.44): every watch has its own Tauri channel that
//! carries raw binary messages (an `ArrayBuffer` each in JavaScript, no JSON or base64).
//!
//! A message is a 16-byte header followed by the payload, little endian:
//!
//! | bytes | field                                                        |
//! |-------|--------------------------------------------------------------|
//! | 0     | kind: [`KIND_VP9`] or [`KIND_RGBA`]                           |
//! | 1     | flags: bit 0 = keyframe (VP9 only)                           |
//! | 2     | temporal layer (VP9 only)                                    |
//! | 3     | reserved, 0                                                  |
//! | 4–11  | timestamp in microseconds (VP9 only)                         |
//! | 12–13 | width                                                        |
//! | 14–15 | height                                                       |
//! | 16–   | VP9 frame for WebCodecs, or `width * height * 4` RGBA bytes  |
//!
//! The UI parses it in `ui/src/api/screenFrames.ts`; keep both in step.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use mumcord_engine::ServerId;
use mumcord_engine::screen_share::wire::Frame;
use mumcord_engine::screen_share::{I420Frame, ScreenFrameSink};
use specta::datatype::DataType;
use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};

pub const HEADER_LEN: usize = 16;
/// An encoded VP9 frame, for WebCodecs.
pub const KIND_VP9: u8 = 1;
/// A picture decoded in Rust (fallback for webviews without WebCodecs VP9).
pub const KIND_RGBA: u8 = 2;
const FLAG_KEYFRAME: u8 = 1;

/// One channel message. Sent raw, so JavaScript gets an `ArrayBuffer` (a `Vec<u8>` would go as a
/// JSON array of numbers).
pub struct FrameMessage(Vec<u8>);

impl IpcResponse for FrameMessage {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Raw(self.0))
    }
}

impl specta::Type for FrameMessage {
    fn definition(_: &mut specta::Types) -> DataType {
        DataType::Reference(specta_typescript::define("ArrayBuffer"))
    }
}

pub type FrameChannel = Channel<FrameMessage>;

/// The channel of every watched share, by server and sharer.
#[derive(Default)]
pub struct ScreenFrames {
    channels: Mutex<HashMap<(ServerId, u32), FrameChannel>>,
}

impl ScreenFrames {
    /// Frames of `sharer` go to `channel` from now on, replacing an earlier one.
    pub fn attach(&self, server_id: ServerId, sharer: u32, channel: FrameChannel) {
        self.lock().insert((server_id, sharer), channel);
    }

    pub fn detach(&self, server_id: ServerId, sharer: u32) {
        self.lock().remove(&(server_id, sharer));
    }

    /// Detaches `sharer` only while `channel_id` is still attached: a failed watch must not take
    /// the channel of a newer watch of the same share.
    pub fn detach_channel(&self, server_id: ServerId, sharer: u32, channel_id: u32) {
        let mut channels = self.lock();
        if channels
            .get(&(server_id, sharer))
            .is_some_and(|c| c.id() == channel_id)
        {
            channels.remove(&(server_id, sharer));
        }
    }

    /// Drops the channels of shares on `server_id` that are no longer in `sharers`.
    pub fn retain(&self, server_id: ServerId, sharers: &[u32]) {
        self.lock()
            .retain(|(server, sharer), _| *server != server_id || sharers.contains(sharer));
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<(ServerId, u32), FrameChannel>> {
        self.channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn send(&self, server_id: ServerId, sharer: u32, message: impl FnOnce() -> Option<Vec<u8>>) {
        // Cloned out so the webview hand-over runs without the lock.
        let Some(channel) = self.lock().get(&(server_id, sharer)).cloned() else {
            return;
        };
        let Some(message) = message() else {
            return;
        };
        if let Err(e) = channel.send(FrameMessage(message)) {
            tracing::debug!(error = %e, "screen share frame not handed to the webview");
            // By channel: a closed pop-out must not take the main window's newer channel.
            self.detach_channel(server_id, sharer, channel.id());
        }
    }
}

impl ScreenFrameSink for ScreenFrames {
    fn on_encoded(&self, server_id: ServerId, sharer: u32, frame: &Frame) {
        self.send(server_id, sharer, || Some(encoded_message(frame)));
    }

    fn on_decoded(&self, server_id: ServerId, sharer: u32, frame: &I420Frame) {
        self.send(server_id, sharer, || picture_message(frame));
    }
}

fn header(kind: u8, flags: u8, layer: u8, timestamp_us: u64, width: u16, height: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN);
    out.extend([kind, flags, layer, 0]);
    out.extend(timestamp_us.to_le_bytes());
    out.extend(width.to_le_bytes());
    out.extend(height.to_le_bytes());
    out
}

fn encoded_message(frame: &Frame) -> Vec<u8> {
    let h = &frame.header;
    let flags = if h.keyframe { FLAG_KEYFRAME } else { 0 };
    let mut out = header(
        KIND_VP9,
        flags,
        h.temporal_layer,
        h.timestamp_us,
        h.width,
        h.height,
    );
    out.extend_from_slice(&frame.payload);
    out
}

/// `None` for a picture too large for the header; the sharer never sends one (720p at most).
fn picture_message(frame: &I420Frame) -> Option<Vec<u8>> {
    let width = u16::try_from(frame.width()).ok()?;
    let height = u16::try_from(frame.height()).ok()?;
    let mut out = header(KIND_RGBA, 0, 0, 0, width, height);
    out.extend(frame.to_rgba());
    Some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Arc;

    use mumcord_engine::screen_share::wire::{FrameHeader, FrameKind};

    use super::*;

    fn vp9(keyframe: bool) -> Frame {
        let payload = vec![0xAA, 0xBB, 0xCC];
        Frame {
            header: FrameHeader {
                kind: FrameKind::Video,
                keyframe,
                temporal_layer: 2,
                timestamp_us: 0x0102_0304_0506_0708,
                width: 1280,
                height: 720,
                payload_len: payload.len() as u32,
            },
            payload,
        }
    }

    fn recording() -> (FrameChannel, Arc<Mutex<Vec<Vec<u8>>>>) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = got.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                sink.lock().unwrap().push(bytes);
            }
            Ok(())
        });
        (channel, got)
    }

    #[test]
    fn encoded_frames_keep_header_fields_and_payload() {
        let message = encoded_message(&vp9(true));
        assert_eq!(message.len(), HEADER_LEN + 3);
        assert_eq!(message[..4], [KIND_VP9, FLAG_KEYFRAME, 2, 0]);
        assert_eq!(message[4..12], 0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(message[12..14], 1280u16.to_le_bytes());
        assert_eq!(message[14..16], 720u16.to_le_bytes());
        assert_eq!(message[16..], [0xAA, 0xBB, 0xCC]);
        assert_eq!(encoded_message(&vp9(false))[1], 0);
    }

    #[test]
    fn pictures_go_as_rgba() {
        let frame = I420Frame::new(4, 2, vec![16; I420Frame::buffer_len(4, 2)]).unwrap();
        let message = picture_message(&frame).unwrap();
        assert_eq!(message[0], KIND_RGBA);
        assert_eq!(message[12..16], [4, 0, 2, 0]);
        assert_eq!(message.len(), HEADER_LEN + 4 * 2 * 4);
    }

    #[test]
    fn frames_reach_only_the_watched_share() {
        let frames = ScreenFrames::default();
        let (channel, got) = recording();
        frames.attach(1, 7, channel);
        frames.on_encoded(1, 7, &vp9(true));
        frames.on_encoded(1, 8, &vp9(true));
        frames.on_encoded(2, 7, &vp9(true));
        assert_eq!(got.lock().unwrap().len(), 1);

        frames.detach(1, 7);
        frames.on_encoded(1, 7, &vp9(false));
        assert_eq!(got.lock().unwrap().len(), 1);
    }

    #[test]
    fn ended_shares_lose_their_channel() {
        let frames = ScreenFrames::default();
        let (first, got_first) = recording();
        let (second, got_second) = recording();
        let (other_server, got_other) = recording();
        frames.attach(1, 7, first);
        frames.attach(1, 8, second);
        frames.attach(2, 7, other_server);
        frames.retain(1, &[8]);
        for (server, sharer) in [(1, 7), (1, 8), (2, 7)] {
            frames.on_encoded(server, sharer, &vp9(true));
        }
        assert!(got_first.lock().unwrap().is_empty());
        assert_eq!(got_second.lock().unwrap().len(), 1);
        assert_eq!(got_other.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_closed_webview_loses_its_channel() {
        let frames = ScreenFrames::default();
        let calls = Arc::new(Mutex::new(0));
        let counter = calls.clone();
        let closed: FrameChannel = Channel::new(move |_| {
            *counter.lock().unwrap() += 1;
            Err(tauri::Error::WebviewNotFound)
        });
        frames.attach(1, 7, closed);
        frames.on_encoded(1, 7, &vp9(true));
        frames.on_encoded(1, 7, &vp9(false));
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn a_re_attached_channel_takes_over_the_frames() {
        let frames = ScreenFrames::default();
        let (docked, got_docked) = recording();
        let (popout, got_popout) = recording();
        frames.attach(1, 7, docked);
        frames.on_encoded(1, 7, &vp9(true));
        frames.attach(1, 7, popout);
        frames.on_encoded(1, 7, &vp9(true));
        assert_eq!(got_docked.lock().unwrap().len(), 1);
        assert_eq!(got_popout.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_older_watch_keeps_the_newer_channel() {
        let frames = ScreenFrames::default();
        let (old, _) = recording();
        let (new, got_new) = recording();
        let old_id = old.id();
        frames.attach(1, 7, old);
        frames.attach(1, 7, new);
        frames.detach_channel(1, 7, old_id);
        frames.on_encoded(1, 7, &vp9(true));
        assert_eq!(got_new.lock().unwrap().len(), 1);
    }
}
