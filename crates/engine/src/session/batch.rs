//! Coalesces bursts of permission answers into one UI event (scale report, bottleneck #2).
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

use std::collections::BTreeSet;

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
}
