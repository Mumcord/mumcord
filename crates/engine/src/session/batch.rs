//! Calms bursts of engine events before they reach the UI.
//!
//! Permission answers (scale report, bottleneck #2) are coalesced into one channel event; talk
//! states are rate-limited per person (FR-2.23, scale report P13).
//!
//! ## Permission answers
//!
//! After every sync the engine asks the server for its effective permissions in each channel,
//! and the answers trickle in one by one (305 on a big server within about 100 ms). Sent on as
//! single channel events, each one cost the UI a full re-render and froze it for seconds. The
//! session loop instead collects the affected channel ids here and emits one
//! `ChannelsUpserted` once the burst has settled.
//!
//! The channel views are built from the protocol state when the batch is flushed, never when an
//! answer arrives, so a flushed batch always carries the latest data and cannot bring back a
//! channel that was removed in the meantime. That is what keeps ordering safe relative to the
//! single `ChannelUpserted` / `ChannelRemoved` events emitted in between.

use std::collections::{BTreeMap, BTreeSet};

use tokio::time::{Duration, Instant};

/// Quiet time after the last answer before the batch is flushed.
pub const SETTLE: Duration = Duration::from_millis(30);
/// Upper bound for holding back an answer while a burst keeps going.
pub const MAX_DELAY: Duration = Duration::from_millis(200);

/// Which channels a flushed batch covers.
#[derive(Debug, PartialEq, Eq)]
pub enum Channels {
    All,
    Some(BTreeSet<u32>),
}

impl Channels {
    pub fn contains(&self, id: u32) -> bool {
        match self {
            Channels::All => true,
            Channels::Some(ids) => ids.contains(&id),
        }
    }
}

#[derive(Default)]
pub struct PermissionBatch {
    pending: Option<Pending>,
}

struct Pending {
    channels: Channels,
    first: Instant,
    last: Instant,
}

impl PermissionBatch {
    /// Records an answer for one channel, or for all channels (`None`).
    pub fn add(&mut self, channel_id: Option<u32>, now: Instant) {
        let pending = self.pending.get_or_insert_with(|| Pending {
            channels: Channels::Some(BTreeSet::new()),
            first: now,
            last: now,
        });
        pending.last = now;
        match (channel_id, &mut pending.channels) {
            (_, Channels::All) => {}
            (None, channels) => *channels = Channels::All,
            (Some(id), Channels::Some(ids)) => {
                ids.insert(id);
            }
        }
    }

    /// When the pending batch is due, if there is one.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending
            .as_ref()
            .map(|p| (p.last + SETTLE).min(p.first + MAX_DELAY))
    }

    /// Takes the pending batch.
    pub fn take(&mut self) -> Option<Channels> {
        self.pending.take().map(|p| p.channels)
    }
}

/// Least time between two shown talk-state changes of one person: about 10 per second.
pub const TALK_MIN_GAP: Duration = Duration::from_millis(100);
/// How long a stop is held back: a pause shorter than this keeps the talking ring on, so a
/// speaker's ring does not flicker between words.
pub const TALK_STOP_HOLD: Duration = Duration::from_millis(150);

/// Talk states as the UI should see them. Starting to talk is shown at once (AC-1.7), unless the
/// person's last change is less than [`TALK_MIN_GAP`] ago; stopping waits [`TALK_STOP_HOLD`].
#[derive(Default)]
pub struct TalkBatch {
    people: BTreeMap<u32, Talker>,
}

struct Talker {
    shown: bool,
    wanted: bool,
    /// When `wanted` last changed.
    since: Instant,
    /// When `shown` last changed.
    shown_at: Option<Instant>,
}

impl TalkBatch {
    /// Records the latest talk state of `session`.
    pub fn set(&mut self, session: u32, talking: bool, now: Instant) {
        let talker = self.people.entry(session).or_insert(Talker {
            shown: false,
            wanted: false,
            since: now,
            shown_at: None,
        });
        if talker.wanted != talking {
            talker.wanted = talking;
            talker.since = now;
        }
    }

    /// The changes due at `now`, in session order; each one is then taken as shown.
    pub fn take_due(&mut self, now: Instant) -> Vec<(u32, bool)> {
        let mut due = Vec::new();
        for (&session, talker) in &mut self.people {
            if talker.wanted == talker.shown {
                continue;
            }
            let hold = if talker.wanted {
                talker.since
            } else {
                talker.since + TALK_STOP_HOLD
            };
            let at = talker
                .shown_at
                .map_or(hold, |shown_at| hold.max(shown_at + TALK_MIN_GAP));
            if now >= at {
                talker.shown = talker.wanted;
                talker.shown_at = Some(now);
                due.push((session, talker.shown));
            }
        }
        // Quiet people are forgotten once their last change no longer limits the next one.
        self.people.retain(|_, t| {
            t.shown || t.wanted || t.shown_at.is_some_and(|at| now < at + TALK_MIN_GAP)
        });
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_batch_has_no_deadline() {
        let mut batch = PermissionBatch::default();
        assert_eq!(batch.deadline(), None);
        assert_eq!(batch.take(), None);
    }

    #[test]
    fn burst_collects_channels_and_waits_for_quiet() {
        let start = Instant::now();
        let mut batch = PermissionBatch::default();
        batch.add(Some(3), start);
        batch.add(Some(1), start + Duration::from_millis(10));
        batch.add(Some(3), start + Duration::from_millis(20));
        assert_eq!(
            batch.deadline(),
            Some(start + Duration::from_millis(20) + SETTLE)
        );
        assert_eq!(batch.take(), Some(Channels::Some(BTreeSet::from([1, 3]))));
        assert_eq!(batch.deadline(), None);
    }

    #[test]
    fn long_burst_is_flushed_after_max_delay() {
        let start = Instant::now();
        let mut batch = PermissionBatch::default();
        for ms in (0..400).step_by(10) {
            batch.add(Some(ms), start + Duration::from_millis(ms.into()));
        }
        assert_eq!(batch.deadline(), Some(start + MAX_DELAY));
    }

    #[test]
    fn answer_for_all_channels_covers_everything() {
        let now = Instant::now();
        let mut batch = PermissionBatch::default();
        batch.add(Some(1), now);
        batch.add(None, now);
        batch.add(Some(2), now);
        let channels = batch.take().expect("pending");
        assert_eq!(channels, Channels::All);
        assert!(channels.contains(42));
    }

    fn ms(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn start_shows_at_once_and_stop_after_the_hold() {
        let start = Instant::now();
        let mut talk = TalkBatch::default();
        talk.set(7, true, start);
        assert_eq!(talk.take_due(start), vec![(7, true)]);
        talk.set(7, false, ms(start, 500));
        assert_eq!(talk.take_due(ms(start, 600)), vec![]);
        assert_eq!(talk.take_due(ms(start, 650)), vec![(7, false)]);
        assert_eq!(talk.take_due(ms(start, 700)), vec![]);
    }

    #[test]
    fn short_pause_keeps_the_ring_on() {
        let start = Instant::now();
        let mut talk = TalkBatch::default();
        talk.set(1, true, start);
        assert_eq!(talk.take_due(start), vec![(1, true)]);
        talk.set(1, false, ms(start, 300));
        assert_eq!(talk.take_due(ms(start, 340)), vec![]);
        talk.set(1, true, ms(start, 380));
        assert_eq!(talk.take_due(ms(start, 1000)), vec![]);
    }

    #[test]
    fn changes_of_one_person_are_rate_limited() {
        let start = Instant::now();
        let mut talk = TalkBatch::default();
        talk.set(2, true, start);
        assert_eq!(talk.take_due(start), vec![(2, true)]);
        talk.set(2, false, start);
        assert_eq!(talk.take_due(ms(start, 150)), vec![(2, false)]);
        // Starting again right after the stop was shown waits for the minimum gap.
        talk.set(2, true, ms(start, 160));
        assert_eq!(talk.take_due(ms(start, 200)), vec![]);
        assert_eq!(talk.take_due(ms(start, 250)), vec![(2, true)]);
    }

    #[test]
    fn people_are_independent_and_reported_in_session_order() {
        let start = Instant::now();
        let mut talk = TalkBatch::default();
        talk.set(9, true, start);
        talk.set(3, true, start);
        talk.set(5, false, start);
        assert_eq!(talk.take_due(start), vec![(3, true), (9, true)]);
    }

    #[test]
    fn quiet_people_are_forgotten() {
        let start = Instant::now();
        let mut talk = TalkBatch::default();
        talk.set(4, true, start);
        talk.take_due(start);
        talk.set(4, false, start);
        talk.take_due(ms(start, 150));
        talk.take_due(ms(start, 300));
        assert!(talk.people.is_empty());
    }
}
