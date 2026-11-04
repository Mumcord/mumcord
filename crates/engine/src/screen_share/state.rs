//! Who shares and who watches on one server, kept for the user's own channel only.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_p2p::{BroadcastId, RelayToken};
use tokio::sync::oneshot;

use super::broadcast::Fanout;
use super::encoder::ActiveEncoder;
use super::media::{Media, ViewerLink};
use super::signal::{CodecName, MAX_VIEWERS};
use super::sound::{SoundStatus, SoundUnavailable};
use super::watch::WatchLink;
use super::{ScreenShareView, ShareKind, WatchBlock};

/// When a newcomer gets the own share a second time. The server silently drops plugin data over
/// its rate limit, and a join sets off a burst from everyone in the channel, so the first may be
/// lost.
pub(crate) const REANNOUNCE: Duration = Duration::from_secs(3);

/// The sharer's answer to a watch request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Answer {
    Accept,
    Full,
    /// Admitted to the sharer's relay broadcast: the bot's `bcast-tok` (spec 07, FR-7.42).
    Broadcast(BroadcastId, RelayToken),
}

/// What a viewer's `Watch` said it can play (FR-7.58).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ViewerCaps {
    pub av1: bool,
    /// The frame version the viewer reads ([`super::wire`]).
    pub wire: u8,
}

impl ViewerCaps {
    /// 0.6.x lists no codecs: VP9 only, frame version 1.
    pub fn from_watch(codecs: &[CodecName]) -> Self {
        if codecs.is_empty() {
            Self {
                av1: false,
                wire: super::wire::V1,
            }
        } else {
            Self {
                av1: codecs.contains(&CodecName::Av1),
                wire: super::wire::V2,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admit {
    New,
    Already,
    Full,
}

/// The user's own share.
pub(crate) struct OwnShare {
    pub paused: bool,
    pub sound: bool,
    /// What the capture gave (FR-7.76, FR-7.84); without a source `sound` stays off.
    pub sound_status: SoundStatus,
    /// Accepted viewers, with their stream once they opened it.
    pub viewers: BTreeMap<u32, Option<ViewerLink>>,
    /// What each viewer can play, direct and relay ones alike: the share's single encoder has
    /// to suit all of them. Changed through [`Self::admit`], [`Self::set_caps`] and
    /// [`Self::remove_viewer`].
    caps: BTreeMap<u32, ViewerCaps>,
    /// What the user picked in the system picker (issue #163).
    pub kind: Option<ShareKind>,
    /// When the share started, for the "since" line (issue #163).
    pub started_at: Instant,
    /// Capture and encoding; `None` only in tests of the bookkeeping.
    pub media: Option<Media>,
    /// The encoder the share's media opened (codec badge, FR-7.39).
    pub encoder: Arc<Mutex<Option<ActiveEncoder>>>,
    /// Newcomers to the channel that get the share once more, and when.
    reannounce: BTreeMap<u32, Instant>,
    /// The relay broadcast (spec 07, FR-7.40); `None` until the share decided about one.
    pub fanout: Option<Fanout>,
}

impl OwnShare {
    pub fn new(sound: bool, sound_status: SoundStatus) -> Self {
        Self {
            paused: false,
            sound: sound && sound_status.available(),
            sound_status,
            viewers: BTreeMap::new(),
            caps: BTreeMap::new(),
            kind: None,
            started_at: Instant::now(),
            media: None,
            encoder: Arc::default(),
            reannounce: BTreeMap::new(),
            fanout: None,
        }
    }

    /// The sound capture broke during the share (FR-7.84): no sound from now on, with the
    /// reason for the live bar; the stored `screen_sound` stays as it is.
    pub fn sound_failed(&mut self) {
        self.sound = false;
        self.sound_status = SoundStatus {
            unavailable: Some(SoundUnavailable::CaptureFailed),
            ..SoundStatus::default()
        };
    }

    /// `newcomer` was just told about the share; they hear of it again after [`REANNOUNCE`].
    pub fn announced_to(&mut self, newcomer: u32, now: Instant) {
        self.reannounce.insert(newcomer, now + REANNOUNCE);
    }

    /// The newcomers due for their second announcement at `now`; each comes up once.
    pub fn take_reannounce(&mut self, now: Instant) -> Vec<u32> {
        let due: Vec<u32> = self
            .reannounce
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(newcomer, _)| *newcomer)
            .collect();
        for newcomer in &due {
            self.reannounce.remove(newcomer);
        }
        due
    }

    /// Takes `viewer` in unless [`MAX_VIEWERS`] others already watch. Asking again updates
    /// what the viewer can play.
    pub fn admit(&mut self, viewer: u32, caps: ViewerCaps) -> Admit {
        let admit = if self.viewers.contains_key(&viewer) {
            Admit::Already
        } else if self.viewers.len() >= MAX_VIEWERS {
            return Admit::Full;
        } else {
            self.viewers.insert(viewer, None);
            Admit::New
        };
        self.caps.insert(viewer, caps);
        self.codecs_changed();
        admit
    }

    /// Ends `viewer`'s watching, closing its direct stream and forgetting what it can play;
    /// false if it had no direct copy.
    pub fn remove_viewer(&mut self, viewer: u32) -> bool {
        let had_caps = self.caps.remove(&viewer).is_some();
        let removed = self.viewers.remove(&viewer).is_some();
        if removed || had_caps {
            self.codecs_changed();
        }
        removed
    }

    /// Records what `viewer` can play when it was taken in elsewhere (the relay broadcast, or
    /// a direct copy the fan-out decided on).
    pub fn set_caps(&mut self, viewer: u32, caps: ViewerCaps) {
        if self.caps.insert(viewer, caps) != Some(caps) {
            self.codecs_changed();
        }
    }

    /// What `viewer` can play; an unknown viewer counts as 0.6.x.
    pub fn viewer_caps(&self, viewer: u32) -> ViewerCaps {
        self.caps
            .get(&viewer)
            .copied()
            .unwrap_or_else(|| ViewerCaps::from_watch(&[]))
    }

    /// The first viewer that cannot play AV1; the share runs VP9 while there is one (FR-7.58).
    pub fn av1_blocker(&self) -> Option<u32> {
        self.caps
            .iter()
            .find(|(_, caps)| !caps.av1)
            .map(|(viewer, _)| *viewer)
    }

    fn codecs_changed(&self) {
        if let Some(media) = &self.media {
            media.set_av1_blocker(self.av1_blocker());
        }
    }

    /// The encoder the share's media opened; `None` until it opened one.
    pub fn active_encoder(&self) -> Option<ActiveEncoder> {
        *crate::session::lock(&self.encoder)
    }

    pub fn viewer_list(&self) -> Vec<u32> {
        let mut list: Vec<u32> = self.viewers.keys().copied().collect();
        if let Some(fanout) = &self.fanout {
            list.extend(fanout.relay_viewers());
            list.sort_unstable();
            list.dedup();
        }
        list
    }

    /// How long the share has been running (issue #163).
    pub fn elapsed_ms(&self) -> u32 {
        Instant::now()
            .saturating_duration_since(self.started_at)
            .as_millis()
            .try_into()
            .unwrap_or(u32::MAX)
    }
}

/// What a sharer says about its own share (issue #163: also what it picked and since when).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Announced {
    pub paused: bool,
    pub sound: bool,
    pub kind: Option<ShareKind>,
    pub elapsed_ms: u32,
}

/// Someone else's share as they announced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OtherShare {
    pub paused: bool,
    pub sound: bool,
    pub viewers: Vec<u32>,
    pub kind: Option<ShareKind>,
    /// What they said about their own elapsed time, and when that arrived: the UI adds the time
    /// since, so no clock of another machine is trusted.
    pub elapsed_ms: u32,
    pub seen_at: Instant,
}

/// The user watching someone's share.
pub(crate) enum Watching {
    /// Asked; waiting for the sharer's answer. `attempt` tells a replaced attempt apart.
    Asked {
        attempt: u64,
        reply: oneshot::Sender<Answer>,
    },
    /// Accepted; attempt `.0` is opening the stream.
    Opening(u64),
    Open(WatchLink),
}

/// What [`Shares::forget`] changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Forgot {
    /// Something the UI shows changed.
    pub changed: bool,
    /// The user's own viewer list changed and has to be sent again.
    pub own_viewers: bool,
}

#[derive(Default)]
pub(crate) struct Shares {
    pub own: Option<OwnShare>,
    pub others: BTreeMap<u32, OtherShare>,
    pub watching: BTreeMap<u32, Watching>,
    /// Why watching a sharer failed last time; kept until their share ends.
    pub failed: BTreeMap<u32, WatchBlock>,
    next_attempt: u64,
}

impl Shares {
    pub fn is_empty(&self) -> bool {
        self.own.is_none() && self.others.is_empty() && self.watching.is_empty()
    }

    /// `sharer` announced a live share; true when it is new or changed.
    pub fn share_live(&mut self, sharer: u32, announced: &Announced, now: Instant) -> bool {
        match self.others.get_mut(&sharer) {
            Some(share) if share.paused == announced.paused && share.sound == announced.sound => {
                false
            }
            Some(share) => {
                share.paused = announced.paused;
                share.sound = announced.sound;
                share.kind = announced.kind;
                share.elapsed_ms = announced.elapsed_ms;
                share.seen_at = now;
                true
            }
            None => {
                self.others.insert(
                    sharer,
                    OtherShare {
                        paused: announced.paused,
                        sound: announced.sound,
                        viewers: Vec::new(),
                        kind: announced.kind,
                        elapsed_ms: announced.elapsed_ms,
                        seen_at: now,
                    },
                );
                true
            }
        }
    }

    /// `sharer`'s share ended; watching it ends too. True when there was one.
    pub fn share_ended(&mut self, sharer: u32) -> bool {
        self.watching.remove(&sharer);
        self.failed.remove(&sharer);
        self.others.remove(&sharer).is_some()
    }

    /// `sharer`'s viewers, already checked against the people in the channel.
    pub fn set_viewers(&mut self, sharer: u32, viewers: Vec<u32>) -> bool {
        match self.others.get_mut(&sharer) {
            Some(share) if share.viewers != viewers => {
                share.viewers = viewers;
                true
            }
            _ => false,
        }
    }

    /// Starts an attempt to watch `sharer`; `None` while the stream is already open. A pending
    /// attempt is replaced: its viewer gave up on it (closed or timed out) and watches again,
    /// and that new viewer must get its own stream rather than wait on the old attempt.
    pub fn begin_watch(&mut self, sharer: u32) -> Option<(u64, oneshot::Receiver<Answer>)> {
        if matches!(self.watching.get(&sharer), Some(Watching::Open(_))) {
            return None;
        }
        self.next_attempt += 1;
        let attempt = self.next_attempt;
        let (reply, answer) = oneshot::channel();
        self.watching
            .insert(sharer, Watching::Asked { attempt, reply });
        Some((attempt, answer))
    }

    /// Whether `attempt` was accepted and is still the one opening `sharer`'s stream.
    pub fn is_opening(&self, sharer: u32, attempt: u64) -> bool {
        matches!(self.watching.get(&sharer), Some(Watching::Opening(a)) if *a == attempt)
    }

    /// Drops `attempt` on `sharer` unless a newer attempt replaced it; true when it was current.
    pub fn end_attempt(&mut self, sharer: u32, attempt: u64) -> bool {
        let current = match self.watching.get(&sharer) {
            Some(Watching::Asked { attempt: a, .. } | Watching::Opening(a)) => *a == attempt,
            _ => false,
        };
        if current {
            self.watching.remove(&sharer);
        }
        current
    }

    /// Hands `sharer`'s answer to the waiting watch request; ignored when nobody asked.
    pub fn answered(&mut self, sharer: u32, answer: Answer) {
        let Some(&Watching::Asked { attempt, .. }) = self.watching.get(&sharer) else {
            return;
        };
        let next = match answer {
            Answer::Accept | Answer::Broadcast(..) => {
                self.watching.insert(sharer, Watching::Opening(attempt))
            }
            Answer::Full => self.watching.remove(&sharer),
        };
        if let Some(Watching::Asked { reply, .. }) = next {
            // The asking call may have timed out already.
            let _ = reply.send(answer);
        }
    }

    /// `session` is no longer in the user's channel: their share, their watching and the user
    /// watching them end.
    pub fn forget(&mut self, session: u32) -> Forgot {
        if let Some(own) = self.own.as_mut() {
            own.reannounce.remove(&session);
        }
        let own_viewers = self
            .own
            .as_mut()
            .is_some_and(|own| own.remove_viewer(session));
        let mut changed = self.share_ended(session) || own_viewers;
        for share in self.others.values_mut() {
            let before = share.viewers.len();
            share.viewers.retain(|v| *v != session);
            changed |= share.viewers.len() != before;
        }
        Forgot {
            changed,
            own_viewers,
        }
    }

    /// Every share in the channel, the user's own first. `blocked` only carries the remembered
    /// failures; the caller adds what it knows up front.
    pub fn views(&self, own: u32) -> Vec<ScreenShareView> {
        let now = Instant::now();
        let own_view = self.own.as_ref().map(|share| ScreenShareView {
            session: own,
            paused: share.paused,
            sound: share.sound,
            sound_status: share.sound_status.clone(),
            viewers: share.viewer_list(),
            blocked: None,
            kind: share.kind,
            elapsed_ms: elapsed(share.started_at, now),
        });
        let others = self.others.iter().map(|(session, share)| ScreenShareView {
            session: *session,
            paused: share.paused,
            sound: share.sound,
            sound_status: SoundStatus::default(),
            viewers: share.viewers.clone(),
            blocked: self.failed.get(session).copied(),
            kind: share.kind,
            // Their own elapsed time at the moment they signalled, plus the time since.
            elapsed_ms: share.elapsed_ms + elapsed(share.seen_at, now),
        });
        own_view.into_iter().chain(others).collect()
    }
}

/// Milliseconds from `then` to `now`.
fn elapsed(then: Instant, now: Instant) -> u32 {
    now.saturating_duration_since(then)
        .as_millis()
        .try_into()
        .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen_share::sound::{AppName, ShareSoundMode};

    const NEW: ViewerCaps = ViewerCaps {
        av1: true,
        wire: super::super::wire::V2,
    };

    #[test]
    fn the_active_encoder_is_whatever_the_media_opened() {
        let own = OwnShare::new(false, SoundStatus::default());
        assert_eq!(own.active_encoder(), None);
        let opened = ActiveEncoder {
            codec: mumcord_screenshare::Codec::Av1,
            gpu: None,
        };
        *crate::session::lock(&own.encoder) = Some(opened);
        assert_eq!(own.active_encoder(), Some(opened));
    }

    #[test]
    fn an_old_viewer_blocks_av1_until_it_leaves() {
        let old = ViewerCaps::from_watch(&[]);
        assert_eq!((old.av1, old.wire), (false, super::super::wire::V1));
        let vp9_only = ViewerCaps::from_watch(&[CodecName::Vp9]);
        assert_eq!(
            (vp9_only.av1, vp9_only.wire),
            (false, super::super::wire::V2)
        );
        assert_eq!(
            ViewerCaps::from_watch(&[CodecName::Av1, CodecName::Vp9]),
            NEW
        );

        let mut own = OwnShare::new(false, SoundStatus::default());
        own.admit(1, NEW);
        assert_eq!(own.av1_blocker(), None);
        own.admit(4, old);
        assert_eq!(own.av1_blocker(), Some(4));
        assert_eq!((own.viewer_caps(1), own.viewer_caps(4)), (NEW, old));
        assert!(own.remove_viewer(4));
        assert_eq!(own.av1_blocker(), None);
        assert_eq!(own.viewer_caps(4), old, "unknown viewers count as 0.6.x");
        own.admit(1, vp9_only);
        assert_eq!(
            own.av1_blocker(),
            Some(1),
            "asking again updates the codecs"
        );
        assert_eq!(own.admit(9, old), Admit::New);
        let mut full = OwnShare::new(false, SoundStatus::default());
        for viewer in 0..MAX_VIEWERS as u32 {
            full.admit(viewer, NEW);
        }
        assert_eq!(full.admit(99, old), Admit::Full);
        assert_eq!(
            full.av1_blocker(),
            None,
            "a turned-away viewer does not count"
        );
    }

    #[test]
    fn relay_viewers_count_for_the_codec_without_a_direct_copy() {
        let vp9_only = ViewerCaps::from_watch(&[CodecName::Vp9]);
        let mut own = OwnShare::new(false, SoundStatus::default());
        own.admit(1, ViewerCaps::from_watch(&[CodecName::Av1, CodecName::Vp9]));
        own.set_caps(30, vp9_only);
        assert_eq!(own.av1_blocker(), Some(30));
        assert_eq!(
            own.viewer_list(),
            [1],
            "no direct copy for the relay viewer"
        );
        assert!(!own.remove_viewer(30), "it had no direct copy");
        assert_eq!(own.av1_blocker(), None, "its codecs are forgotten");
    }

    #[test]
    fn sound_follows_the_source_that_take_sound_gave() {
        let unavailable = SoundStatus {
            unavailable: Some(SoundUnavailable::OsTooOld),
            ..SoundStatus::default()
        };
        let mut shares = Shares {
            own: Some(OwnShare::new(true, unavailable.clone())),
            ..Shares::default()
        };
        let view = &shares.views(1)[0];
        assert_eq!(
            view.sound_status, unavailable,
            "the reason reaches the view"
        );
        assert!(!view.sound, "asking for sound without a source stays off");

        let app = SoundStatus {
            mode: ShareSoundMode::App,
            app: Some(AppName::new("Player")),
            unavailable: None,
        };
        shares.own = Some(OwnShare::new(true, app.clone()));
        let view = &shares.views(1)[0];
        assert!(view.sound);
        assert_eq!(view.sound_status, app);

        shares.own = Some(OwnShare::new(false, app));
        assert!(!shares.views(1)[0].sound, "switched off");
    }

    #[test]
    fn a_broken_sound_capture_turns_the_sound_off_with_its_reason() {
        let mut own = OwnShare::new(
            true,
            SoundStatus {
                mode: ShareSoundMode::App,
                app: Some(AppName::new("Player")),
                unavailable: None,
            },
        );
        own.sound_failed();
        assert!(!own.sound);
        assert_eq!(own.sound_status.mode, ShareSoundMode::None);
        assert_eq!(
            own.sound_status.app, None,
            "the app name is gone with the sound"
        );
        assert_eq!(
            own.sound_status.unavailable,
            Some(SoundUnavailable::CaptureFailed)
        );
        assert!(
            !own.sound_status.available(),
            "the switch cannot turn it back on"
        );
    }

    #[test]
    fn the_ninth_viewer_is_turned_away() {
        let mut own = OwnShare::new(false, SoundStatus::default());
        for viewer in 0..MAX_VIEWERS as u32 {
            assert_eq!(own.admit(viewer, NEW), Admit::New);
        }
        assert_eq!(own.admit(3, NEW), Admit::Already, "asking again is fine");
        assert_eq!(own.admit(100, NEW), Admit::Full);
        assert!(own.remove_viewer(0));
        assert_eq!(
            own.admit(100, NEW),
            Admit::New,
            "a free place is taken again"
        );
    }

    fn announced(paused: bool, sound: bool) -> Announced {
        Announced {
            paused,
            sound,
            kind: Some(ShareKind::Window),
            elapsed_ms: 1000,
        }
    }

    #[test]
    fn share_updates_report_changes_only() {
        let mut shares = Shares::default();
        let now = Instant::now();
        assert!(shares.share_live(5, &announced(false, true), now));
        assert!(!shares.share_live(5, &announced(false, true), now));
        assert!(shares.share_live(5, &announced(true, true), now));
        assert!(shares.set_viewers(5, vec![1, 2]));
        assert!(!shares.set_viewers(5, vec![1, 2]));
        assert!(!shares.set_viewers(6, vec![1]), "no share, no viewers");
        assert!(shares.share_ended(5));
        assert!(!shares.share_ended(5));
        assert!(shares.is_empty());
    }

    #[test]
    fn answers_reach_only_a_waiting_request() {
        let mut shares = Shares::default();
        shares.answered(5, Answer::Accept);
        assert!(shares.watching.is_empty(), "an unasked accept is ignored");

        let (attempt, mut rx) = shares.begin_watch(5).expect("not watching yet");
        shares.answered(5, Answer::Accept);
        assert_eq!(rx.try_recv(), Ok(Answer::Accept));
        assert!(shares.is_opening(5, attempt));
        shares.answered(5, Answer::Full);
        assert!(
            shares.is_opening(5, attempt),
            "a late full does not undo the accept"
        );

        let (_, mut rx) = shares.begin_watch(6).expect("not watching yet");
        shares.answered(6, Answer::Full);
        assert_eq!(rx.try_recv(), Ok(Answer::Full));
        assert!(!shares.watching.contains_key(&6));
    }

    #[test]
    fn watching_again_replaces_a_pending_attempt() {
        let mut shares = Shares::default();
        let (first, mut first_rx) = shares.begin_watch(5).expect("not watching yet");
        let (second, _second_rx) = shares
            .begin_watch(5)
            .expect("a pending attempt is no open stream");
        assert_ne!(first, second);
        assert!(
            first_rx.try_recv().is_err(),
            "the replaced request stops waiting"
        );
        shares.answered(5, Answer::Accept);
        assert!(!shares.is_opening(5, first) && shares.is_opening(5, second));

        assert!(
            !shares.end_attempt(5, first),
            "the replaced attempt failing late leaves the new one alone"
        );
        assert!(shares.is_opening(5, second));
        assert!(shares.end_attempt(5, second));
        assert!(shares.watching.is_empty());
    }

    #[test]
    fn leaving_the_channel_ends_sharing_and_watching() {
        let mut shares = Shares::default();
        let mut own = OwnShare::new(true, SoundStatus::test_source());
        own.admit(7, NEW);
        shares.own = Some(own);
        shares.share_live(5, &announced(false, false), Instant::now());
        shares.set_viewers(5, vec![1, 7]);
        shares.watching.insert(5, Watching::Opening(1));
        shares.failed.insert(5, WatchBlock::NoDirectPath);

        assert_eq!(
            shares.forget(7),
            Forgot {
                changed: true,
                own_viewers: true
            }
        );
        assert_eq!(shares.others[&5].viewers, [1]);
        assert_eq!(
            shares.forget(5),
            Forgot {
                changed: true,
                own_viewers: false
            }
        );
        assert!(shares.watching.is_empty() && shares.failed.is_empty());
        assert_eq!(shares.forget(9), Forgot::default());
    }

    #[test]
    fn newcomers_hear_of_the_own_share_once_more() {
        let now = Instant::now();
        let mut shares = Shares {
            own: Some(OwnShare::new(false, SoundStatus::default())),
            ..Shares::default()
        };
        let own = shares.own.as_mut().expect("own share");
        own.announced_to(9, now);
        own.announced_to(8, now + Duration::from_secs(1));
        own.announced_to(7, now);
        assert!(own.take_reannounce(now).is_empty(), "not yet");
        assert_eq!(own.take_reannounce(now + REANNOUNCE), [7, 9]);
        assert!(
            own.take_reannounce(now + REANNOUNCE).is_empty(),
            "only once"
        );

        shares.forget(8);
        let own = shares.own.as_mut().expect("own share");
        assert!(
            own.take_reannounce(now + REANNOUNCE * 2).is_empty(),
            "not after leaving the channel"
        );
    }

    #[test]
    fn views_put_the_own_share_first() {
        let mut shares = Shares::default();
        shares.share_live(2, &announced(true, false), Instant::now());
        shares.failed.insert(2, WatchBlock::NoDirectPath);
        shares.own = Some(OwnShare::new(true, SoundStatus::test_source()));
        let views = shares.views(9);
        assert_eq!(views.len(), 2);
        assert_eq!((views[0].session, views[0].sound), (9, true));
        assert_eq!(views[1].session, 2);
        assert!(views[1].paused);
        assert_eq!(views[1].blocked, Some(WatchBlock::NoDirectPath));
        assert_eq!(
            views[1].kind,
            Some(ShareKind::Window),
            "what they picked travels with the announcement (issue #163)"
        );
    }
}
