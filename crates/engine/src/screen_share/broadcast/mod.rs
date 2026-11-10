//! Screen-share broadcast through the community relay (spec 07, FR-7.40 – FR-7.52).

pub(crate) mod admit;
pub mod crypto;
pub mod keyx;
pub(crate) mod link;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use mumcord_p2p::broadcast::BindRole;
use mumcord_p2p::{
    BroadcastEnd, BroadcastErrorReason, BroadcastId, P2pError, RelayRefusal, RelayToken,
};

use super::media::BROADCAST_KEY;
use super::state::{Answer, Watching};
use super::{Here, Signal, WatchBlock, emit, here, send, send_viewers};
use crate::ServerId;
use crate::peer::{RelayUnavailable, RelayUse};
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, SessionCtx, lock};
use admit::{Decision, Roster};
use keyx::CertKey;
use link::{OnEnd, Publisher, Subscriber};

/// Why the own share sends one copy per viewer instead of once through the relay (FR-7.47,
/// FR-7.49). The sharer sees it once per share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanoutUnavailable {
    /// The relay can't be used for screen sharing right now (none, not trusted, bot gone,
    /// screen sharing not allowed, …), with the reason.
    Relay(RelayUnavailable),
    /// The relay offers no broadcasts (an older relay, or broadcasts turned off).
    NotOffered,
    /// The user's P2P mode leaves the relay out ("off" or "direct only").
    Mode,
    /// The user has no client certificate to bind with and sign the key exchange.
    NoCertificate,
    /// The bot refused the broadcast (`bcast-err`), e.g. the relay runs as many as it may.
    Refused(BroadcastErrorReason),
    /// The relay was lost during the share; the running share stays on direct copies.
    Lost,
}

/// How the own share reaches its viewers (FR-7.36, FR-7.52).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanoutView {
    /// Asking the bot for a broadcast.
    Starting,
    /// Sent once via the relay, up to `limit` viewers in all.
    Relay { limit: u16 },
    /// One copy per viewer, up to 8, and why (the one-time notice of FR-7.47).
    Direct { reason: FanoutUnavailable },
}

/// The own share's relay broadcast as the engine keeps it.
pub(crate) enum Fanout {
    /// Asking the bot; `Watch` requests stay unanswered meanwhile and are sent again by the
    /// viewers.
    Starting,
    Live {
        publisher: link::Publisher,
        roster: admit::Roster,
        limit: u16,
        /// Certificate hashes of the relay viewers by session, for the rekey when one leaves.
        hashes: BTreeMap<u32, String>,
        /// The last report of each relay viewer and when it came.
        reports: BTreeMap<u32, (link::Received, Instant)>,
    },
    /// Direct copies only, for the rest of the share.
    Off(FanoutUnavailable),
}

impl Fanout {
    /// The viewers that watch through the relay.
    pub fn relay_viewers(&self) -> Vec<u32> {
        match self {
            Self::Live { roster, .. } => roster.relay_viewers(),
            _ => Vec::new(),
        }
    }

    pub fn view(&self) -> FanoutView {
        match self {
            Self::Starting => FanoutView::Starting,
            Self::Live { limit, .. } => FanoutView::Relay { limit: *limit },
            Self::Off(reason) => FanoutView::Direct { reason: *reason },
        }
    }
}

/// A relay viewer of the own share as the viewer list shows it (FR-7.52).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayViewer {
    pub session: u32,
    /// Video frames per second it receives, from its last report; 0 before the first.
    pub fps: u32,
    /// "slow connection": it lost layers within the last 10 s.
    pub slow: bool,
}

/// The own share's fan-out for the sharer's bar and viewer list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanoutInfo {
    pub view: FanoutView,
    /// The viewers watching through the relay; direct viewers are in the media stats.
    pub relay_viewers: Vec<RelayViewer>,
}

/// How long a viewer report counts; viewers send one every few seconds.
const REPORT_VALID: std::time::Duration = std::time::Duration::from_secs(15);

impl Fanout {
    pub fn info(&self, now: Instant) -> FanoutInfo {
        let relay_viewers = match self {
            Self::Live {
                roster, reports, ..
            } => roster
                .relay_viewers()
                .into_iter()
                .map(|session| {
                    let report = reports
                        .get(&session)
                        .filter(|(_, at)| now.saturating_duration_since(*at) < REPORT_VALID)
                        .map(|(r, _)| *r)
                        .unwrap_or_default();
                    RelayViewer {
                        session,
                        fps: report.fps,
                        slow: report.slow,
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        FanoutInfo {
            view: self.view(),
            relay_viewers,
        }
    }
}

/// How long a viewer whose broadcast ended waits for the sharer to move it to a direct copy
/// (FR-7.49) before the watch ends.
const RELAY_SWITCH_WAIT: Duration = Duration::from_secs(10);

/// Why the relay link could not be used, as the watch and fan-out reasons name it.
fn relay_reason(e: &P2pError) -> RelayUnavailable {
    match e {
        P2pError::RelayRefused(RelayRefusal::UseNotAllowed) => {
            RelayUnavailable::UseNotAllowed(RelayUse::ScreenShare)
        }
        P2pError::RelayRefused(refusal) => RelayUnavailable::Refused(*refusal),
        P2pError::RelayCertMismatch => RelayUnavailable::Untrusted,
        _ => RelayUnavailable::Unreachable,
    }
}

pub(crate) fn cert_key(identity: &mumcord_store::Identity) -> Option<CertKey> {
    CertKey::new(identity.cert_der().to_vec(), identity.key_pkcs8_der())
        .inspect_err(|e| tracing::warn!(error = %e, "certificate not usable for a broadcast"))
        .ok()
}

/// The open publisher side, until it is put into the share.
struct Opened {
    id: BroadcastId,
    me: CertKey,
    link: mumcord_p2p::BroadcastLink,
    limit: u16,
}

impl Engine {
    /// Sets up the relay broadcast of the share with media `token` when fan-out is available
    /// (FR-7.40); otherwise the share keeps direct copies and the reason is kept for the
    /// one-time notice (FR-7.47).
    pub(super) fn start_fanout(&self, server_id: ServerId, token: usize) {
        let engine = self.clone();
        self.runtime().spawn(async move {
            let opened = engine.open_fanout(server_id, token).await;
            engine.fanout_opened(server_id, token, opened);
        });
    }

    /// The relay's broadcast limit when this side can use a broadcast on `server_id` now
    /// (FR-7.40): the relay allows screen sharing and offers broadcasts, the P2P mode uses the
    /// relay, and the Mumble server knows the user's certificate. Checked without waiting, as
    /// the start dialog and a Watch need it; the share's start checks the stored certificate.
    pub(crate) fn broadcast_limit(&self, server_id: ServerId) -> Option<u16> {
        self.relays()
            .availability_for(server_id, RelayUse::ScreenShare)
            .ok()?;
        if !self.relay_mode_allows_broadcast(server_id) {
            return None;
        }
        let ctx = self.session_ctx_for(server_id).ok()?;
        let own = ctx.handle.with_state(|s| s.session)?;
        super::cert_hash(&ctx.handle, own)?;
        self.relays()
            .broadcast_announcement(server_id)
            .ok()?
            .broadcast
            .filter(|max| *max > 0)
    }

    async fn open_fanout(
        &self,
        server_id: ServerId,
        token: usize,
    ) -> Result<Opened, FanoutUnavailable> {
        self.relays()
            .availability_for(server_id, RelayUse::ScreenShare)
            .map_err(FanoutUnavailable::Relay)?;
        if !self.relay_mode_allows_broadcast(server_id) {
            return Err(FanoutUnavailable::Mode);
        }
        let limit = self
            .relays()
            .broadcast_announcement(server_id)
            .map_err(FanoutUnavailable::Relay)?
            .broadcast
            .filter(|max| *max > 0)
            .ok_or(FanoutUnavailable::NotOffered)?;
        let (p2p, identity) = self
            .p2p_with_identity()
            .await
            .ok_or(FanoutUnavailable::NoCertificate)?;
        let me = cert_key(&identity).ok_or(FanoutUnavailable::NoCertificate)?;
        self.with_fanout(server_id, token, |fanout| *fanout = Some(Fanout::Starting));
        let (ann, id, relay_token) = self
            .relay_broadcast_request(server_id, RelayUse::ScreenShare, limit, false)
            .await?;
        let link = p2p
            .broadcast(&ann, &relay_token, BindRole::Publisher, false)
            .await
            .map_err(|e| {
                tracing::info!(error = %e, "screen share broadcast link not opened");
                FanoutUnavailable::Relay(relay_reason(&e))
            })?;
        Ok(Opened {
            id,
            me,
            link,
            limit,
        })
    }

    /// Runs `update` on the fan-out of the own share with media `token`; false when that share
    /// is gone.
    fn with_fanout(
        &self,
        server_id: ServerId,
        token: usize,
        update: impl FnOnce(&mut Option<Fanout>),
    ) -> bool {
        let mut all = lock(self.screen_share_state());
        match all
            .servers
            .get_mut(&server_id)
            .and_then(|s| s.own.as_mut())
            .filter(|own| own.media.as_ref().is_some_and(|m| m.token() == token))
        {
            Some(own) => {
                update(&mut own.fanout);
                true
            }
            None => false,
        }
    }

    fn fanout_opened(
        &self,
        server_id: ServerId,
        token: usize,
        opened: Result<Opened, FanoutUnavailable>,
    ) {
        let weak = self.downgrade();
        let on_end: OnEnd = Box::new(move |end| {
            if let Some(engine) = weak.upgrade() {
                engine.fanout_lost(server_id, token, end);
            }
        });
        let rt = self.runtime().clone();
        let mut all = lock(self.screen_share_state());
        let Some(own) = all
            .servers
            .get_mut(&server_id)
            .and_then(|s| s.own.as_mut())
            .filter(|own| own.media.as_ref().is_some_and(|m| m.token() == token))
        else {
            // The share ended meanwhile; dropping the link ends the broadcast.
            return;
        };
        let fanout = match opened {
            Err(reason) => {
                tracing::info!(?reason, "screen share without relay fan-out");
                Fanout::Off(reason)
            }
            Ok(opened) => {
                let Some(media) = own.media.as_ref() else {
                    return;
                };
                match Publisher::start(
                    &rt,
                    opened.id,
                    opened.me,
                    opened.link,
                    media.subscribe(BROADCAST_KEY),
                    on_end,
                ) {
                    Ok(publisher) => {
                        let mut roster = Roster::new(opened.limit);
                        let now = Instant::now();
                        // Viewers that got direct copies while the broadcast started.
                        for viewer in own.viewers.keys() {
                            roster.decide(*viewer, false, now);
                        }
                        tracing::info!(
                            limit = opened.limit,
                            "screen share sent once via the relay"
                        );
                        Fanout::Live {
                            publisher,
                            roster,
                            limit: opened.limit,
                            hashes: BTreeMap::new(),
                            reports: BTreeMap::new(),
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "screen share broadcast not started");
                        Fanout::Off(FanoutUnavailable::Lost)
                    }
                }
            }
        };
        own.fanout = Some(fanout);
        drop(all);
        if let Ok(ctx) = self.session_ctx_for(server_id) {
            emit(self.screen_share_state(), self.mumcord_state(), &ctx);
        }
    }

    /// The broadcast of the share with media `token` ended on its own (FR-7.49): the first 8
    /// viewers by joining continue on direct copies, the others are told the share is full,
    /// and the share stays on direct copies.
    fn fanout_lost(&self, server_id: ServerId, token: usize, end: BroadcastEnd) {
        let mut moved = Vec::new();
        let mut dropped = Vec::new();
        let lost = self.with_fanout(server_id, token, |fanout| {
            if let Some(Fanout::Live { roster, .. }) = fanout.as_mut() {
                (moved, dropped) = roster.relay_lost();
                *fanout = Some(Fanout::Off(FanoutUnavailable::Lost));
            }
        });
        if !lost || (moved.is_empty() && dropped.is_empty() && end == BroadcastEnd::Closed) {
            if lost {
                self.emit_shares(server_id);
            }
            return;
        }
        tracing::info!(
            ?end,
            moved = moved.len(),
            dropped = dropped.len(),
            "relay lost; viewers moved to direct copies"
        );
        if let Some(own) = lock(self.screen_share_state())
            .servers
            .get_mut(&server_id)
            .and_then(|s| s.own.as_mut())
        {
            for viewer in &moved {
                own.viewers.entry(*viewer).or_insert(None);
            }
            // What the dropped viewers can play no longer limits the share's codec.
            for viewer in &dropped {
                own.remove_viewer(*viewer);
            }
        }
        let Ok(ctx) = self.session_ctx_for(server_id) else {
            return;
        };
        send(&ctx.handle, &moved, &Signal::Accept);
        send(&ctx.handle, &dropped, &Signal::Full);
        if let Some(Here { others, .. }) = here(&ctx.handle) {
            send_viewers(
                self.screen_share_state(),
                self.mumcord_state(),
                &ctx,
                &others,
            );
        }
        emit(self.screen_share_state(), self.mumcord_state(), &ctx);
    }

    fn emit_shares(&self, server_id: ServerId) {
        if let Ok(ctx) = self.session_ctx_for(server_id) {
            emit(self.screen_share_state(), self.mumcord_state(), &ctx);
        }
    }

    /// Asks the bot to admit `viewer` to broadcast `id`; a refusal gives the viewer a direct
    /// copy while there is room, else "This share is full" (FR-7.42, FR-7.48).
    pub(super) fn admit_to_broadcast(&self, server_id: ServerId, id: BroadcastId, viewer: u32) {
        let engine = self.clone();
        self.runtime().spawn(async move {
            let Err(reason) = engine
                .relay_broadcast_admit(server_id, RelayUse::ScreenShare, id, viewer)
                .await
            else {
                return;
            };
            tracing::info!(?reason, "viewer not admitted to the broadcast");
            let decision = {
                let mut all = lock(engine.screen_share_state());
                let Some(own) = all.servers.get_mut(&server_id).and_then(|s| s.own.as_mut()) else {
                    return;
                };
                let Some(Fanout::Live {
                    publisher,
                    roster,
                    hashes,
                    ..
                }) = own.fanout.as_mut().filter(
                    |f| matches!(f, Fanout::Live { publisher, .. } if publisher.id() == id),
                )
                else {
                    return;
                };
                if let Some(hash) = hashes.remove(&viewer) {
                    publisher.remove(&hash);
                }
                let decision = roster.admit_failed(viewer, reason == BroadcastErrorReason::Full);
                if decision == Decision::Direct {
                    own.viewers.entry(viewer).or_insert(None);
                }
                decision
            };
            let Ok(ctx) = engine.session_ctx_for(server_id) else {
                return;
            };
            match decision {
                Decision::Direct => send(&ctx.handle, &[viewer], &Signal::Accept),
                Decision::Full => send(&ctx.handle, &[viewer], &Signal::Full),
                Decision::Relay | Decision::Wait => {}
            }
            if let Some(Here { others, .. }) = here(&ctx.handle) {
                send_viewers(
                    engine.screen_share_state(),
                    engine.mumcord_state(),
                    &ctx,
                    &others,
                );
            }
            emit(engine.screen_share_state(), engine.mumcord_state(), &ctx);
        });
    }

    /// The bot's `bcast-tok` for this client (from the pinned bot, checked by the caller):
    /// taken only while a watch request to `sharer` waits for its answer (section 6.6). False
    /// when it was not taken.
    pub(crate) fn screen_share_broadcast_token(
        &self,
        ctx: &SessionCtx,
        sharer: u32,
        id: BroadcastId,
        token: RelayToken,
    ) -> bool {
        if !here(&ctx.handle).is_some_and(|h| h.others.contains(&sharer)) {
            return false;
        }
        lock(self.screen_share_state())
            .servers
            .get_mut(&ctx.server_id)
            .is_some_and(|shares| shares.answered(sharer, Answer::Broadcast(id, token)))
    }

    /// Opens the subscriber end of `sharer`'s broadcast `id` for watch `attempt` (FR-7.42).
    pub(super) async fn watch_broadcast(
        &self,
        ctx: &SessionCtx,
        sharer: u32,
        attempt: u64,
        id: BroadcastId,
        token: RelayToken,
    ) -> EngineResult<()> {
        let server_id = ctx.server_id;
        let opened = self.open_subscriber(ctx, sharer, &token).await;
        let volume = self.person_share_volume(server_id, sharer);
        let result = {
            let mut all = lock(self.screen_share_state());
            let sink = all.sink.clone();
            let shares = all.servers.get_mut(&server_id);
            match opened {
                Ok((me, hash, link)) => {
                    if let Some(shares) = shares.filter(|s| s.is_opening(sharer, attempt)) {
                        shares.failed.remove(&sharer);
                        let weak = self.downgrade();
                        let on_end: OnEnd = Box::new(move |end| {
                            if let Some(engine) = weak.upgrade() {
                                engine.broadcast_watch_ended(server_id, sharer, id, end);
                            }
                        });
                        let (subscriber, frames, units) =
                            Subscriber::start(self.runtime(), id, me, hash, link, on_end);
                        if let Some(watching) = shares.watching.get_mut(&sharer) {
                            *watching = Watching::Open(super::watch::WatchLink::start_broadcast(
                                self.runtime(),
                                server_id,
                                sharer,
                                sink,
                                super::sound::ShareSound::new(self.voice_shared().clone(), volume),
                                frames,
                                units,
                                subscriber,
                            ));
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    if let Some(shares) = shares {
                        let current = shares.end_attempt(sharer, attempt);
                        if let (true, Some(block)) = (current, e.1) {
                            shares.failed.insert(sharer, block);
                        }
                    }
                    Err(e.0)
                }
            }
        };
        if result.is_err() {
            send(&ctx.handle, &[sharer], &Signal::Unwatch);
        }
        emit(self.screen_share_state(), self.mumcord_state(), ctx);
        result
    }

    async fn open_subscriber(
        &self,
        ctx: &SessionCtx,
        sharer: u32,
        token: &RelayToken,
    ) -> Result<(CertKey, String, mumcord_p2p::BroadcastLink), (EngineError, Option<WatchBlock>)>
    {
        let block = |b: WatchBlock| (super::blocked(b), Some(b));
        let ann = self
            .relays()
            .broadcast_announcement(ctx.server_id)
            .map_err(|r| block(WatchBlock::Relay(r)))?;
        let hash = super::cert_hash(&ctx.handle, sharer)
            .ok_or_else(|| block(WatchBlock::PeerUnsupported))?;
        let (p2p, identity) = self
            .p2p_with_identity()
            .await
            .ok_or_else(|| block(WatchBlock::PeerUnsupported))?;
        let me = cert_key(&identity).ok_or_else(|| block(WatchBlock::PeerUnsupported))?;
        let link = p2p
            .broadcast(&ann, token, BindRole::Subscriber, false)
            .await
            .map_err(|e| {
                tracing::info!(error = %e, "screen share broadcast not joined");
                match e {
                    P2pError::RelayRefused(RelayRefusal::BroadcastFull) => (
                        EngineError::new(ErrorKind::ShareFull, "this share is full"),
                        None,
                    ),
                    e => block(WatchBlock::Relay(relay_reason(&e))),
                }
            })?;
        Ok((me, hash, link))
    }

    /// The broadcast link of a watched share ended. While the share runs, the sharer moves
    /// viewers to direct copies (FR-7.49); without that within [`RELAY_SWITCH_WAIT`] the watch
    /// ends.
    fn broadcast_watch_ended(
        &self,
        server_id: ServerId,
        sharer: u32,
        id: BroadcastId,
        end: BroadcastEnd,
    ) {
        tracing::info!(?end, "watched broadcast ended");
        let engine = self.clone();
        self.runtime().spawn(async move {
            tokio::time::sleep(RELAY_SWITCH_WAIT).await;
            let ended = {
                let mut all = lock(engine.screen_share_state());
                let Some(shares) = all.servers.get_mut(&server_id) else {
                    return;
                };
                let same = matches!(
                    shares.watching.get(&sharer),
                    Some(Watching::Open(link)) if link.broadcast().is_some_and(|b| b.id() == id)
                );
                if same {
                    shares.watching.remove(&sharer);
                }
                same
            };
            if ended && let Ok(ctx) = engine.session_ctx_for(server_id) {
                send(&ctx.handle, &[sharer], &Signal::Unwatch);
                emit(engine.screen_share_state(), engine.mumcord_state(), &ctx);
            }
        });
    }

    /// Whether `sharer`'s share arrives through the relay broadcast ("via relay", FR-7.51);
    /// `None` while not watching it.
    pub fn screen_share_watch_via_relay(&self, server_id: ServerId, sharer: u32) -> Option<bool> {
        match lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.watching.get(&sharer))
        {
            Some(Watching::Open(link)) => Some(link.broadcast().is_some()),
            _ => None,
        }
    }

    /// How the own share reaches its viewers, with the relay viewers' reports (FR-7.36,
    /// FR-7.47, FR-7.52); `None` without a share or before it decided. Changes come with
    /// [`crate::EngineEvent::ScreenSharesChanged`].
    pub fn screen_share_fanout(&self, server_id: ServerId) -> Option<FanoutInfo> {
        lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.own.as_ref())
            .and_then(|own| own.fanout.as_ref())
            .map(|f| f.info(Instant::now()))
    }
}

/// Sends each sharer watched through the relay what arrives here, every few seconds
/// (FR-7.52; session tick).
pub(super) fn send_reports(state: &super::ServerShares, ctx: &SessionCtx) {
    let now = Instant::now();
    let reports: Vec<(u32, link::Received)> = lock(state)
        .servers
        .get(&ctx.server_id)
        .map(|s| {
            s.watching
                .iter()
                .filter_map(|(sharer, w)| match w {
                    Watching::Open(link) => link
                        .broadcast()
                        .and_then(|b| b.take_report(now))
                        .map(|r| (*sharer, r)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    for (sharer, report) in reports {
        send(
            &ctx.handle,
            &[sharer],
            &Signal::Report {
                fps: report.fps,
                slow: report.slow,
            },
        );
    }
}
