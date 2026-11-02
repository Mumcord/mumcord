//! Incoming flood guard: more than 5 messages per second per sender per plugin are dropped
//! (FR-6.36).

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Messages accepted per sender and plugin within [`WINDOW`].
pub const MAX_PER_WINDOW: usize = 5;
pub const WINDOW: Duration = Duration::from_secs(1);
/// Above this many tracked pairs, idle ones are pruned on the next message.
const PRUNE_ABOVE: usize = 1024;

/// Sliding one-second windows per (sender session, plugin id).
#[derive(Debug, Default)]
pub struct IncomingLimiter {
    windows: HashMap<(u32, String), VecDeque<Instant>>,
}

impl IncomingLimiter {
    /// Records a message and returns false if it must be dropped. Dropped messages do not count
    /// toward the window, so a sender that slows down gets through again after one second.
    pub fn admit(&mut self, sender: u32, plugin: &str, now: Instant) -> bool {
        if self.windows.len() > PRUNE_ABOVE {
            self.prune(now);
        }
        let window = self.windows.entry((sender, plugin.to_owned())).or_default();
        while window
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= WINDOW)
        {
            window.pop_front();
        }
        if window.len() >= MAX_PER_WINDOW {
            return false;
        }
        window.push_back(now);
        true
    }

    /// Forgets a session that left or disconnected (session ids are reused by the server).
    pub fn forget_session(&mut self, sender: u32) {
        self.windows.retain(|(session, _), _| *session != sender);
    }

    /// Forgets everything about a plugin (disabled or removed).
    pub fn forget_plugin(&mut self, plugin: &str) {
        self.windows.retain(|(_, id), _| id != plugin);
    }

    fn prune(&mut self, now: Instant) {
        self.windows.retain(|_, w| {
            w.back()
                .is_some_and(|t| now.saturating_duration_since(*t) < WINDOW)
        });
    }

    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.windows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_than_five_per_second_are_dropped() {
        let t0 = Instant::now();
        let mut limiter = IncomingLimiter::default();
        let admitted = (0..10)
            .filter(|i| limiter.admit(7, "dice", t0 + Duration::from_millis(i * 50)))
            .count();
        assert_eq!(admitted, 5);
        // Other senders and other plugins have their own window.
        assert!(limiter.admit(8, "dice", t0));
        assert!(limiter.admit(7, "poll", t0));
        // One second after the first accepted message, one slot is free again.
        assert!(limiter.admit(7, "dice", t0 + Duration::from_millis(1_000)));
        assert!(!limiter.admit(7, "dice", t0 + Duration::from_millis(1_010)));
    }

    #[test]
    fn steady_five_per_second_all_pass() {
        let t0 = Instant::now();
        let mut limiter = IncomingLimiter::default();
        assert!((0..50).all(|i| limiter.admit(1, "dice", t0 + Duration::from_millis(i * 200))));
    }

    #[test]
    fn forgets_sessions_plugins_and_idle_pairs() {
        let t0 = Instant::now();
        let mut limiter = IncomingLimiter::default();
        limiter.admit(1, "a", t0);
        limiter.admit(1, "b", t0);
        limiter.admit(2, "a", t0);
        limiter.forget_session(1);
        assert_eq!(limiter.tracked(), 1);
        limiter.forget_plugin("a");
        assert_eq!(limiter.tracked(), 0);

        for s in 0..=PRUNE_ABOVE as u32 {
            limiter.admit(s, "a", t0);
        }
        limiter.admit(9_999, "a", t0 + WINDOW);
        assert_eq!(limiter.tracked(), 1);
    }
}
