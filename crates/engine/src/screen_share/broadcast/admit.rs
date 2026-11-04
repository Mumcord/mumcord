//! Who watches the own share how, while it has a relay broadcast (spec 07, FR-7.42, FR-7.47 –
//! FR-7.49). Pure bookkeeping, no I/O: the caller sends `bcast-admit`, `Accept` or `Full` as
//! the [`Decision`] says.
//!
//! - A viewer whose Mumcord announced `screen-broadcast` goes through the relay; any other gets
//!   a direct copy as before, at most [`MAX_VIEWERS`] of those.
//! - All viewers together stay within the broadcast's limit (the relay's `bc`).
//! - When the relay is lost, the first [`MAX_VIEWERS`] viewers in order of joining continue on
//!   direct copies and the others are told the share is full.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::screen_share::signal::MAX_VIEWERS;

/// How long an admitted relay viewer has to bind before a repeated `Watch` admits it again
/// (its token or the bot's answer may have been lost). Admitting again gives a new token and
/// revokes the old one, so a viewer still busy binding must not be admitted twice.
pub(crate) const READMIT_AFTER: Duration = Duration::from_secs(4);

/// How a viewer gets the share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Path {
    /// Through the relay broadcast.
    Relay,
    /// A copy of its own over the P2P connection.
    Direct,
}

/// What to do about a `Watch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Ask the bot to admit the viewer to the broadcast (`bcast-admit`); no `Accept`.
    Relay,
    /// Send `Accept`; the viewer opens a direct stream.
    Direct,
    /// Send nothing: the viewer was just admitted and its token is on the way.
    Wait,
    /// Send `Full`.
    Full,
}

#[derive(Debug, Clone, Copy)]
struct Seat {
    path: Path,
    /// Order of joining (FR-7.49).
    joined: u64,
    admitted_at: Instant,
}

/// The viewers of the own share and how each gets it.
#[derive(Debug)]
pub(crate) struct Roster {
    seats: BTreeMap<u32, Seat>,
    next: u64,
    /// Viewers in all (the broadcast's `max`).
    limit: usize,
    /// The broadcast is up; without it everyone gets a direct copy.
    relay: bool,
}

impl Roster {
    pub fn new(limit: u16) -> Self {
        Self {
            seats: BTreeMap::new(),
            next: 0,
            limit: usize::from(limit).max(1),
            relay: true,
        }
    }

    /// Viewers in all while the broadcast is up, else [`MAX_VIEWERS`].
    pub fn limit(&self) -> usize {
        if self.relay { self.limit } else { MAX_VIEWERS }
    }

    /// Decides about a `Watch` from `viewer`; `broadcast` says whether its Mumcord can join
    /// broadcasts. Records the seat.
    pub fn decide(&mut self, viewer: u32, broadcast: bool, now: Instant) -> Decision {
        if let Some(seat) = self.seats.get_mut(&viewer) {
            return match seat.path {
                Path::Direct => Decision::Direct,
                Path::Relay if now.saturating_duration_since(seat.admitted_at) >= READMIT_AFTER => {
                    seat.admitted_at = now;
                    Decision::Relay
                }
                Path::Relay => Decision::Wait,
            };
        }
        if self.seats.len() >= self.limit() {
            return Decision::Full;
        }
        let path = if self.relay && broadcast {
            Path::Relay
        } else if self.direct_count() < MAX_VIEWERS {
            Path::Direct
        } else {
            return Decision::Full;
        };
        self.seat(viewer, path, now);
        match path {
            Path::Relay => Decision::Relay,
            Path::Direct => Decision::Direct,
        }
    }

    /// The bot refused to admit `viewer`: a full broadcast makes the share full for them,
    /// anything else gives a direct copy while there is room for one.
    pub fn admit_failed(&mut self, viewer: u32, full: bool) -> Decision {
        let Some(seat) = self.seats.get_mut(&viewer) else {
            return Decision::Wait;
        };
        if seat.path == Path::Direct {
            return Decision::Direct;
        }
        if !full && self.direct_count() < MAX_VIEWERS {
            if let Some(seat) = self.seats.get_mut(&viewer) {
                seat.path = Path::Direct;
            }
            return Decision::Direct;
        }
        self.seats.remove(&viewer);
        Decision::Full
    }

    fn seat(&mut self, viewer: u32, path: Path, now: Instant) {
        self.next += 1;
        self.seats.insert(
            viewer,
            Seat {
                path,
                joined: self.next,
                admitted_at: now,
            },
        );
    }

    /// `viewer` stopped watching or left; returns how it watched.
    pub fn remove(&mut self, viewer: u32) -> Option<Path> {
        self.seats.remove(&viewer).map(|s| s.path)
    }

    pub fn path(&self, viewer: u32) -> Option<Path> {
        self.seats.get(&viewer).map(|s| s.path)
    }

    pub fn direct_count(&self) -> usize {
        self.seats
            .values()
            .filter(|s| s.path == Path::Direct)
            .count()
    }

    /// The viewers watching through the relay, sorted by session.
    pub fn relay_viewers(&self) -> Vec<u32> {
        self.seats
            .iter()
            .filter(|(_, s)| s.path == Path::Relay)
            .map(|(v, _)| *v)
            .collect()
    }

    /// The broadcast is gone (FR-7.49): the first [`MAX_VIEWERS`] viewers by joining stay or
    /// become direct, the rest are dropped. Returns (newly direct, dropped).
    pub fn relay_lost(&mut self) -> (Vec<u32>, Vec<u32>) {
        self.relay = false;
        let mut order: Vec<(u64, u32)> = self.seats.iter().map(|(v, s)| (s.joined, *v)).collect();
        order.sort_unstable();
        let (mut moved, mut dropped) = (Vec::new(), Vec::new());
        for (i, (_, viewer)) in order.into_iter().enumerate() {
            if i < MAX_VIEWERS {
                if let Some(seat) = self.seats.get_mut(&viewer)
                    && seat.path == Path::Relay
                {
                    seat.path = Path::Direct;
                    moved.push(viewer);
                }
            } else {
                self.seats.remove(&viewer);
                dropped.push(viewer);
            }
        }
        (moved, dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_viewers_go_through_the_relay_others_get_direct_copies() {
        let now = Instant::now();
        let mut roster = Roster::new(25);
        assert_eq!(roster.decide(1, true, now), Decision::Relay);
        assert_eq!(roster.decide(2, false, now), Decision::Direct);
        assert_eq!(roster.path(1), Some(Path::Relay));
        assert_eq!(roster.relay_viewers(), vec![1]);
        assert_eq!(roster.path(2), Some(Path::Direct));
    }

    #[test]
    fn at_most_eight_direct_copies_and_the_total_within_the_limit() {
        let now = Instant::now();
        let mut roster = Roster::new(10);
        for v in 0..8 {
            assert_eq!(roster.decide(v, false, now), Decision::Direct);
        }
        assert_eq!(roster.decide(8, false, now), Decision::Full);
        assert_eq!(roster.decide(9, true, now), Decision::Relay);
        assert_eq!(roster.decide(10, true, now), Decision::Relay);
        // 10 of 10: even a broadcast viewer is turned away.
        assert_eq!(roster.decide(11, true, now), Decision::Full);
        roster.remove(3);
        assert_eq!(roster.decide(11, true, now), Decision::Relay);
    }

    #[test]
    fn a_repeated_watch_waits_for_the_token_then_admits_again() {
        let now = Instant::now();
        let mut roster = Roster::new(25);
        assert_eq!(roster.decide(1, true, now), Decision::Relay);
        assert_eq!(
            roster.decide(1, true, now + Duration::from_secs(1)),
            Decision::Wait
        );
        assert_eq!(roster.decide(1, true, now + READMIT_AFTER), Decision::Relay);
        assert_eq!(roster.decide(2, false, now), Decision::Direct);
        assert_eq!(roster.decide(2, false, now), Decision::Direct);
    }

    #[test]
    fn a_refused_admission_falls_back_to_a_direct_copy_unless_full() {
        let now = Instant::now();
        let mut roster = Roster::new(25);
        roster.decide(1, true, now);
        roster.decide(2, true, now);
        assert_eq!(roster.admit_failed(1, false), Decision::Direct);
        assert_eq!(roster.path(1), Some(Path::Direct));
        assert_eq!(roster.admit_failed(2, true), Decision::Full);
        assert_eq!(roster.path(2), None);
        assert_eq!(roster.admit_failed(9, false), Decision::Wait);
    }

    #[test]
    fn a_lost_relay_keeps_the_first_eight_by_joining() {
        let now = Instant::now();
        let mut roster = Roster::new(25);
        // Sessions in reverse order of joining, so joining and session order differ.
        for v in (0..12).rev() {
            roster.decide(v, v != 5, now);
        }
        let (moved, dropped) = roster.relay_lost();
        // Joined first: 11, 10, …, 4; 5 was direct already.
        assert_eq!(moved, vec![11, 10, 9, 8, 7, 6, 4]);
        assert_eq!(dropped, vec![3, 2, 1, 0]);
        assert_eq!(roster.direct_count(), 8);
        assert_eq!(roster.limit(), MAX_VIEWERS);
        assert_eq!(roster.decide(20, true, now), Decision::Full);
    }
}
