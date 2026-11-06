//! Other people's cameras (spec 08, section 7.2, FR-8.47, FR-8.52 – FR-8.60): the viewer side
//! of the camera room.
//!
//! Every `mumcord.camera on` in the own channel becomes a tile. A tile that should show video
//! (Show cameras not Never, room open, cameras not hidden by the strip position, the tile not
//! hidden, the person not blocked) asks the sender with `cam-join`, waits for the bot's token,
//! binds as a subscriber of the camera broadcast and plays it through the screen-share viewer
//! pipeline ([`WatchLink`]): the same key exchange, decryption, layer filter and dav1d / libvpx
//! decoding for the codecs the webview cannot decode. Hiding unbinds and sends `cam-leave`, so
//! the sender kicks and rekeys (FR-8.57).
//!
//! The session tick (once a second) drives the rest: `want` reports of the drawn tile height
//! (FR-8.55), the "{n} fps" badge (FR-8.58) and the `cam-join` retries. A sender without room
//! answers `full` (FR-8.52): the tile shows "Too many people are watching {name}" and joins
//! again on the sender's next `on`, which it sends when a slot frees up, at most once per
//! [`JOIN_EVERY`]. A join that brings neither a token nor `full` within [`FAILED_AFTER`] failed
//! for another reason and is retried once per [`JOIN_EVERY`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use mumcord_p2p::broadcast::{BindRole, ControlKind};
use mumcord_p2p::{BroadcastEnd, BroadcastId, RelayToken, Unit};
use mumcord_screenshare::I420Frame;
use mumcord_store::CameraShow;
use tokio::sync::mpsc;

use super::send::round_want;
use super::session::{ServerCameras, broadcast_hex, cert_hash};
use super::signal::{CameraMessage, Codec};
use crate::ServerId;
use crate::peer::RelayUse;
use crate::screen_share::ScreenFrameSink;
use crate::screen_share::broadcast::cert_key;
use crate::screen_share::broadcast::link::{OnEnd, Subscriber};
use crate::screen_share::sound::ShareSound;
use crate::screen_share::watch::WatchLink;
use crate::screen_share::wire::{self, Frame};
use crate::session::{Engine, EngineEvent, SessionCtx, lock};

/// The sender takes at most one `cam-join` per viewer in this time (section 6.5).
const JOIN_EVERY: Duration = Duration::from_secs(10);
/// A join without a token or `full` after this long failed (the sender or the bot never
/// answered).
const FAILED_AFTER: Duration = Duration::from_secs(10);
/// Below this many frames per second over the last [`FPS_SAMPLES`] seconds a tile shows the
/// "{n} fps" badge (FR-8.58).
const WEAK_FPS: u32 = 25;
const FPS_SAMPLES: usize = 3;
/// Data saver caps a camera at its 15 fps layer (FR-8.56).
const DATA_SAVER_FPS: u32 = 15;
/// "Best" wants the full 720p whatever the tile size (FR-8.56).
const BEST_WANT: u16 = 720;

/// Where a camera tile is (section 7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileState {
    /// The person's camera is on; nothing is downloaded yet.
    Announced,
    /// `cam-join` sent (or about to be, within the sender's join limit); waiting for the token,
    /// the bind and the key.
    Joining,
    /// Bound and receiving video.
    Playing,
    /// No room for this viewer: "Too many people are watching {name}" (FR-8.52).
    Full,
    /// The join brought no answer: "Couldn't join {name}'s camera"; retried once per 10 s.
    Failed,
    /// Not downloading: the tile, the room or all cameras are hidden, or Show cameras is Never.
    Hidden,
}

/// "Quality for me" of one tile (FR-8.56).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TileQuality {
    /// Fits the tile.
    #[default]
    Auto,
    /// Up to 15 fps; `want` as Auto.
    DataSaver,
    /// Up to 720p · 30 fps whatever the tile size.
    Best,
}

/// One camera in the room as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraTile {
    /// The camera owner's session.
    pub sharer: u32,
    pub state: TileState,
    /// The user chose "Hide {name}'s video" on this tile.
    pub hidden: bool,
    pub quality: TileQuality,
    /// Data saver is in effect (the tile's choice or the Settings switch, FR-8.56).
    pub data_saver: bool,
    /// The "{n} fps" badge with "Weak connection" (FR-8.58); `None` hides it.
    pub fps_badge: Option<u32>,
}

/// The cameras of the own channel on one server (blocked people are left out, FR-8.59).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraRoomView {
    pub server_id: ServerId,
    /// The room is open (the engine opens it for "Show cameras: Always" when the first camera
    /// turns on, FR-8.53; the UI reports opening and closing).
    pub open: bool,
    /// The strip position Hidden is in effect (FR-8.71).
    pub videos_hidden: bool,
    /// Ordered by session.
    pub tiles: Vec<CameraTile>,
}

/// What a tile's state change asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// Send `cam-join` and wait for the token.
    Join,
    /// Drop the subscriber; with `leave` also send `cam-leave` (the sender may have admitted).
    Unbind { leave: bool },
}

/// Frames handed to the UI per second over the last seconds, for the badge.
#[derive(Debug, Default, Clone)]
struct FpsMeter {
    samples: [u32; FPS_SAMPLES],
    count: usize,
    /// Highest one-second rate seen: what the sender sends when nothing is lost (a camera sent
    /// at 15 fps is not a weak connection).
    peak: u32,
}

impl FpsMeter {
    fn push(&mut self, fps: u32) {
        self.samples.rotate_left(1);
        self.samples[FPS_SAMPLES - 1] = fps;
        self.count = (self.count + 1).min(FPS_SAMPLES);
        self.peak = self.peak.max(fps);
    }

    /// The badge when the last seconds brought fewer than [`WEAK_FPS`] and clearly fewer than
    /// expected; `cap` is the viewer's own frame-rate cap.
    fn badge(&self, cap: Option<u32>) -> Option<u32> {
        if self.count < FPS_SAMPLES {
            return None;
        }
        let average = self.samples.iter().sum::<u32>() / FPS_SAMPLES as u32;
        let expected = self.peak.min(cap.unwrap_or(u32::MAX)).max(1);
        (average < WEAK_FPS && average * 10 < expected * 9).then_some(average)
    }
}

/// Counts the frames of one camera on their way to the UI's sink.
struct CountingSink {
    inner: Option<Arc<dyn ScreenFrameSink>>,
    frames: Arc<AtomicU32>,
}

impl ScreenFrameSink for CountingSink {
    fn on_encoded(&self, server_id: ServerId, sharer: u32, frame: &Frame) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        if let Some(inner) = &self.inner {
            inner.on_encoded(server_id, sharer, frame);
        }
    }

    fn on_decoded(&self, server_id: ServerId, sharer: u32, codec: wire::Codec, frame: &I420Frame) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        if let Some(inner) = &self.inner {
            inner.on_decoded(server_id, sharer, codec, frame);
        }
    }
}

/// A bound camera.
struct TileWatch {
    link: WatchLink,
    /// The subscriber link's unit sender, for `want`.
    units: mpsc::Sender<Vec<u8>>,
    frames: Arc<AtomicU32>,
    sent_want: Option<u16>,
    data_saver: bool,
}

struct Tile {
    /// The broadcast id from `on` (16 hex).
    id: String,
    state: TileState,
    hidden: bool,
    blocked: bool,
    quality: TileQuality,
    /// The tile height the UI draws, in device pixels; 0 until reported.
    height: u32,
    last_join: Option<Instant>,
    /// When the current join went out; `None` while it waits for the join limit.
    joining_since: Option<Instant>,
    /// Tells a bind apart from an earlier, cancelled one.
    attempt: u64,
    /// A full camera was announced again: join once more (FR-8.52).
    retry: bool,
    watch: Option<TileWatch>,
    fps: FpsMeter,
    badge: Option<u32>,
}

impl Tile {
    fn new(id: String) -> Self {
        Self {
            id,
            state: TileState::Announced,
            hidden: false,
            blocked: false,
            quality: TileQuality::Auto,
            height: 0,
            last_join: None,
            joining_since: None,
            attempt: 0,
            retry: false,
            watch: None,
            fps: FpsMeter::default(),
            badge: None,
        }
    }

    /// Moves the tile towards showing video (`wants`) or not, along section 7.2.
    fn step(&mut self, wants: bool, now: Instant) -> Option<Act> {
        if !wants {
            let act = match self.state {
                // A failed join may still have been admitted by the sender.
                TileState::Joining | TileState::Playing | TileState::Failed => {
                    Some(Act::Unbind { leave: true })
                }
                TileState::Full => Some(Act::Unbind { leave: false }),
                TileState::Announced | TileState::Hidden => None,
            };
            self.state = TileState::Hidden;
            self.joining_since = None;
            self.retry = false;
            if act.is_some() {
                self.attempt += 1;
            }
            return act;
        }
        match self.state {
            TileState::Announced | TileState::Hidden => {
                self.state = TileState::Joining;
                self.joining_since = None;
                self.try_join(now)
            }
            TileState::Joining => match self.joining_since {
                None => self.try_join(now),
                Some(since) if now.saturating_duration_since(since) >= FAILED_AFTER => {
                    self.state = TileState::Failed;
                    None
                }
                Some(_) => None,
            },
            // "Retry on the next state announcement", within the sender's join limit.
            TileState::Full if self.retry => {
                let act = self.try_join(now);
                self.retry = act.is_none();
                act
            }
            TileState::Full => None,
            TileState::Failed => self.try_join(now),
            TileState::Playing => None,
        }
    }

    fn try_join(&mut self, now: Instant) -> Option<Act> {
        if self
            .last_join
            .is_some_and(|at| now.saturating_duration_since(at) < JOIN_EVERY)
        {
            return None;
        }
        self.last_join = Some(now);
        if self.state == TileState::Joining {
            self.joining_since = Some(now);
        }
        self.attempt += 1;
        Some(Act::Join)
    }

    /// The sender answered `full` for broadcast `id`; `false` if that changes nothing.
    fn full(&mut self, id: &str) -> bool {
        if self.id != id
            || !matches!(
                self.state,
                TileState::Joining | TileState::Failed | TileState::Full
            )
        {
            return false;
        }
        self.state = TileState::Full;
        self.joining_since = None;
        self.retry = false;
        true
    }

    fn data_saver(&self, everyone: bool) -> bool {
        match self.quality {
            TileQuality::DataSaver => true,
            TileQuality::Best => false,
            TileQuality::Auto => everyone,
        }
    }

    /// The tile height to report (FR-8.55); `None` until the UI drew the tile.
    fn want(&self) -> Option<u16> {
        if self.quality == TileQuality::Best {
            return Some(BEST_WANT);
        }
        let height = u16::try_from(self.height).unwrap_or(u16::MAX);
        (height > 0).then(|| u16::try_from(round_want(height)).unwrap_or(BEST_WANT))
    }

    fn view(&self, sharer: u32, everyone_data_saver: bool) -> CameraTile {
        CameraTile {
            sharer,
            state: self.state,
            hidden: self.hidden,
            quality: self.quality,
            data_saver: self.data_saver(everyone_data_saver),
            fps_badge: self.badge.filter(|_| self.state == TileState::Playing),
        }
    }
}

#[derive(Default)]
struct Room {
    open: bool,
    videos_hidden: bool,
    tiles: BTreeMap<u32, Tile>,
    /// The view last sent to the UI.
    sent: Option<CameraRoomView>,
}

/// The settings the viewer side follows.
#[derive(Debug, Clone, Copy)]
struct Prefs {
    show: CameraShow,
    data_saver: bool,
}

/// The viewer side's state for all servers, kept in the engine's camera state.
#[derive(Default)]
pub(crate) struct Receiving {
    rooms: HashMap<ServerId, Room>,
    sink: Option<Arc<dyn ScreenFrameSink>>,
    /// Codecs decoded in the engine because the webview cannot (FR-7.59).
    decode_here: Vec<wire::Codec>,
    prefs: Option<Prefs>,
}

impl Room {
    fn view(&self, server_id: ServerId, everyone_data_saver: bool) -> CameraRoomView {
        CameraRoomView {
            server_id,
            open: self.open,
            videos_hidden: self.videos_hidden,
            tiles: self
                .tiles
                .iter()
                .filter(|(_, t)| !t.blocked)
                .map(|(s, t)| t.view(*s, everyone_data_saver))
                .collect(),
        }
    }

    /// Whether the room shows video at all; each tile adds its own Hide and the block rule.
    fn shows(&self, show: CameraShow) -> bool {
        show != CameraShow::Never && self.open && !self.videos_hidden
    }

    /// Per-tile choices last until the room closes (section 7.1).
    fn close(&mut self) {
        self.open = false;
        for tile in self.tiles.values_mut() {
            tile.hidden = false;
            tile.quality = TileQuality::Auto;
        }
    }
}

/// A message for one sharer, sent after the camera state is unlocked.
enum Out {
    Join {
        sharer: u32,
        id: String,
        attempt: u64,
    },
    Leave {
        sharer: u32,
        id: String,
    },
}

fn send_want(watch: &mut TileWatch, want: u16) {
    if watch.sent_want == Some(want) {
        return;
    }
    match Unit::control(ControlKind::Want, &want.to_be_bytes()) {
        // A full queue just means the report waits for the next second.
        Ok(unit) => {
            if watch.units.try_send(unit).is_ok() {
                watch.sent_want = Some(want);
            }
        }
        Err(e) => tracing::warn!(error = %e, "camera want not sent"),
    }
}

fn apply_data_saver(watch: &mut TileWatch, on: bool) {
    if watch.data_saver != on {
        watch.data_saver = on;
        watch.link.set_max_fps(on.then_some(DATA_SAVER_FPS));
    }
}

impl Engine {
    /// Where the frames of every camera the user watches go (FR-8.54); same hand-over as screen
    /// sharing, with the camera owner's session as `sharer`. Applies to cameras bound afterwards.
    pub fn camera_set_frame_sink(&self, sink: Arc<dyn ScreenFrameSink>) {
        lock(self.camera_state()).recv.sink = Some(sink);
    }

    /// Decodes camera frames of `codecs` in the engine (dav1d / libvpx) instead of handing
    /// encoded frames to the webview, which cannot decode them (FR-8.54, FR-7.59).
    pub fn camera_set_decode_codecs(&self, codecs: &[wire::Codec]) {
        let mut cameras = lock(self.camera_state());
        cameras.recv.decode_here = codecs.to_vec();
        for room in cameras.recv.rooms.values() {
            for watch in room.tiles.values().filter_map(|t| t.watch.as_ref()) {
                watch.link.set_decode_codecs(codecs);
            }
        }
    }

    /// The cameras of the own channel on `server_id`.
    pub fn camera_room(&self, server_id: ServerId) -> CameraRoomView {
        let everyone = self.camera_prefs().data_saver;
        lock(self.camera_state())
            .recv
            .rooms
            .get(&server_id)
            .map_or_else(
                || Room::default().view(server_id, everyone),
                |r| r.view(server_id, everyone),
            )
    }

    /// The user opened or closed the camera room (FR-8.53, FR-8.57). Closing unbinds every
    /// camera and resets the per-tile choices; opening joins them again.
    pub fn camera_room_set_open(&self, server_id: ServerId, open: bool) {
        self.camera_room_change(server_id, |room| {
            if open {
                room.open = true;
            } else {
                room.close();
            }
        });
    }

    /// The strip position Hidden came into effect or ended (FR-8.71): hidden unbinds every
    /// camera like Hide video, while the header keeps counting them.
    pub fn camera_room_set_videos_hidden(&self, server_id: ServerId, hidden: bool) {
        self.camera_room_change(server_id, |room| room.videos_hidden = hidden);
    }

    /// Hide / Show video on one tile (FR-8.57).
    pub fn camera_tile_set_hidden(&self, server_id: ServerId, sharer: u32, hidden: bool) {
        self.camera_room_change(server_id, |room| {
            if let Some(tile) = room.tiles.get_mut(&sharer) {
                tile.hidden = hidden;
            }
        });
    }

    /// Quality for me on one tile (FR-8.56).
    pub fn camera_tile_set_quality(&self, server_id: ServerId, sharer: u32, quality: TileQuality) {
        self.camera_room_change(server_id, |room| {
            if let Some(tile) = room.tiles.get_mut(&sharer)
                && tile.quality != quality
            {
                tile.quality = quality;
                tile.fps = FpsMeter::default();
                tile.badge = None;
            }
        });
    }

    /// The height the UI draws `sharer`'s tile at, in device pixels; reported to the sender as
    /// `want` on the next second (FR-8.55).
    pub fn camera_tile_set_height(&self, server_id: ServerId, sharer: u32, height: u32) {
        if let Some(tile) = lock(self.camera_state())
            .recv
            .rooms
            .get_mut(&server_id)
            .and_then(|r| r.tiles.get_mut(&sharer))
        {
            tile.height = height;
        }
    }

    /// Asks `sharer`'s camera for a keyframe, e.g. after the UI's decoder failed on a frame.
    pub fn camera_request_keyframe(&self, server_id: ServerId, sharer: u32) {
        if let Some(watch) = lock(self.camera_state())
            .recv
            .rooms
            .get(&server_id)
            .and_then(|r| r.tiles.get(&sharer))
            .and_then(|t| t.watch.as_ref())
        {
            watch.link.request_keyframe();
        }
    }

    /// Show cameras or Data saver changed in Settings (FR-8.53, FR-8.56).
    pub(crate) fn camera_settings_changed(&self, settings: &mumcord_store::Settings) {
        lock(self.camera_state()).recv.prefs = Some(Prefs {
            show: settings.camera_show,
            data_saver: settings.camera_data_saver,
        });
        for ctx in self.session_ctxs() {
            self.camera_sync(&ctx, false);
        }
    }

    /// The block list changed: a camera shown of someone now blocked unbinds at once, and one
    /// of someone unblocked may show again (FR-8.59).
    pub(crate) fn camera_blocks_changed(&self) {
        for ctx in self.session_ctxs() {
            self.camera_sync(&ctx, false);
        }
    }

    fn camera_prefs(&self) -> Prefs {
        if let Some(prefs) = lock(self.camera_state()).recv.prefs {
            return prefs;
        }
        let prefs = match self.settings() {
            Ok(s) => Prefs {
                show: s.camera_show,
                data_saver: s.camera_data_saver,
            },
            Err(e) => {
                tracing::warn!(error = %e, "camera settings not readable; using the defaults");
                Prefs {
                    show: CameraShow::default(),
                    data_saver: false,
                }
            }
        };
        lock(self.camera_state()).recv.prefs = Some(prefs);
        prefs
    }

    fn camera_room_change(&self, server_id: ServerId, change: impl FnOnce(&mut Room)) {
        change(
            lock(self.camera_state())
                .recv
                .rooms
                .entry(server_id)
                .or_default(),
        );
        if let Ok(ctx) = self.session_ctx_for(server_id) {
            self.camera_sync(&ctx, false);
        }
    }

    /// Brings every tile on `ctx`'s server to where it should be and sends what that takes;
    /// on the second `tick` also the `want` reports and the fps badges.
    fn camera_sync(&self, ctx: &SessionCtx, tick: bool) {
        let prefs = self.camera_prefs();
        let blocked = self.camera_blocked(ctx);
        let now = Instant::now();
        let mut out = Vec::new();
        let mut unbound = Vec::new();
        let view = {
            let mut cameras = lock(self.camera_state());
            let Some(room) = cameras.recv.rooms.get_mut(&ctx.server_id) else {
                return;
            };
            let shows = room.shows(prefs.show);
            for (&sharer, tile) in &mut room.tiles {
                tile.blocked = blocked.contains(&sharer);
                let wants = shows && !tile.hidden && !tile.blocked;
                match tile.step(wants, now) {
                    Some(Act::Join) => out.push(Out::Join {
                        sharer,
                        id: tile.id.clone(),
                        attempt: tile.attempt,
                    }),
                    Some(Act::Unbind { leave }) => {
                        unbound.extend(tile.watch.take());
                        tile.badge = None;
                        if leave {
                            out.push(Out::Leave {
                                sharer,
                                id: tile.id.clone(),
                            });
                        }
                    }
                    None => {}
                }
                let data_saver = tile.data_saver(prefs.data_saver);
                let want = tile.want();
                if let Some(watch) = tile.watch.as_mut() {
                    apply_data_saver(watch, data_saver);
                    if tick {
                        if let Some(want) = want {
                            send_want(watch, want);
                        }
                        tile.fps.push(watch.frames.swap(0, Ordering::Relaxed));
                        tile.badge = tile.fps.badge(data_saver.then_some(DATA_SAVER_FPS));
                    }
                }
            }
            let view = room.view(ctx.server_id, prefs.data_saver);
            (room.sent.as_ref() != Some(&view)).then(|| {
                room.sent = Some(view.clone());
                view
            })
        };
        // The links close outside the camera lock.
        drop(unbound);
        for message in out {
            match message {
                Out::Join {
                    sharer,
                    id,
                    attempt,
                } => self.camera_send_join(ctx, sharer, id, attempt),
                Out::Leave { sharer, id } => {
                    super::signal::send(ctx, &[sharer], &CameraMessage::CamLeave { id });
                }
            }
        }
        if let Some(view) = view {
            ctx.events.emit(EngineEvent::CameraRoomChanged(view));
        }
    }

    /// The people in the own channel `ctx` blocks.
    fn camera_blocked(&self, ctx: &SessionCtx) -> HashSet<u32> {
        let sessions: Vec<u32> = ctx.handle.with_state(|s| s.users.keys().copied().collect());
        sessions
            .into_iter()
            .filter(|s| crate::blocking::is_blocked(ctx, self.blocked_state(), *s))
            .collect()
    }

    /// Sends `cam-join` for camera `id` of `sharer` and binds once the bot's token comes.
    fn camera_send_join(&self, ctx: &SessionCtx, sharer: u32, id: String, attempt: u64) {
        let token = self.camera_await_token(ctx.server_id, sharer);
        let join = CameraMessage::CamJoin {
            id: id.clone(),
            // Both play: whatever the webview cannot decode is decoded here.
            codecs: vec![Codec::Av1, Codec::Vp9],
        };
        if !super::signal::send(ctx, &[sharer], &join) {
            // The tile stays Joining; the tick retries within the join limit.
            return;
        }
        let engine = self.clone();
        let ctx = ctx.clone();
        self.runtime().spawn(async move {
            // A later join or a reset drops the waiting sender.
            let Ok((bid, token)) = token.await else {
                return;
            };
            if broadcast_hex(bid) != id {
                tracing::debug!("camera token for another broadcast ignored");
                return;
            }
            engine.camera_bind(&ctx, sharer, attempt, bid, token).await;
        });
    }

    /// Binds to `sharer`'s camera broadcast as a subscriber and starts playing it.
    async fn camera_bind(
        &self,
        ctx: &SessionCtx,
        sharer: u32,
        attempt: u64,
        id: BroadcastId,
        token: RelayToken,
    ) {
        let server_id = ctx.server_id;
        let opened = async {
            let ann = self
                .relays()
                .broadcast_announcement_for(server_id, RelayUse::Camera)
                .map_err(|_| "no relay for cameras")?;
            let hash = cert_hash(ctx, sharer).ok_or("the sender has no certificate")?;
            let (p2p, identity) = self
                .p2p_with_identity()
                .await
                .ok_or("no identity for the key exchange")?;
            let me = cert_key(&identity).ok_or("no key for the key exchange")?;
            let link = p2p
                .broadcast(&ann, &token, BindRole::Subscriber)
                .await
                .map_err(|e| {
                    tracing::info!(error = %e, "camera bind refused");
                    "the relay refused the bind"
                })?;
            Ok::<_, &'static str>((me, hash, link))
        }
        .await;
        let (me, hash, link) = match opened {
            Ok(opened) => opened,
            Err(why) => {
                tracing::info!(why, "camera not received");
                // Joining again after the join limit; not "full", the token came.
                if let Some(tile) = lock(self.camera_state())
                    .recv
                    .rooms
                    .get_mut(&server_id)
                    .and_then(|r| r.tiles.get_mut(&sharer))
                    .filter(|t| t.attempt == attempt)
                {
                    tile.joining_since = None;
                }
                return;
            }
        };
        let weak = self.downgrade();
        let on_end: OnEnd = Box::new(move |end| {
            if let Some(engine) = weak.upgrade() {
                engine.camera_watch_ended(server_id, sharer, attempt, end);
            }
        });
        let (subscriber, frames, units) =
            Subscriber::start(self.runtime(), id, me, hash, link, on_end);
        let counter = Arc::new(AtomicU32::new(0));
        let (sink, decode_here) = {
            let cameras = lock(self.camera_state());
            (cameras.recv.sink.clone(), cameras.recv.decode_here.clone())
        };
        let link = WatchLink::start_broadcast(
            self.runtime(),
            server_id,
            sharer,
            Some(Arc::new(CountingSink {
                inner: sink,
                frames: counter.clone(),
            })),
            // A camera has no sound; the share's sound source stays empty.
            ShareSound::new(self.voice_shared().clone(), 0),
            frames,
            units.clone(),
            subscriber,
        );
        link.set_decode_codecs(&decode_here);
        let watch = TileWatch {
            link,
            units,
            frames: counter,
            sent_want: None,
            data_saver: false,
        };
        let stale = {
            let mut cameras = lock(self.camera_state());
            match cameras
                .recv
                .rooms
                .get_mut(&server_id)
                .and_then(|r| r.tiles.get_mut(&sharer))
                .filter(|t| {
                    t.attempt == attempt
                        && matches!(
                            t.state,
                            TileState::Joining | TileState::Full | TileState::Failed
                        )
                }) {
                Some(tile) => {
                    tile.state = TileState::Playing;
                    tile.joining_since = None;
                    tile.fps = FpsMeter::default();
                    tile.badge = None;
                    tile.watch = Some(watch);
                    None
                }
                None => Some(watch),
            }
        };
        // Hidden or gone meanwhile: the unbind already told the sender.
        drop(stale);
        self.camera_sync(ctx, false);
    }

    /// A camera link ended by itself (the sender turned off or kicked this viewer, or the relay
    /// went away): the tile joins again within the join limit unless `off` removes it first.
    fn camera_watch_ended(
        &self,
        server_id: ServerId,
        sharer: u32,
        attempt: u64,
        end: BroadcastEnd,
    ) {
        tracing::info!(?end, "camera link ended");
        let watch = {
            let mut cameras = lock(self.camera_state());
            let Some(tile) = cameras
                .recv
                .rooms
                .get_mut(&server_id)
                .and_then(|r| r.tiles.get_mut(&sharer))
                .filter(|t| t.attempt == attempt && t.state == TileState::Playing)
            else {
                return;
            };
            tile.state = TileState::Joining;
            tile.joining_since = None;
            tile.badge = None;
            tile.watch.take()
        };
        drop(watch);
        if let Ok(ctx) = self.session_ctx_for(server_id) {
            self.camera_sync(&ctx, false);
        }
    }
}

/// `on` from `sharer` in the own channel (FR-8.8): a new tile, or a new broadcast on an
/// existing one. With "Show cameras: Always" the first camera opens the room (FR-8.53).
pub(crate) fn on_camera_on(engine: &Engine, ctx: &SessionCtx, sharer: u32, id: String) {
    let show = engine.camera_prefs().show;
    let replaced = {
        let mut cameras = lock(engine.camera_state());
        let room = cameras.recv.rooms.entry(ctx.server_id).or_default();
        if room.tiles.is_empty() && show == CameraShow::Always {
            room.open = true;
        }
        match room.tiles.get_mut(&sharer) {
            Some(tile) if tile.id == id => {
                tile.retry = tile.state == TileState::Full;
                None
            }
            Some(tile) => {
                let old = std::mem::replace(tile, Tile::new(id));
                // The choices made on the tile stay with the person.
                tile.hidden = old.hidden;
                tile.quality = old.quality;
                tile.height = old.height;
                tile.last_join = old.last_join;
                tile.attempt = old.attempt + 1;
                Some(old)
            }
            None => {
                room.tiles.insert(sharer, Tile::new(id));
                None
            }
        }
    };
    drop(replaced);
    engine.camera_sync(ctx, false);
}

/// `full` from `sharer` (FR-8.52): its camera has no room for this client.
pub(crate) fn on_camera_full(engine: &Engine, ctx: &SessionCtx, sharer: u32, id: &str) {
    let changed = lock(engine.camera_state())
        .recv
        .rooms
        .get_mut(&ctx.server_id)
        .and_then(|r| r.tiles.get_mut(&sharer))
        .is_some_and(|t| t.full(id));
    if changed {
        engine.camera_sync(ctx, false);
    }
}

/// `sharer`'s camera is gone from the own channel: `off`, the person left or moved away.
pub(crate) fn on_camera_gone(engine: &Engine, ctx: &SessionCtx, sharer: u32) {
    let removed = lock(engine.camera_state())
        .recv
        .rooms
        .get_mut(&ctx.server_id)
        .and_then(|r| r.tiles.remove(&sharer));
    if removed.is_some() {
        drop(removed);
        engine.camera_sync(ctx, false);
    }
}

/// The own user moved to another channel: the cameras of the old one are gone, and the new
/// channel's senders announce theirs (FR-8.61 shows only the own channel).
pub(crate) fn on_own_channel_changed(engine: &Engine, ctx: &SessionCtx) {
    let room = lock(engine.camera_state())
        .recv
        .rooms
        .remove(&ctx.server_id);
    let Some(room) = room else {
        return;
    };
    drop(room);
    let view = Room::default().view(ctx.server_id, false);
    ctx.events.emit(EngineEvent::CameraRoomChanged(view));
}

/// Session tick, once a second: `want` reports, fps badges and join retries.
pub(crate) fn on_tick(engine: &Engine, ctx: &SessionCtx) {
    engine.camera_sync(ctx, true);
}

/// The connection to `server_id` ended or started afresh: every camera there is gone. Returns
/// the empty room to announce when there was one.
pub(crate) fn on_connection_reset(
    state: &ServerCameras,
    server_id: ServerId,
) -> Option<CameraRoomView> {
    let room = lock(state).recv.rooms.remove(&server_id)?;
    drop(room);
    Some(Room::default().view(server_id, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef";

    fn later(now: Instant, secs: u64) -> Instant {
        now + Duration::from_secs(secs)
    }

    #[test]
    fn a_shown_camera_joins_plays_and_unbinds_with_cam_leave_when_hidden() {
        let now = Instant::now();
        let mut tile = Tile::new(ID.into());
        assert_eq!(tile.step(true, now), Some(Act::Join));
        assert_eq!(tile.state, TileState::Joining);
        assert_eq!(tile.step(true, later(now, 1)), None);
        tile.state = TileState::Playing;
        assert_eq!(tile.step(true, later(now, 2)), None);
        assert_eq!(
            tile.step(false, later(now, 3)),
            Some(Act::Unbind { leave: true })
        );
        assert_eq!(tile.state, TileState::Hidden);
    }

    #[test]
    fn a_camera_that_should_not_show_never_joins() {
        let mut tile = Tile::new(ID.into());
        assert_eq!(tile.step(false, Instant::now()), None);
        assert_eq!(tile.state, TileState::Hidden);
        assert_eq!(tile.last_join, None);
    }

    #[test]
    fn showing_again_waits_for_the_senders_join_limit() {
        let now = Instant::now();
        let mut tile = Tile::new(ID.into());
        assert_eq!(tile.step(true, now), Some(Act::Join));
        tile.step(false, later(now, 2));
        assert_eq!(tile.step(true, later(now, 3)), None);
        assert_eq!(tile.state, TileState::Joining);
        assert_eq!(tile.step(true, later(now, 9)), None);
        assert_eq!(tile.step(true, later(now, 10)), Some(Act::Join));
    }

    #[test]
    fn a_join_without_any_answer_fails_and_retries_every_ten_seconds() {
        let now = Instant::now();
        let mut tile = Tile::new(ID.into());
        tile.step(true, now);
        assert_eq!(tile.step(true, later(now, 9)), None);
        assert_eq!(tile.step(true, later(now, 10)), None);
        assert_eq!(tile.state, TileState::Failed);
        assert_eq!(tile.step(true, later(now, 11)), Some(Act::Join));
        assert_eq!(tile.state, TileState::Failed);
        assert_eq!(tile.step(true, later(now, 15)), None);
        assert_eq!(tile.step(true, later(now, 21)), Some(Act::Join));
        // The sender may have admitted one of the joins.
        assert_eq!(
            tile.step(false, later(now, 22)),
            Some(Act::Unbind { leave: true })
        );
    }

    #[test]
    fn full_waits_for_the_next_announcement_and_the_join_limit() {
        let now = Instant::now();
        let mut tile = Tile::new(ID.into());
        assert_eq!(tile.step(true, now), Some(Act::Join));
        assert!(
            !tile.full("ffffffffffffffff"),
            "another broadcast's full is ignored"
        );
        assert!(tile.full(ID));
        assert_eq!(tile.state, TileState::Full);
        // Neither the old join's timeout nor the ticks retry without an announcement.
        assert_eq!(tile.step(true, later(now, 30)), None);
        assert_eq!(tile.state, TileState::Full);
        // `on` again: retried at once, as the last join is old enough.
        tile.retry = true;
        assert_eq!(tile.step(true, later(now, 31)), Some(Act::Join));
        assert_eq!(tile.step(true, later(now, 32)), None);
        // Full again, announced again 3 s later: the retry waits for the join limit.
        assert!(tile.full(ID));
        tile.retry = true;
        assert_eq!(tile.step(true, later(now, 34)), None);
        assert_eq!(tile.step(true, later(now, 40)), None);
        assert_eq!(tile.step(true, later(now, 41)), Some(Act::Join));
        assert_eq!(tile.state, TileState::Full);
        // A failed join turns full when the sender answers late.
        let mut late = Tile::new(ID.into());
        late.step(true, now);
        late.step(true, later(now, 10));
        assert_eq!(late.state, TileState::Failed);
        assert!(late.full(ID));
        // Hiding a full camera unbinds without cam-leave: the sender never admitted it.
        assert_eq!(
            tile.step(false, later(now, 42)),
            Some(Act::Unbind { leave: false })
        );
        // A playing tile ignores a stray full.
        tile.state = TileState::Playing;
        assert!(!tile.full(ID));
    }

    #[test]
    fn every_join_and_unbind_starts_a_new_attempt() {
        let now = Instant::now();
        let mut tile = Tile::new(ID.into());
        tile.step(true, now);
        let first = tile.attempt;
        tile.step(false, later(now, 1));
        assert!(tile.attempt > first);
    }

    #[test]
    fn the_room_shows_video_only_when_open_not_hidden_and_not_never() {
        let mut room = Room {
            open: true,
            ..Room::default()
        };
        assert!(room.shows(CameraShow::Always));
        assert!(room.shows(CameraShow::OnOpen));
        assert!(!room.shows(CameraShow::Never));
        room.videos_hidden = true;
        assert!(!room.shows(CameraShow::Always));
        room.videos_hidden = false;
        room.open = false;
        assert!(!room.shows(CameraShow::Always));
    }

    #[test]
    fn closing_the_room_resets_the_tile_choices() {
        let mut room = Room {
            open: true,
            ..Room::default()
        };
        let mut tile = Tile::new(ID.into());
        tile.hidden = true;
        tile.quality = TileQuality::Best;
        room.tiles.insert(7, tile);
        room.close();
        assert!(!room.open);
        assert!(!room.tiles[&7].hidden);
        assert_eq!(room.tiles[&7].quality, TileQuality::Auto);
    }

    #[test]
    fn blocked_people_have_no_tile() {
        let mut room = Room::default();
        let mut tile = Tile::new(ID.into());
        tile.blocked = true;
        room.tiles.insert(7, tile);
        room.tiles.insert(8, Tile::new(ID.into()));
        let view = room.view(1, false);
        assert_eq!(view.tiles.len(), 1);
        assert_eq!(view.tiles[0].sharer, 8);
    }

    #[test]
    fn want_is_the_rounded_tile_height_or_720_for_best() {
        let mut tile = Tile::new(ID.into());
        assert_eq!(tile.want(), None);
        tile.height = 200;
        assert_eq!(tile.want(), Some(360));
        tile.height = 100;
        assert_eq!(tile.want(), Some(180));
        tile.height = 5000;
        assert_eq!(tile.want(), Some(720));
        tile.height = 100;
        tile.quality = TileQuality::Best;
        assert_eq!(tile.want(), Some(720));
        tile.quality = TileQuality::DataSaver;
        assert_eq!(tile.want(), Some(180));
    }

    #[test]
    fn data_saver_follows_the_tile_then_the_setting() {
        let mut tile = Tile::new(ID.into());
        assert!(!tile.data_saver(false));
        assert!(tile.data_saver(true));
        tile.quality = TileQuality::Best;
        assert!(!tile.data_saver(true));
        tile.quality = TileQuality::DataSaver;
        assert!(tile.data_saver(false));
    }

    fn meter(samples: &[u32]) -> FpsMeter {
        let mut meter = FpsMeter::default();
        for &fps in samples {
            meter.push(fps);
        }
        meter
    }

    #[test]
    fn the_badge_shows_below_25_fps_over_three_seconds() {
        assert_eq!(meter(&[7, 7]).badge(None), None, "needs three seconds");
        assert_eq!(meter(&[30, 7, 7, 7]).badge(None), Some(7));
        assert_eq!(meter(&[30, 30, 30, 30]).badge(None), None);
        assert_eq!(meter(&[30, 30, 20, 20]).badge(None), Some(23));
        assert_eq!(meter(&[30, 0, 0, 0]).badge(None), Some(0));
    }

    #[test]
    fn a_camera_sent_at_15_fps_or_capped_by_data_saver_is_not_weak() {
        assert_eq!(meter(&[15, 15, 15]).badge(None), None);
        assert_eq!(meter(&[30, 15, 15, 15]).badge(Some(15)), None);
        assert_eq!(meter(&[30, 7, 7, 7]).badge(Some(15)), Some(7));
    }

    #[test]
    fn a_stalled_camera_shows_zero_fps() {
        assert_eq!(meter(&[0, 0, 0]).badge(None), Some(0));
    }
}
