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

use mumcord_p2p::broadcast::{Content, ControlKind, MAX_TEMPORAL_LAYER, VideoCodec, split_slot};
use mumcord_p2p::{BroadcastEnd, BroadcastId, BroadcastLink, Unit};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::crypto::{FrameOpener, FrameSealer};
use super::keyx::{self, CertKey, PendingRequest, Reassembler, SharerChannel, ViewerChannel};
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

/// Sends `message` as `key`/`keyreq` chunks; `slot` is set for `key` (publisher side).
pub(crate) async fn send_chunks(
    units: &mpsc::Sender<Vec<u8>>,
    kind: ControlKind,
    slot: Option<u16>,
    message: &[u8],
    set_id: u8,
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
        let unit = match Unit::control(kind, &payload) {
            Ok(unit) => unit,
            Err(e) => {
                tracing::warn!(error = %e, "broadcast key chunk not sent");
                return true;
            }
        };
        if units.send(unit).await.is_err() {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Publisher
// ---------------------------------------------------------------------------------------------

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
    /// runs when the link ends by itself (the relay or the network).
    pub fn start(
        rt: &tokio::runtime::Handle,
        id: BroadcastId,
        me: CertKey,
        link: BroadcastLink,
        frames: Subscription,
        on_end: OnEnd,
    ) -> Result<Self, super::crypto::CryptoError> {
        let sealer = Arc::new(Mutex::new(FrameSealer::new()?));
        let frames = Arc::new(frames);
        let admitted = Arc::new(Mutex::new(HashSet::new()));
        let (commands, command_rx) = mpsc::unbounded_channel();
        let writer = rt.spawn(write_units(frames.clone(), sealer.clone(), link.sender()));
        let control = rt.spawn(
            PublisherControl {
                id,
                me,
                sealer,
                frames,
                admitted: admitted.clone(),
                channels: HashMap::new(),
                reassembler: Reassembler::new(),
                set_id: 0,
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

async fn write_units(
    frames: Arc<Subscription>,
    sealer: Arc<Mutex<FrameSealer>>,
    units: mpsc::Sender<Vec<u8>>,
) {
    loop {
        let frame = frames.next().await;
        let Some(plain) = plaintext(&frame) else {
            continue;
        };
        let mut unit = Vec::new();
        if let Err(e) = lock(&sealer).seal(content_of(&frame), &plain, &mut unit) {
            // Only a used-up counter or a frame the header can't describe gets here.
            tracing::warn!(error = %e, "broadcast frame not sealed");
            continue;
        }
        if units.send(unit).await.is_err() {
            return;
        }
    }
}

struct PublisherControl {
    id: BroadcastId,
    me: CertKey,
    sealer: Arc<Mutex<FrameSealer>>,
    frames: Arc<Subscription>,
    admitted: Arc<Mutex<HashSet<String>>>,
    channels: HashMap<u16, ViewerChannel>,
    reassembler: Reassembler<u16>,
    set_id: u8,
}

impl PublisherControl {
    async fn run(
        mut self,
        mut link: BroadcastLink,
        mut commands: mpsc::UnboundedReceiver<PubCommand>,
        on_end: OnEnd,
    ) {
        let units = link.sender();
        let end = loop {
            tokio::select! {
                unit = link.recv() => match unit {
                    Ok(unit) => {
                        if !self.on_unit(&unit, &units).await {
                            break BroadcastEnd::Lost;
                        }
                    }
                    Err(end) => break end,
                },
                Some(PubCommand::Removed(hash)) = commands.recv() => {
                    self.channels.retain(|_, ch| ch.cert_hash() != hash);
                    self.rekey(&units).await;
                }
            }
        };
        tracing::info!(?end, "screen share broadcast ended");
        on_end(end);
    }

    /// Handles one unit from the relay; false once the link is gone.
    async fn on_unit(&mut self, unit: &Unit, units: &mpsc::Sender<Vec<u8>>) -> bool {
        let Content::Control(kind) = unit.header.content else {
            // Only subscribers get media.
            return true;
        };
        match kind {
            ControlKind::KeyframeRequest => self.frames.request_keyframe(),
            ControlKind::Gone => {
                if let Some((slot, _)) = split_slot(unit.payload()) {
                    self.reassembler.forget(&slot);
                    if self.channels.remove(&slot).is_some() {
                        return self.rekey(units).await;
                    }
                }
            }
            ControlKind::KeyRequest => {
                let Some((slot, chunk)) = split_slot(unit.payload()) else {
                    return true;
                };
                match self.reassembler.push(slot, chunk, Instant::now()) {
                    Ok(Some(request)) => return self.answer(slot, &request, units).await,
                    Ok(None) => {}
                    Err(e) => tracing::debug!(error = %e, "broadcast key request chunk dropped"),
                }
            }
            // `want` is camera sharing's resolution hint; a screen share encodes one size for all.
            // Not the publisher's to receive: `key` and `layers`.
            ControlKind::Want | ControlKind::Key | ControlKind::Layers => {}
        }
        true
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
                self.set_id = self.set_id.wrapping_add(1);
                send_chunks(units, ControlKind::Key, Some(slot), &answer, self.set_id).await
            }
            Err(e) => {
                tracing::info!(error = %e, "broadcast key request refused");
                true
            }
        }
    }

    /// Stages a new key, hands it to every remaining viewer and makes the next frame a
    /// keyframe, where the switch happens.
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
            if !send_chunks(units, ControlKind::Key, Some(slot), &update, self.set_id).await {
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
        let task = rt.spawn(
            SubscriberState {
                id,
                me,
                sharer_hash,
                pending: None,
                channel: None,
                opener: FrameOpener::new(),
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
    pending: Option<PendingRequest>,
    channel: Option<SharerChannel>,
    opener: FrameOpener,
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
            || !send_control(&units, ControlKind::KeyframeRequest, &[]).await
        {
            on_end(BroadcastEnd::Lost);
            return;
        }
        let mut retry =
            tokio::time::interval_at(tokio::time::Instant::now() + KEY_RETRY, KEY_RETRY);
        let mut detached = false;
        let end = loop {
            tokio::select! {
                unit = link.recv() => match unit {
                    Ok(mut unit) => {
                        let max = *max_fps.borrow();
                        if detached {
                            self.try_old_key(&mut unit);
                        } else if !self.on_unit(&mut unit, &units, &frames, max).await {
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
        send_chunks(units, ControlKind::KeyRequest, None, &request, self.set_id).await
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
                    || send_control(units, ControlKind::KeyframeRequest, &[]).await
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
        }
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
        }
        tracing::debug!(keys = ?self.opener.key_ids(), "broadcast keys received");
    }

    async fn send_layers(&mut self, units: &mpsc::Sender<Vec<u8>>, max: MaxFps) -> bool {
        let layer = lock(&self.meter).layer_for(max.fps());
        if self.sent_layer == Some(layer) || (self.sent_layer.is_none() && max == MaxFps::Auto) {
            return true;
        }
        self.sent_layer = Some(layer);
        send_control(units, ControlKind::Layers, &[layer]).await
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

async fn send_control(units: &mpsc::Sender<Vec<u8>>, kind: ControlKind, payload: &[u8]) -> bool {
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
