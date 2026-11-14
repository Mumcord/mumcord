//! The two ends of a relay broadcast (spec 07, FR-7.41 – FR-7.45, FR-7.51; mumpeer
//! `docs/protocol.md` "Screen-share broadcasts").
//!
//! [`Publisher`] (sharer): takes the share once from the media hub like one more viewer, seals
//! every frame with the share key and sends it as one unit; answers key requests of admitted
//! viewers, sends a keyframe when the relay asks, and rekeys when a viewer leaves.
//!
//! [`Subscriber`] (viewer): asks the sharer for the share keys right after binding, decrypts
//! the units and hands the frames to the normal viewer pipeline (`watch`). It also tells the
//! relay the highest temporal layer it wants (the viewer's frame-rate choice) and keeps track of
//! what it receives for the "slow connection" report.
//!
//! The plaintext of a media unit is the frame as the direct stream carries it ([`wire`]: header
//! with timestamp and size, then the payload), so both paths end in the same frames.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_p2p::broadcast::{
    Content, ControlKind, MAX_TEMPORAL_LAYER, VideoCodec, profile, split_slot,
};
use mumcord_p2p::media::{self, Description, Track, VideoCodec as MediaVideoCodec, video_channel};
use mumcord_p2p::{BroadcastEnd, BroadcastId, BroadcastLink, BroadcastMessage, Unit};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::crypto::{FrameOpener, FrameSealer, ShareKey, TrackOpener, TrackSealer};
use super::keyx::{self, CertKey, PendingRequest, Reassembler, SharerChannel, ViewerChannel};
use crate::inspector::{MediaFormat, MediaTap, MediaTapSink, TapDirection};
use crate::screen_share::media::Subscription;
use crate::screen_share::wire::{self, Codec, Frame, FrameHeader, FrameKind};
use crate::session::lock;

/// A viewer without the keys asks again after this long, with a new chunk set.
const KEY_RETRY: Duration = Duration::from_secs(3);
/// Window of the "slow connection" flag (FR-7.52).
const SLOW_WINDOW: Duration = Duration::from_secs(10);
/// How often a relay viewer reports to the sharer (FR-7.52); plugin data is rate-limited by
/// the server, so not more often.
pub(crate) const REPORT_EVERY: Duration = Duration::from_secs(5);
/// Decrypted frames waiting for the viewer pipeline.
const FRAME_QUEUE: usize = 16;

/// Called once when a link ends on its own (not when it is dropped).
pub(crate) type OnEnd = Box<dyn FnOnce(BroadcastEnd) + Send>;

fn content_of(frame: &Frame) -> Content {
    match frame.header.kind {
        FrameKind::Video(codec) => Content::Video {
            codec: match codec {
                Codec::Vp9 => VideoCodec::Vp9,
                Codec::Av1 => VideoCodec::Av1,
            },
            keyframe: frame.header.keyframe,
            temporal_layer: frame.header.temporal_layer,
        },
        FrameKind::Audio => Content::Audio,
    }
}

/// The frame as a direct stream of frame version 2 would carry it: the plaintext of a media
/// unit. Every client that joins a broadcast reads version 2 (FR-7.58).
fn plaintext(frame: &Frame) -> Option<Vec<u8>> {
    let header = frame.header.encode(wire::V2).ok()?;
    let mut out = Vec::with_capacity(header.as_slice().len() + frame.payload.len());
    out.extend_from_slice(header.as_slice());
    out.extend_from_slice(&frame.payload);
    Some(out)
}

/// The frame inside a decrypted media unit; `None` for anything malformed.
fn frame_of(plain: &[u8]) -> Option<Frame> {
    let len = wire::header_len(*plain.first()?)?;
    let (head, payload) = (plain.get(..len)?, plain.get(len..)?);
    let header = FrameHeader::decode(head).ok()?;
    (usize::try_from(header.payload_len).ok()? == payload.len()).then(|| Frame {
        header,
        payload: payload.to_vec(),
    })
}

/// The new-format control kind for a legacy one; both name the same messages.
fn profile_control(kind: ControlKind) -> media::ControlKind {
    match kind {
        ControlKind::KeyRequest => media::ControlKind::KeyRequest,
        ControlKind::Key => media::ControlKind::Key,
        ControlKind::KeyframeRequest => media::ControlKind::Keyframe,
        ControlKind::Layers => media::ControlKind::Layers,
        ControlKind::Want => media::ControlKind::Want,
        ControlKind::Gone => media::ControlKind::Gone,
    }
}

/// The legacy name for a new-format control message; `None` for the description.
pub(crate) fn legacy_control(kind: media::ControlKind) -> Option<ControlKind> {
    Some(match kind {
        media::ControlKind::Description => return None,
        media::ControlKind::KeyRequest => ControlKind::KeyRequest,
        media::ControlKind::Key => ControlKind::Key,
        media::ControlKind::Keyframe => ControlKind::KeyframeRequest,
        media::ControlKind::Layers => ControlKind::Layers,
        media::ControlKind::Want => ControlKind::Want,
        media::ControlKind::Gone => ControlKind::Gone,
    })
}

/// Sends `message` as `key`/`keyreq` chunks; `slot` is set for `key` (publisher side).
pub(crate) async fn send_chunks(
    units: &mpsc::Sender<Vec<u8>>,
    kind: ControlKind,
    slot: Option<u16>,
    message: &[u8],
    set_id: u8,
    media: bool,
) -> bool {
    let chunks = match keyx::split(message, set_id) {
        Ok(chunks) => chunks,
        Err(e) => {
            tracing::warn!(error = %e, "broadcast key message not sent");
            return true;
        }
    };
    for chunk in chunks {
        let mut payload = Vec::with_capacity(2 + chunk.len());
        if let Some(slot) = slot {
            payload.extend_from_slice(&slot.to_be_bytes());
        }
        payload.extend_from_slice(&chunk);
        let sent = if media {
            let message = match profile::Message::control(profile_control(kind), payload) {
                Ok(message) => message,
                Err(e) => {
                    tracing::warn!(error = %e, "broadcast key chunk not sent");
                    return true;
                }
            };
            let mut writer = profile::MessageWriter::new();
            let bytes = match writer.encode(&message) {
                Ok(bytes) => bytes.to_vec(),
                Err(e) => {
                    tracing::warn!(error = %e, "broadcast key chunk not sent");
                    return true;
                }
            };
            units.send(bytes).await
        } else {
            let unit = match Unit::control(kind, &payload) {
                Ok(unit) => unit,
                Err(e) => {
                    tracing::warn!(error = %e, "broadcast key chunk not sent");
                    return true;
                }
            };
            units.send(unit).await
        };
        if sent.is_err() {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Publisher
// ---------------------------------------------------------------------------------------------

/// Where a [`Publisher`] takes its frames from: the screen share's media hub
/// ([`Subscription`]) or the camera's encoder (spec 08).
pub(crate) trait FrameSource: Send + Sync + 'static {
    /// The next frame to seal and send.
    fn next(&self) -> impl Future<Output = Arc<Frame>> + Send;

    /// The relay asked for a keyframe (a new subscriber, or one that lost frames).
    fn request_keyframe(&self);

    /// The next frame must be a keyframe: a new key takes effect only there (FR-7.44).
    fn force_keyframe(&self);

    /// The viewers that hold the keys changed, or one reported a new tile height (camera
    /// `want`, FR-8.45): `viewers` is how many hold the keys, `max_want` the largest height any
    /// of them reported. A screen share encodes one size for all and ignores this.
    fn viewers_changed(&self, viewers: usize, max_want: Option<u16>) {
        let _ = (viewers, max_want);
    }

    /// The stream description for the last video frame, on a media-profile link.
    fn description(&self, _video: &FrameHeader) -> Option<Description> {
        None
    }
}

impl FrameSource for Subscription {
    async fn next(&self) -> Arc<Frame> {
        Subscription::next(self).await
    }

    fn request_keyframe(&self) {
        Subscription::request_keyframe(self);
    }

    fn force_keyframe(&self) {
        Subscription::force_keyframe(self);
    }

    fn description(&self, video: &FrameHeader) -> Option<Description> {
        Some(Subscription::description(self, *video))
    }
}

/// The publisher's sealing layer: the legacy header for old links or the media profile's SFrame
/// track messages for new ones.
enum PublisherSealer {
    Legacy(FrameSealer),
    Media(TrackSealer),
}

impl PublisherSealer {
    fn new(media: bool) -> Result<Self, super::crypto::CryptoError> {
        if media {
            Ok(Self::Media(TrackSealer::new()?))
        } else {
            Ok(Self::Legacy(FrameSealer::new()?))
        }
    }

    fn keys(&self) -> Vec<ShareKey> {
        match self {
            Self::Legacy(sealer) => sealer.keys(),
            Self::Media(sealer) => sealer.keys(),
        }
    }

    fn rekey(&mut self) -> Result<ShareKey, super::crypto::CryptoError> {
        match self {
            Self::Legacy(sealer) => sealer.rekey(),
            Self::Media(sealer) => sealer.rekey(),
        }
    }

    /// One sealed media unit (legacy) or track message (media).
    fn seal_frame(&mut self, frame: &Frame) -> Result<Vec<u8>, super::crypto::CryptoError> {
        let mut out = Vec::new();
        match self {
            Self::Legacy(sealer) => {
                let Some(plain) = plaintext(frame) else {
                    return Ok(out);
                };
                sealer.seal(content_of(frame), &plain, &mut out)?;
            }
            Self::Media(sealer) => {
                let channel = match frame.header.kind {
                    FrameKind::Video(_) => video_channel(frame.header.temporal_layer)
                        .ok_or(super::crypto::CryptoError::NotMedia)?,
                    FrameKind::Audio => media::CHANNEL_AUDIO,
                };
                sealer.seal(
                    channel,
                    frame.header.keyframe,
                    frame.header.timestamp_us,
                    &frame.payload,
                    &mut out,
                )?;
            }
        }
        Ok(out)
    }

    fn seal_description(
        &mut self,
        description: &Description,
    ) -> Result<Vec<u8>, super::crypto::CryptoError> {
        match self {
            Self::Media(sealer) => {
                let document = description
                    .encode()
                    .map_err(|_| super::crypto::CryptoError::TooLarge)?;
                let mut out = Vec::new();
                sealer.seal_control(&document, &mut out)?;
                Ok(out)
            }
            Self::Legacy(_) => Err(super::crypto::CryptoError::NotMedia),
        }
    }

    fn has_pending(&self) -> bool {
        match self {
            Self::Media(sealer) => sealer.has_pending(),
            Self::Legacy(_) => false,
        }
    }

    /// Installs a staged media key now. The legacy sealer switches inside `seal` instead.
    fn commit_pending(&mut self) -> bool {
        match self {
            Self::Media(sealer) => sealer.commit_pending(),
            Self::Legacy(_) => false,
        }
    }
}

/// A sealed description waiting to be sent to a late joiner, shared between the writer and the
/// control task.
type SharedDescription = Arc<Mutex<Option<Description>>>;

/// Keeps one sealed track message of a media-profile link for the packet inspector (FR-7.101):
/// its channel, its clear flags byte and its size, never the ciphertext.
fn tap_track(tap: &Option<MediaTapSink>, channel: u16, sealed: &[u8]) {
    let Some(tap) = tap else {
        return;
    };
    tap.record(
        TapDirection::Sent,
        MediaTap::track(
            MediaFormat::Document,
            channel,
            sealed.first().copied().unwrap_or(0),
            u32::try_from(sealed.len()).unwrap_or(u32::MAX),
        ),
    );
}

/// Keeps one sealed description of a media-profile link, with its decoded form, for the packet
/// inspector (FR-7.101).
fn tap_description(tap: &Option<MediaTapSink>, size: u32, description: &Description) {
    let Some(tap) = tap else {
        return;
    };
    tap.record(
        TapDirection::Sent,
        MediaTap::control(
            MediaFormat::Document,
            media::ControlKind::Description,
            size,
            Some(description),
        ),
    );
}

/// The tile height in a `want` from the relay (slot already split off); `None` when malformed.
fn want_height(payload: &[u8]) -> Option<u16> {
    let bytes: [u8; 2] = payload.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

/// The largest tile height wanted by a slot that holds the keys.
fn max_want<C>(channels: &HashMap<u16, C>, wants: &HashMap<u16, u16>) -> Option<u16> {
    wants
        .iter()
        .filter(|(slot, _)| channels.contains_key(slot))
        .map(|(_, height)| *height)
        .max()
}

enum PubCommand {
    /// A viewer left or was removed: drop its key channel and rekey (FR-7.44).
    Removed(String),
}

/// The sharer's end of the broadcast; dropping it closes the link, which ends the broadcast.
pub(crate) struct Publisher {
    id: BroadcastId,
    /// Certificate hashes of the viewers this sharer admitted; only they get keys.
    admitted: Arc<Mutex<HashSet<String>>>,
    commands: mpsc::UnboundedSender<PubCommand>,
    tasks: [JoinHandle<()>; 2],
}

impl Publisher {
    /// Starts sending the share taken from `frames` over `link` as broadcast `id`. `on_end`
    /// runs when the link ends by itself (the relay or the network); `tap` records every message
    /// of a media-profile link for the packet inspector (FR-7.101).
    pub fn start<S: FrameSource>(
        rt: &tokio::runtime::Handle,
        id: BroadcastId,
        me: CertKey,
        link: BroadcastLink,
        frames: S,
        tap: Option<MediaTapSink>,
        on_end: OnEnd,
    ) -> Result<Self, super::crypto::CryptoError> {
        let media = link.is_media();
        let sealer = Arc::new(Mutex::new(PublisherSealer::new(media)?));
        let description: SharedDescription = Arc::new(Mutex::new(None));
        let frames = Arc::new(frames);
        let admitted = Arc::new(Mutex::new(HashSet::new()));
        let (commands, command_rx) = mpsc::unbounded_channel();
        let writer = rt.spawn(write_units(
            frames.clone(),
            sealer.clone(),
            description.clone(),
            link.sender(),
            media,
            tap.clone(),
        ));
        let control = rt.spawn(
            PublisherControl {
                id,
                me,
                sealer,
                media,
                description,
                frames,
                admitted: admitted.clone(),
                channels: HashMap::new(),
                wants: HashMap::new(),
                reassembler: Reassembler::new(),
                set_id: 0,
                tap,
            }
            .run(link, command_rx, on_end),
        );
        Ok(Self {
            id,
            admitted,
            commands,
            tasks: [writer, control],
        })
    }

    pub fn id(&self) -> BroadcastId {
        self.id
    }

    /// The viewer with certificate `hash` was admitted and may get the keys.
    pub fn admit(&self, hash: &str) {
        lock(&self.admitted).insert(hash.to_ascii_lowercase());
    }

    /// The viewer with certificate `hash` left: it gets no keys any more and the share
    /// switches to a new key at the next keyframe.
    pub fn remove(&self, hash: &str) {
        let hash = hash.to_ascii_lowercase();
        lock(&self.admitted).remove(&hash);
        // The control task is gone only when the link is; then nothing needs a new key.
        let _ = self.commands.send(PubCommand::Removed(hash));
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// What a video frame does with a staged key and the stream description before it is sealed
/// (FR-7.44, FR-7.95). The description is sealed with the new key, so the key is committed
/// first; a viewer applies a pending description at the keyframe, which has to follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MediaPreface {
    /// Install the staged key before sealing anything of this frame.
    commit: bool,
    /// Seal and send the description ahead of the frame.
    send_description: bool,
    /// This frame is not a keyframe, so the next one must be: the viewer applies the new
    /// description there.
    force_keyframe: bool,
}

fn media_preface(keyframe: bool, pending: bool, changed: bool) -> MediaPreface {
    let commit = keyframe && pending;
    MediaPreface {
        commit,
        send_description: changed || commit,
        force_keyframe: changed && !keyframe,
    }
}

async fn write_units<S: FrameSource>(
    frames: Arc<S>,
    sealer: Arc<Mutex<PublisherSealer>>,
    description: SharedDescription,
    units: mpsc::Sender<Vec<u8>>,
    media: bool,
    tap: Option<MediaTapSink>,
) {
    let mut writer = profile::MessageWriter::new();
    loop {
        let frame = frames.next().await;
        if media && let FrameKind::Video(_) = frame.header.kind {
            let pending = lock(&sealer).has_pending();
            let wanted = frames.description(&frame.header);
            let changed = wanted
                .as_ref()
                .is_some_and(|wanted| lock(&description).as_ref() != Some(wanted));
            let preface = media_preface(frame.header.keyframe, pending, changed);
            if preface.commit {
                lock(&sealer).commit_pending();
            }
            if preface.send_description
                && let Some(wanted) = wanted
            {
                let failed = |reason: &str| {
                    tracing::warn!(reason, "broadcast description not sent");
                };
                let sealed = match lock(&sealer).seal_description(&wanted) {
                    Ok(sealed) => Some(sealed),
                    Err(e) => {
                        failed(&e.to_string());
                        None
                    }
                };
                let sent = match sealed {
                    Some(sealed) => {
                        let sealed_len = u32::try_from(sealed.len()).unwrap_or(u32::MAX);
                        match profile::Message::control(media::ControlKind::Description, sealed) {
                            Ok(message) => match writer.encode(&message) {
                                Ok(bytes) => {
                                    if units.send(bytes.to_vec()).await.is_err() {
                                        return;
                                    }
                                    tap_description(&tap, sealed_len, &wanted);
                                    true
                                }
                                Err(e) => {
                                    failed(&e.to_string());
                                    false
                                }
                            },
                            Err(e) => {
                                failed(&e.to_string());
                                false
                            }
                        }
                    }
                    None => false,
                };
                if sent {
                    *lock(&description) = Some(wanted);
                    if preface.force_keyframe {
                        // The viewer applies the description at the next keyframe.
                        frames.force_keyframe();
                    }
                } else {
                    // The description did not go out. When the key already moved, the next
                    // keyframe retries it and gives viewers a frame they can start that key on.
                    if preface.commit {
                        frames.force_keyframe();
                    }
                    continue;
                }
            }
        }
        let sealed = match lock(&sealer).seal_frame(&frame) {
            Ok(sealed) if !sealed.is_empty() => sealed,
            Ok(_) => continue,
            Err(e) => {
                tracing::warn!(error = %e, "broadcast frame not sealed");
                continue;
            }
        };
        let wire = if media {
            let message = match frame.header.kind {
                FrameKind::Video(_) => video_channel(frame.header.temporal_layer)
                    .and_then(|channel| profile::Message::track(channel, sealed).ok()),
                FrameKind::Audio => profile::Message::track(media::CHANNEL_AUDIO, sealed).ok(),
            };
            let Some(message) = message else {
                tracing::warn!("broadcast frame outside the media profile");
                continue;
            };
            if let profile::Message::Track { channel, payload } = &message {
                tap_track(&tap, *channel, payload);
            }
            match writer.encode(&message) {
                Ok(bytes) => bytes.to_vec(),
                Err(e) => {
                    tracing::warn!(error = %e, "broadcast frame not made");
                    continue;
                }
            }
        } else {
            sealed
        };
        if units.send(wire).await.is_err() {
            return;
        }
    }
}

struct PublisherControl<S> {
    id: BroadcastId,
    me: CertKey,
    sealer: Arc<Mutex<PublisherSealer>>,
    media: bool,
    description: SharedDescription,
    frames: Arc<S>,
    admitted: Arc<Mutex<HashSet<String>>>,
    channels: HashMap<u16, ViewerChannel>,
    /// The last tile height each slot asked for (`want`, camera only).
    wants: HashMap<u16, u16>,
    reassembler: Reassembler<u16>,
    set_id: u8,
    /// Where the control messages this publisher receives go for the packet inspector
    /// (FR-7.101).
    tap: Option<MediaTapSink>,
}

impl<S: FrameSource> PublisherControl<S> {
    async fn run(
        mut self,
        mut link: BroadcastLink,
        mut commands: mpsc::UnboundedReceiver<PubCommand>,
        on_end: OnEnd,
    ) {
        let units = link.sender();
        let end = loop {
            tokio::select! {
                message = link.recv() => match message {
                    Ok(BroadcastMessage::Legacy(unit)) => {
                        if !self.on_unit(&unit, &units).await {
                            break BroadcastEnd::Lost;
                        }
                    }
                    Ok(BroadcastMessage::Media(profile::Message::Control { kind, payload })) => {
                        if let Some(tap) = &self.tap {
                            tap.record(
                                TapDirection::Received,
                                MediaTap::control(
                                    MediaFormat::Document,
                                    kind,
                                    u32::try_from(payload.len() + 1).unwrap_or(u32::MAX),
                                    None,
                                ),
                            );
                        }
                        if let Some(kind) = legacy_control(kind)
                            && !self.on_control(kind, &payload, &units).await
                        {
                            break BroadcastEnd::Lost;
                        }
                    }
                    Ok(BroadcastMessage::Media(_)) => {}
                    Err(end) => break end,
                },
                Some(PubCommand::Removed(hash)) = commands.recv() => {
                    let wants = &mut self.wants;
                    self.channels.retain(|slot, ch| {
                        let keep = ch.cert_hash() != hash;
                        if !keep {
                            wants.remove(slot);
                        }
                        keep
                    });
                    self.viewers_changed();
                    self.rekey(&units).await;
                }
            }
        };
        tracing::info!(?end, "screen share broadcast ended");
        on_end(end);
    }

    /// Handles one old-format unit from the relay; false once the link is gone.
    async fn on_unit(&mut self, unit: &Unit, units: &mpsc::Sender<Vec<u8>>) -> bool {
        let Content::Control(kind) = unit.header.content else {
            // Only subscribers get media.
            return true;
        };
        self.on_control(kind, unit.payload(), units).await
    }

    async fn on_control(
        &mut self,
        kind: ControlKind,
        payload: &[u8],
        units: &mpsc::Sender<Vec<u8>>,
    ) -> bool {
        match kind {
            ControlKind::KeyframeRequest => self.frames.request_keyframe(),
            ControlKind::Gone => {
                if let Some((slot, _)) = split_slot(payload) {
                    self.reassembler.forget(&slot);
                    self.wants.remove(&slot);
                    if self.channels.remove(&slot).is_some() {
                        self.viewers_changed();
                        return self.rekey(units).await;
                    }
                }
            }
            ControlKind::KeyRequest => {
                let Some((slot, chunk)) = split_slot(payload) else {
                    return true;
                };
                match self.reassembler.push(slot, chunk, Instant::now()) {
                    Ok(Some(request)) => return self.answer(slot, &request, units).await,
                    Ok(None) => {}
                    Err(e) => tracing::debug!(error = %e, "broadcast key request chunk dropped"),
                }
            }
            ControlKind::Want => {
                if let Some((slot, height)) =
                    split_slot(payload).and_then(|(slot, rest)| Some((slot, want_height(rest)?)))
                {
                    self.wants.insert(slot, height);
                    self.viewers_changed();
                }
            }
            // Not the publisher's to receive: `key` and `layers`.
            ControlKind::Key | ControlKind::Layers => {}
        }
        true
    }

    /// Tells the source how many viewers hold the keys and the largest tile height they want.
    /// A `want` may come before the keys; it counts once its slot holds them.
    fn viewers_changed(&self) {
        self.frames
            .viewers_changed(self.channels.len(), max_want(&self.channels, &self.wants));
    }

    async fn answer(&mut self, slot: u16, request: &[u8], units: &mpsc::Sender<Vec<u8>>) -> bool {
        let keys = lock(&self.sealer).keys();
        let admitted = self.admitted.clone();
        let answered = keyx::answer_request(
            self.id,
            &self.me,
            request,
            |hash| lock(&admitted).contains(hash),
            &keys,
        );
        match answered {
            Ok((channel, answer)) => {
                self.channels.insert(slot, channel);
                self.viewers_changed();
                self.set_id = self.set_id.wrapping_add(1);
                send_chunks(
                    units,
                    ControlKind::Key,
                    Some(slot),
                    &answer,
                    self.set_id,
                    self.media,
                )
                .await
                    && self.send_description(units).await
            }
            Err(e) => {
                tracing::info!(error = %e, "broadcast key request refused");
                true
            }
        }
    }

    /// Sends the current description to the relay after a viewer got the keys, so a late joiner
    /// has it before the next frame.
    async fn send_description(&self, units: &mpsc::Sender<Vec<u8>>) -> bool {
        if !self.media {
            return true;
        }
        let Some(description) = lock(&self.description).clone() else {
            return true;
        };
        let sealed = match lock(&self.sealer).seal_description(&description) {
            Ok(sealed) => sealed,
            Err(e) => {
                tracing::warn!(error = %e, "broadcast description not sealed");
                return true;
            }
        };
        let message = match profile::Message::control(media::ControlKind::Description, sealed) {
            Ok(message) => message,
            Err(e) => {
                tracing::warn!(error = %e, "broadcast description not made");
                return true;
            }
        };
        let mut writer = profile::MessageWriter::new();
        match writer.encode(&message) {
            Ok(bytes) => units.send(bytes.to_vec()).await.is_ok(),
            Err(e) => {
                tracing::warn!(error = %e, "broadcast description not made");
                true
            }
        }
    }

    /// Stages a new key, hands it to every remaining viewer and makes the next frame a
    /// keyframe. The writer commits the key there and reseals the description with it before
    /// that frame (FR-7.95).
    async fn rekey(&mut self, units: &mpsc::Sender<Vec<u8>>) -> bool {
        let keys = {
            let mut sealer = lock(&self.sealer);
            if let Err(e) = sealer.rekey() {
                tracing::warn!(error = %e, "broadcast not rekeyed");
                return true;
            }
            sealer.keys()
        };
        let new_id = keys.last().map(|k| k.id);
        tracing::info!(key_id = ?new_id, viewers = self.channels.len(), "broadcast rekeyed");
        let mut updates = Vec::new();
        for (slot, channel) in &mut self.channels {
            match channel.update(&keys) {
                Ok(update) => updates.push((*slot, update)),
                Err(e) => tracing::warn!(error = %e, "broadcast key update not sealed"),
            }
        }
        for (slot, update) in updates {
            self.set_id = self.set_id.wrapping_add(1);
            if !send_chunks(
                units,
                ControlKind::Key,
                Some(slot),
                &update,
                self.set_id,
                self.media,
            )
            .await
            {
                return false;
            }
        }
        self.frames.force_keyframe();
        true
    }
}

// ---------------------------------------------------------------------------------------------
// Subscriber
// ---------------------------------------------------------------------------------------------

/// The viewer's frame-rate choice (FR-7.51), sent to the relay as the highest temporal layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MaxFps {
    #[default]
    Auto,
    Up30,
    Up15,
}

impl MaxFps {
    /// The viewer's cap as the relay takes it: "Up to 30" or "Up to 15"; anything above 30
    /// leaves every layer on.
    pub(crate) fn from_fps(fps: Option<u32>) -> Self {
        match fps {
            Some(fps) if fps <= 15 => Self::Up15,
            Some(fps) if fps <= 30 => Self::Up30,
            _ => Self::Auto,
        }
    }

    fn fps(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::Up30 => Some(30),
            Self::Up15 => Some(15),
        }
    }
}

/// What a viewer receives through the broadcast, for its report to the sharer (FR-7.52).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Received {
    /// Video frames in the last second.
    pub fps: u32,
    /// Layers were missing that this viewer did not turn off itself, within the last 10 s.
    pub slow: bool,
}

#[derive(Default)]
struct Meter {
    /// Highest layer the share was seen with.
    top: u8,
    /// Average time between two layer-0 frames, in microseconds.
    base_interval_us: Option<u64>,
    last_base_ts: Option<u64>,
    second_start: Option<Instant>,
    frames: u32,
    fps: u32,
    /// Highest layer seen in the current second.
    second_top: u8,
    /// When a second ended with fewer layers than this viewer asked for.
    short_at: Option<Instant>,
}

impl Meter {
    fn on_frame(&mut self, header: &FrameHeader, wanted: u8, now: Instant) {
        self.top = self.top.max(header.temporal_layer);
        if header.temporal_layer == 0 {
            if let Some(last) = self.last_base_ts
                && header.timestamp_us > last
            {
                let interval = header.timestamp_us - last;
                // Ignore gaps (pauses, lost frames): only intervals up to a second count.
                if interval <= 1_000_000 {
                    self.base_interval_us = Some(match self.base_interval_us {
                        Some(avg) => (avg * 7 + interval) / 8,
                        None => interval,
                    });
                }
            }
            self.last_base_ts = Some(header.timestamp_us);
        }
        let start = *self.second_start.get_or_insert(now);
        if now.saturating_duration_since(start) >= Duration::from_secs(1) {
            self.fps = self.frames;
            if self.second_top < self.top.min(wanted) {
                self.short_at = Some(now);
            }
            self.frames = 0;
            self.second_top = 0;
            self.second_start = Some(now);
        }
        self.frames += 1;
        self.second_top = self.second_top.max(header.temporal_layer);
    }

    /// The highest layer that stays at or below `max_fps`; every layer below the top halves
    /// the frame rate.
    fn layer_for(&self, max_fps: Option<u32>) -> u8 {
        let (Some(max), Some(interval)) = (max_fps, self.base_interval_us) else {
            return MAX_TEMPORAL_LAYER;
        };
        let base_fps = (1_000_000 / interval.max(1)) as u32;
        let mut layer = 0;
        while layer < self.top && base_fps << (layer + 1) <= max + max / 10 {
            layer += 1;
        }
        layer
    }

    fn received(&self, now: Instant) -> Received {
        Received {
            fps: self.fps,
            slow: self
                .short_at
                .is_some_and(|at| now.saturating_duration_since(at) < SLOW_WINDOW),
        }
    }
}

/// The viewer's end of a broadcast; dropping it closes the subscriber link (with the
/// `keep_old_key` test hook it stays open and only tries the old keys, AC-7.30).
pub(crate) struct Subscriber {
    id: BroadcastId,
    /// When the last report went to the sharer.
    reported: Mutex<Option<Instant>>,
    max_fps: watch::Sender<MaxFps>,
    meter: Arc<Mutex<Meter>>,
    detach: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Subscriber {
    /// Joins broadcast `id` of the sharer with certificate `sharer_hash` over `link`.
    /// Decrypted frames go to the returned receiver; keyframe requests to the returned sender.
    pub fn start(
        rt: &tokio::runtime::Handle,
        id: BroadcastId,
        me: CertKey,
        sharer_hash: String,
        link: BroadcastLink,
        on_end: OnEnd,
    ) -> (Self, mpsc::Receiver<Frame>, mpsc::Sender<Vec<u8>>) {
        let (frames_tx, frames) = mpsc::channel(FRAME_QUEUE);
        let (max_fps, max_fps_rx) = watch::channel(MaxFps::Auto);
        let (detach, detach_rx) = watch::channel(false);
        let meter = Arc::new(Mutex::new(Meter::default()));
        let units = link.sender();
        let media = link.is_media();
        let task = rt.spawn(
            SubscriberState {
                id,
                me,
                sharer_hash,
                media,
                pending: None,
                channel: None,
                opener: FrameOpener::new(),
                media_opener: TrackOpener::new(),
                description: None,
                pending_description: None,
                reassembler: Reassembler::new(),
                set_id: 0,
                meter: meter.clone(),
                sent_layer: None,
            }
            .run(link, frames_tx, max_fps_rx, detach_rx, on_end),
        );
        (
            Self {
                id,
                reported: Mutex::new(None),
                max_fps,
                meter,
                detach,
                task,
            },
            frames,
            units,
        )
    }

    pub fn id(&self) -> BroadcastId {
        self.id
    }

    /// What to report to the sharer, once every [`REPORT_EVERY`].
    pub fn take_report(&self, now: Instant) -> Option<Received> {
        let mut reported = lock(&self.reported);
        if reported.is_some_and(|at| now.saturating_duration_since(at) < REPORT_EVERY) {
            return None;
        }
        *reported = Some(now);
        Some(lock(&self.meter).received(now))
    }

    pub fn set_max_fps(&self, max: MaxFps) {
        self.max_fps.send_replace(max);
    }
}

impl Drop for Subscriber {
    fn drop(&mut self) {
        if crate::test_hooks::keep_old_key() {
            // The relay closes the link once the sharer removed this viewer.
            self.detach.send_replace(true);
        } else {
            self.task.abort();
        }
    }
}

struct SubscriberState {
    id: BroadcastId,
    me: CertKey,
    sharer_hash: String,
    media: bool,
    pending: Option<PendingRequest>,
    channel: Option<SharerChannel>,
    opener: FrameOpener,
    media_opener: TrackOpener,
    description: Option<Description>,
    pending_description: Option<Description>,
    reassembler: Reassembler<()>,
    set_id: u8,
    meter: Arc<Mutex<Meter>>,
    sent_layer: Option<u8>,
}

impl SubscriberState {
    async fn run(
        mut self,
        mut link: BroadcastLink,
        frames: mpsc::Sender<Frame>,
        mut max_fps: watch::Receiver<MaxFps>,
        mut detach: watch::Receiver<bool>,
        on_end: OnEnd,
    ) {
        let units = link.sender();
        // Nothing arrives until the next keyframe (protocol): ask for the keys and one.
        if !self.request_keys(&units).await
            || !send_control(&units, ControlKind::KeyframeRequest, &[], self.media).await
        {
            on_end(BroadcastEnd::Lost);
            return;
        }
        let mut retry =
            tokio::time::interval_at(tokio::time::Instant::now() + KEY_RETRY, KEY_RETRY);
        let mut detached = false;
        let end = loop {
            tokio::select! {
                message = link.recv() => match message {
                    Ok(BroadcastMessage::Legacy(mut unit)) => {
                        let max = *max_fps.borrow();
                        if detached {
                            self.try_old_key(&mut unit);
                        } else if !self.on_unit(&mut unit, &units, &frames, max).await {
                            break BroadcastEnd::Lost;
                        }
                    }
                    Ok(BroadcastMessage::Media(message)) => {
                        let max = *max_fps.borrow();
                        if !detached && !self.on_media(message, &units, &frames, max).await {
                            break BroadcastEnd::Lost;
                        }
                    }
                    Err(end) => break end,
                },
                _ = retry.tick(), if self.channel.is_none() && !detached => {
                    if !self.request_keys(&units).await {
                        break BroadcastEnd::Lost;
                    }
                }
                Ok(()) = max_fps.changed(), if !detached => {
                    let max = *max_fps.borrow_and_update();
                    if !self.send_layers(&units, max).await {
                        break BroadcastEnd::Lost;
                    }
                }
                Ok(()) = detach.changed(), if !detached => {
                    tracing::info!(keys = ?self.opener.key_ids(), "broadcast left; keeping the old keys (test hook)");
                    detached = true;
                }
            }
        };
        tracing::info!(?end, "screen share broadcast link ended");
        if !detached {
            on_end(end);
        }
    }

    async fn request_keys(&mut self, units: &mpsc::Sender<Vec<u8>>) -> bool {
        let (pending, request) = match keyx::request(self.id, &self.me) {
            Ok(request) => request,
            Err(e) => {
                tracing::warn!(error = %e, "broadcast key request not made");
                return true;
            }
        };
        self.pending = Some(pending);
        self.set_id = self.set_id.wrapping_add(1);
        send_chunks(
            units,
            ControlKind::KeyRequest,
            None,
            &request,
            self.set_id,
            self.media,
        )
        .await
    }

    async fn on_unit(
        &mut self,
        unit: &mut Unit,
        units: &mpsc::Sender<Vec<u8>>,
        frames: &mpsc::Sender<Frame>,
        max_fps: MaxFps,
    ) -> bool {
        match unit.header.content {
            Content::Control(ControlKind::Key) => {
                let first = self.channel.is_none();
                match self.reassembler.push((), unit.payload(), Instant::now()) {
                    Ok(Some(message)) => self.on_key_message(&message),
                    Ok(None) => {}
                    Err(e) => tracing::debug!(error = %e, "broadcast key chunk dropped"),
                }
                // The keyframe asked for at the start may have come before the keys.
                !(first && self.channel.is_some())
                    || send_control(units, ControlKind::KeyframeRequest, &[], self.media).await
            }
            Content::Control(_) | Content::Data => true,
            Content::Video { .. } | Content::Audio => {
                if self.channel.is_none() {
                    return true;
                }
                let frame = match self.opener.open(&mut unit.bytes) {
                    Ok((_, plain)) => frame_of(plain),
                    Err(e) => {
                        tracing::debug!(error = %e, "broadcast frame not decrypted");
                        None
                    }
                };
                let Some(frame) = frame else {
                    return true;
                };
                self.deliver_frame(frame, units, frames, max_fps).await
            }
        }
    }

    async fn deliver_frame(
        &mut self,
        frame: Frame,
        units: &mpsc::Sender<Vec<u8>>,
        frames: &mpsc::Sender<Frame>,
        max_fps: MaxFps,
    ) -> bool {
        if matches!(frame.header.kind, FrameKind::Video(_)) {
            let wanted = self.sent_layer.unwrap_or(MAX_TEMPORAL_LAYER);
            let layer = {
                let mut meter = lock(&self.meter);
                meter.on_frame(&frame.header, wanted, Instant::now());
                meter.layer_for(max_fps.fps())
            };
            // The share's frame rate becomes known with the frames.
            if max_fps != MaxFps::Auto
                && self.sent_layer != Some(layer)
                && !self.send_layers(units, max_fps).await
            {
                return false;
            }
        }
        frames.send(frame).await.is_ok()
    }

    async fn on_media(
        &mut self,
        message: profile::Message,
        units: &mpsc::Sender<Vec<u8>>,
        frames: &mpsc::Sender<Frame>,
        max_fps: MaxFps,
    ) -> bool {
        match message {
            profile::Message::Control { kind, mut payload } => match kind {
                media::ControlKind::Description => {
                    if let Ok(plain) = self.media_opener.open_control(&mut payload)
                        && let Ok(description) = Description::parse(plain)
                    {
                        if self.description.is_some() {
                            self.pending_description = Some(description);
                        } else {
                            self.description = Some(description);
                        }
                    }
                    true
                }
                media::ControlKind::Key => {
                    let first = self.channel.is_none();
                    match self.reassembler.push((), &payload, Instant::now()) {
                        Ok(Some(message)) => self.on_key_message(&message),
                        Ok(None) => {}
                        Err(e) => tracing::debug!(error = %e, "broadcast key chunk dropped"),
                    }
                    !(first && self.channel.is_some())
                        || send_control(units, ControlKind::KeyframeRequest, &[], self.media).await
                }
                _ => true,
            },
            profile::Message::Track {
                channel,
                mut payload,
            } => {
                if self.channel.is_none() {
                    return true;
                }
                let opened = match self.media_opener.open(channel, &mut payload) {
                    Ok(opened) => opened,
                    Err(e) => {
                        tracing::debug!(error = %e, "broadcast frame not decrypted");
                        return true;
                    }
                };
                let Some(frame) = self.media_frame(channel, opened) else {
                    return true;
                };
                self.deliver_frame(frame, units, frames, max_fps).await
            }
        }
    }

    fn media_frame(
        &mut self,
        channel: u16,
        opened: super::crypto::OpenedTrack<'_>,
    ) -> Option<Frame> {
        if opened.keyframe
            && let Some(description) = self.pending_description.take()
        {
            self.description = Some(description);
        }
        let description = self.description.as_ref()?;
        let header = match media::channel_use(channel) {
            Some(media::ChannelUse::VideoLayer(layer)) => {
                if opened.keyframe && layer != 0 {
                    return None;
                }
                let Some(Track::Video(track)) = description
                    .tracks
                    .iter()
                    .find(|track| track.channel() == channel)
                else {
                    return None;
                };
                FrameHeader {
                    kind: FrameKind::Video(match track.codec {
                        MediaVideoCodec::Vp9 => Codec::Vp9,
                        MediaVideoCodec::Av1 => Codec::Av1,
                    }),
                    keyframe: opened.keyframe,
                    temporal_layer: layer,
                    timestamp_us: opened.timestamp_micros,
                    width: track.width,
                    height: track.height,
                    payload_len: u32::try_from(opened.frame.len()).ok()?,
                }
            }
            Some(media::ChannelUse::Audio) => FrameHeader {
                kind: FrameKind::Audio,
                keyframe: false,
                temporal_layer: 0,
                timestamp_us: opened.timestamp_micros,
                width: 0,
                height: 0,
                payload_len: u32::try_from(opened.frame.len()).ok()?,
            },
            _ => return None,
        };
        header.validate().ok()?;
        Some(Frame {
            header,
            payload: opened.frame.to_vec(),
        })
    }

    fn on_key_message(&mut self, message: &[u8]) {
        let keys = if let Some(pending) = self.pending.take() {
            match pending.accept(message, &self.sharer_hash) {
                Ok((channel, keys)) => {
                    self.channel = Some(channel);
                    keys
                }
                Err(e) => {
                    // Asked again on the next retry tick.
                    tracing::warn!(error = %e, "broadcast key answer refused");
                    return;
                }
            }
        } else if let Some(channel) = self.channel.as_mut() {
            match channel.open_update(message) {
                Ok(keys) => keys,
                Err(e) => {
                    tracing::warn!(error = %e, "broadcast key update refused");
                    return;
                }
            }
        } else {
            return;
        };
        for key in &keys {
            self.opener.add_key(key);
            self.media_opener.add_key(key);
        }
        tracing::debug!(
            keys = ?self.opener.key_ids(),
            media_keys = ?self.media_opener.key_ids(),
            "broadcast keys received"
        );
    }

    async fn send_layers(&mut self, units: &mpsc::Sender<Vec<u8>>, max: MaxFps) -> bool {
        let layer = lock(&self.meter).layer_for(max.fps());
        if self.sent_layer == Some(layer) || (self.sent_layer.is_none() && max == MaxFps::Auto) {
            return true;
        }
        self.sent_layer = Some(layer);
        send_control(units, ControlKind::Layers, &[layer], self.media).await
    }

    /// `keep_old_key` hook (AC-7.30): after leaving, tries the keys held from before on what
    /// still arrives and logs the outcome, without passing anything on.
    fn try_old_key(&mut self, unit: &mut Unit) {
        if matches!(unit.header.content, Content::Control(_) | Content::Data) {
            return;
        }
        let key_id = unit.header.key_id;
        match self.opener.open(&mut unit.bytes) {
            Ok(_) => tracing::info!(key_id, "old broadcast key decrypted a frame (test hook)"),
            Err(_) => {
                tracing::info!(
                    key_id,
                    "old broadcast key cannot decrypt a frame (test hook)"
                );
            }
        }
    }
}

async fn send_control(
    units: &mpsc::Sender<Vec<u8>>,
    kind: ControlKind,
    payload: &[u8],
    media: bool,
) -> bool {
    if media {
        let message = match profile::Message::control(profile_control(kind), payload.to_vec()) {
            Ok(message) => message,
            Err(e) => {
                tracing::warn!(error = %e, "broadcast control message not made");
                return true;
            }
        };
        let mut writer = profile::MessageWriter::new();
        return match writer.encode(&message) {
            Ok(bytes) => units.send(bytes.to_vec()).await.is_ok(),
            Err(e) => {
                tracing::warn!(error = %e, "broadcast control message not made");
                true
            }
        };
    }
    match Unit::control(kind, payload) {
        Ok(unit) => units.send(unit).await.is_ok(),
        Err(e) => {
            tracing::warn!(error = %e, "broadcast control unit not sent");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_carry_frame_version_2_with_the_codec() {
        let frame = Frame {
            header: FrameHeader {
                kind: FrameKind::Video(Codec::Av1),
                keyframe: true,
                payload_len: 3,
                ..video(1, 5)
            },
            payload: vec![1, 2, 3],
        };
        let plain = plaintext(&frame).expect("plaintext");
        assert_eq!(plain[0], wire::V2);
        assert_eq!(frame_of(&plain).map(|f| f.header), Some(frame.header));
        assert_eq!(
            content_of(&frame),
            Content::Video {
                codec: VideoCodec::Av1,
                keyframe: true,
                temporal_layer: 1,
            }
        );
        assert_eq!(frame_of(&plain[..plain.len() - 1]), None, "short payload");
    }

    #[test]
    fn a_want_counts_only_for_slots_holding_the_keys() {
        assert_eq!(want_height(&[0x02, 0xd0]), Some(720));
        assert_eq!(want_height(&[0x02]), None);
        assert_eq!(want_height(&[0, 1, 2]), None);
        let channels: HashMap<u16, ()> = [(1, ()), (2, ())].into();
        let mut wants = HashMap::from([(1, 360), (3, 720)]);
        assert_eq!(max_want(&channels, &wants), Some(360));
        wants.insert(2, 540);
        assert_eq!(max_want(&channels, &wants), Some(540));
        assert_eq!(max_want(&HashMap::<u16, ()>::new(), &wants), None);
    }

    #[test]
    fn a_rekey_keyframe_reseals_the_description_before_the_frame() {
        // Unchanged description, no staged key: nothing to send.
        assert_eq!(
            media_preface(false, false, false),
            MediaPreface {
                commit: false,
                send_description: false,
                force_keyframe: false,
            }
        );
        // A staged key waits for a video keyframe; a delta stays on the old key.
        assert_eq!(
            media_preface(false, true, false),
            MediaPreface {
                commit: false,
                send_description: false,
                force_keyframe: false,
            }
        );
        // The keyframe commits first, then the description goes out with the new key.
        assert_eq!(
            media_preface(true, true, false),
            MediaPreface {
                commit: true,
                send_description: true,
                force_keyframe: false,
            }
        );
        // A resolution change on a delta sends the description with the current key and asks
        // for a keyframe. On a keyframe it is already the frame the viewer applies it at.
        assert_eq!(
            media_preface(false, false, true),
            MediaPreface {
                commit: false,
                send_description: true,
                force_keyframe: true,
            }
        );
        assert_eq!(
            media_preface(true, false, true),
            MediaPreface {
                commit: false,
                send_description: true,
                force_keyframe: false,
            }
        );
        // Both at once: the delta does not commit. The following keyframe does, and resends.
        assert_eq!(
            media_preface(false, true, true),
            MediaPreface {
                commit: false,
                send_description: true,
                force_keyframe: true,
            }
        );
        assert_eq!(
            media_preface(true, true, true),
            MediaPreface {
                commit: true,
                send_description: true,
                force_keyframe: false,
            }
        );
    }

    #[test]
    fn the_viewer_fps_choice_maps_to_the_relay_cap() {
        assert_eq!(MaxFps::from_fps(None), MaxFps::Auto);
        assert_eq!(MaxFps::from_fps(Some(30)), MaxFps::Up30);
        assert_eq!(MaxFps::from_fps(Some(15)), MaxFps::Up15);
        assert_eq!(MaxFps::from_fps(Some(60)), MaxFps::Auto);
    }

    fn video(layer: u8, ts_ms: u64) -> FrameHeader {
        FrameHeader {
            kind: FrameKind::Video(Codec::Vp9),
            keyframe: false,
            temporal_layer: layer,
            timestamp_us: ts_ms * 1000,
            width: 1920,
            height: 1080,
            payload_len: 0,
        }
    }

    #[test]
    fn a_frame_survives_the_trip_through_a_unit_plaintext() {
        let frame = Frame {
            header: FrameHeader {
                payload_len: 3,
                ..video(1, 5)
            },
            payload: vec![1, 2, 3],
        };
        let plain = plaintext(&frame).expect("test");
        assert_eq!(frame_of(&plain), Some(frame.clone()));
        assert_eq!(frame_of(&plain[..plain.len() - 1]), None);
        assert_eq!(
            content_of(&frame),
            Content::Video {
                codec: VideoCodec::Vp9,
                keyframe: false,
                temporal_layer: 1
            }
        );
    }

    #[test]
    fn sealed_broadcast_messages_are_tapped_for_the_inspector() {
        let inspector = Arc::new(crate::inspector::Inspector::default());
        inspector.set_enabled(true);
        let tap = Some(MediaTapSink::new(&inspector, 1));
        // A track message as it goes on the wire: flags byte, SFrame header, ciphertext.
        tap_track(&tap, 3, &[media::FLAG_KEYFRAME, 0x10, 0xab, 0xcd]);
        let description = Description {
            kind: media::StreamKind::Screen,
            timebase: 1,
            id: None,
            tracks: Vec::new(),
        };
        tap_description(&tap, 42, &description);

        let poll = inspector.poll(None, Vec::new());
        assert_eq!(poll.packets.len(), 2);
        let track = poll.packets[0].media.as_ref().expect("a media row");
        assert_eq!(track.channel, Some(3));
        assert!(track.keyframe);
        assert_eq!(poll.packets[0].size, 4);
        assert_eq!(poll.packets[0].raw_hex, "", "never the ciphertext");
        let control = poll.packets[1].media.as_ref().expect("a media row");
        assert_eq!(control.channel, Some(media::CHANNEL_CONTROL));
        assert_eq!(control.control, Some(media::KIND_DESCRIPTION));
        assert_eq!(poll.packets[1].size, 42);
        assert!(
            poll.packets[1]
                .decoded
                .as_deref()
                .is_some_and(|d: &str| d.contains("screen")),
            "the description is decoded"
        );
    }

    /// 60 fps with four layers: layer 0 every 8 frames (7.5 fps).
    fn meter_at_60(now: Instant) -> Meter {
        let mut meter = Meter::default();
        let pattern = [0, 3, 2, 3, 1, 3, 2, 3];
        for i in 0..64u64 {
            let at = now + Duration::from_micros(i * 16_667);
            meter.on_frame(&video(pattern[(i % 8) as usize], i * 16_667 / 1000), 3, at);
        }
        meter
    }

    #[test]
    fn the_frame_rate_choice_picks_the_temporal_layer() {
        let meter = meter_at_60(Instant::now());
        assert_eq!(meter.top, 3);
        assert_eq!(meter.layer_for(None), MAX_TEMPORAL_LAYER);
        assert_eq!(meter.layer_for(Some(30)), 2);
        assert_eq!(meter.layer_for(Some(15)), 1);
    }

    #[test]
    fn missing_layers_make_a_viewer_slow_for_ten_seconds() {
        let start = Instant::now();
        let mut meter = meter_at_60(start);
        assert!(!meter.received(start + Duration::from_secs(1)).slow);
        // Two seconds with layer 0 only, though everything is wanted.
        let mut at = start + Duration::from_secs(2);
        for i in 0..20u64 {
            at = start + Duration::from_secs(2) + Duration::from_millis(i * 133);
            meter.on_frame(&video(0, 2000 + i * 133), 3, at);
        }
        assert!(meter.received(at).slow);
        assert!(meter.received(at + Duration::from_secs(1)).fps < 60);
        assert!(!meter.received(at + SLOW_WINDOW).slow);
        // Asking for layer 0 only is no slow connection.
        let mut capped = meter_at_60(start);
        for i in 0..20u64 {
            let at = start + Duration::from_secs(2) + Duration::from_millis(i * 133);
            capped.on_frame(&video(0, 2000 + i * 133), 0, at);
        }
        assert!(!capped.received(start + Duration::from_secs(5)).slow);
    }
}
