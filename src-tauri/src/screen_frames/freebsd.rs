//! FreeBSD's raw pictures need explicit webview credit: Tauri's channel cache itself
//! is unbounded. Allow two messages to overlap IPC latency, retaining only the newest
//! waiting decoded picture. The webview discards a late older conversion by its token.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use super::{Codec, Frame, FrameChannel, FrameMessage, I420Frame, ScreenFrameSink, ServerId};

const ACK_REQUIRED: u8 = 2;
const MAX_IN_FLIGHT: usize = 2;
// Shared by screen and camera registries, so a credit cannot acknowledge the other registry.
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

struct Picture {
    codec: Codec,
    frame: I420Frame,
}

struct Stream {
    channel: FrameChannel,
    in_flight: Vec<u64>,
    latest: Option<Picture>,
}

#[derive(Default)]
pub struct ScreenFrames {
    channels: Mutex<HashMap<(ServerId, u32), Stream>>,
}

impl ScreenFrames {
    pub fn attach(&self, server_id: ServerId, sharer: u32, channel: FrameChannel) {
        self.lock().insert(
            (server_id, sharer),
            Stream {
                channel,
                in_flight: Vec::with_capacity(MAX_IN_FLIGHT),
                latest: None,
            },
        );
    }

    pub fn detach(&self, server_id: ServerId, sharer: u32) {
        self.lock().remove(&(server_id, sharer));
    }

    pub fn detach_channel(&self, server_id: ServerId, sharer: u32, channel_id: u32) {
        let mut channels = self.lock();
        if channels
            .get(&(server_id, sharer))
            .is_some_and(|stream| stream.channel.id() == channel_id)
        {
            channels.remove(&(server_id, sharer));
        }
    }

    pub fn retain(&self, server_id: ServerId, sharers: &[u32]) {
        self.lock()
            .retain(|(server, sharer), _| *server != server_id || sharers.contains(sharer));
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<(ServerId, u32), Stream>> {
        self.channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Late credits after a watch closes or moves to another window are harmless.
    pub(crate) fn acknowledge(&self, token: u64) -> bool {
        let ready = {
            let mut channels = self.lock();
            let Some((&(server, sharer), stream)) = channels
                .iter_mut()
                .find(|(_, stream)| stream.in_flight.contains(&token))
            else {
                return false;
            };
            stream.in_flight.retain(|in_flight| *in_flight != token);
            stream.latest.take().map(|picture| {
                let next = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
                stream.in_flight.push(next);
                (server, sharer, stream.channel.clone(), next, picture)
            })
        };
        if let Some((server, sharer, channel, token, picture)) = ready {
            self.send_picture(
                server,
                sharer,
                channel,
                token,
                picture.codec,
                &picture.frame,
            );
        }
        true
    }

    fn send_picture(
        &self,
        server: ServerId,
        sharer: u32,
        channel: FrameChannel,
        token: u64,
        codec: Codec,
        frame: &I420Frame,
    ) {
        let Some(mut message) = super::picture_message(codec, frame) else {
            tracing::warn!("screen picture exceeds channel dimensions");
            self.detach_channel(server, sharer, channel.id());
            return;
        };
        message[1] |= ACK_REQUIRED;
        message[4..12].copy_from_slice(&token.to_le_bytes());
        if let Err(error) = channel.send(FrameMessage(message)) {
            tracing::debug!(%error, "screen picture not handed to the webview");
            self.detach_channel(server, sharer, channel.id());
        }
    }
}

impl ScreenFrameSink for ScreenFrames {
    fn on_encoded(&self, server: ServerId, sharer: u32, frame: &Frame) {
        let channel = self
            .lock()
            .get(&(server, sharer))
            .map(|stream| stream.channel.clone());
        if let (Some(channel), Some(message)) = (channel, super::encoded_message(frame))
            && let Err(error) = channel.send(FrameMessage(message))
        {
            tracing::debug!(%error, "encoded screen frame not handed to the webview");
            self.detach_channel(server, sharer, channel.id());
        }
    }

    fn on_decoded(&self, server: ServerId, sharer: u32, codec: Codec, frame: &I420Frame) {
        let (channel, token) = {
            let mut channels = self.lock();
            let Some(stream) = channels.get_mut(&(server, sharer)) else {
                return;
            };
            if stream.in_flight.len() >= MAX_IN_FLIGHT {
                stream.latest = Some(Picture {
                    codec,
                    frame: frame.clone(),
                });
                return;
            }
            let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
            stream.in_flight.push(token);
            (stream.channel.clone(), token)
        };
        self.send_picture(server, sharer, channel, token, codec, frame);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tauri::ipc::{Channel, InvokeResponseBody};

    fn recording() -> (FrameChannel, Arc<Mutex<Vec<Vec<u8>>>>) {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let copy = messages.clone();
        (
            Channel::new(move |body| {
                if let InvokeResponseBody::Raw(bytes) = body {
                    copy.lock().unwrap().push(bytes);
                }
                Ok(())
            }),
            messages,
        )
    }

    fn picture(value: u8) -> I420Frame {
        I420Frame::new(2, 2, vec![value; I420Frame::buffer_len(2, 2)]).unwrap()
    }

    fn token(message: &[u8]) -> u64 {
        u64::from_le_bytes(message[4..12].try_into().unwrap())
    }

    #[test]
    fn decoded_delivery_is_bounded_and_credit_sends_only_the_latest_picture() {
        let frames = ScreenFrames::default();
        let (channel, got) = recording();
        frames.attach(1, 7, channel);
        frames.on_decoded(1, 7, Codec::Vp9, &picture(16));
        let first = token(&got.lock().unwrap()[0]);
        for value in 17..100 {
            frames.on_decoded(1, 7, Codec::Av1, &picture(value));
        }
        assert_eq!(got.lock().unwrap().len(), MAX_IN_FLIGHT);
        assert!(frames.acknowledge(first));
        let messages = got.lock().unwrap();
        assert_eq!(messages.len(), MAX_IN_FLIGHT + 1);
        assert_eq!(messages[2][1], ACK_REQUIRED);
        assert_eq!(&messages[2][16..], &picture(99).to_rgba());
        assert_ne!(token(&messages[2]), first);
        drop(messages);
        assert!(!frames.acknowledge(first));
    }

    #[test]
    fn late_credits_cannot_take_a_replacement_watch_or_revive_a_detached_one() {
        let frames = ScreenFrames::default();
        let (old, got_old) = recording();
        frames.attach(1, 7, old);
        frames.on_decoded(1, 7, Codec::Vp9, &picture(16));
        let old_token = token(&got_old.lock().unwrap()[0]);
        frames.on_decoded(1, 7, Codec::Vp9, &picture(50));
        let (new, got_new) = recording();
        frames.attach(1, 7, new);
        assert!(!frames.acknowledge(old_token));
        frames.on_decoded(1, 7, Codec::Vp9, &picture(99));
        assert_eq!(got_new.lock().unwrap().len(), 1);
        let new_token = token(&got_new.lock().unwrap()[0]);
        frames.detach(1, 7);
        assert!(!frames.acknowledge(new_token));
        assert!(frames.lock().is_empty());
    }

    #[test]
    fn a_slow_watch_does_not_hold_another_and_retaining_watches_drops_pending_pictures() {
        let frames = ScreenFrames::default();
        let (a, got_a) = recording();
        let (b, got_b) = recording();
        frames.attach(1, 7, a);
        frames.attach(1, 8, b);
        frames.on_decoded(1, 7, Codec::Vp9, &picture(16));
        frames.on_decoded(1, 7, Codec::Vp9, &picture(50));
        frames.on_decoded(1, 7, Codec::Vp9, &picture(75));
        frames.on_decoded(1, 8, Codec::Vp9, &picture(99));
        assert_eq!(got_b.lock().unwrap().len(), 1);
        let old = token(&got_a.lock().unwrap()[0]);
        frames.retain(1, &[8]);
        assert!(!frames.acknowledge(old));
        assert_eq!(frames.lock().len(), 1);
    }

    #[test]
    fn a_closed_raw_channel_clears_its_credit() {
        let frames = ScreenFrames::default();
        let channel = Channel::new(|_| Err(tauri::Error::WebviewNotFound));
        frames.attach(1, 7, channel);
        frames.on_decoded(1, 7, Codec::Vp9, &picture(16));
        assert!(frames.lock().is_empty());
    }

    #[test]
    fn credits_are_unique_across_screen_and_camera_registries() {
        let screen = ScreenFrames::default();
        let camera = ScreenFrames::default();
        let (a, got_a) = recording();
        let (b, got_b) = recording();
        screen.attach(1, 7, a);
        camera.attach(1, 7, b);
        screen.on_decoded(1, 7, Codec::Vp9, &picture(16));
        camera.on_decoded(1, 7, Codec::Vp9, &picture(16));
        let a = token(&got_a.lock().unwrap()[0]);
        let b = token(&got_b.lock().unwrap()[0]);
        assert_ne!(a, b);
        assert!(!screen.acknowledge(b));
        assert!(!camera.acknowledge(a));
        assert!(screen.acknowledge(a));
        assert!(camera.acknowledge(b));
    }
}
