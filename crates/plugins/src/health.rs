//! Restart policy for crashed plugins (#268): back off 5 s, 30 s, then 5 min; after 3 failures
//! within 10 minutes the plugin stays off until the user turns it on again.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const BACKOFF: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(300),
];
const WINDOW: Duration = Duration::from_secs(600);
const MAX_FAILURES_IN_WINDOW: usize = 3;

/// What happens after a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Start the plugin again at this time.
    RetryAt(Instant),
    /// Too many failures: stays off until the user turns it on again.
    StayOff,
}

#[derive(Debug, Default)]
pub struct Health {
    recent: VecDeque<Instant>,
    /// Failures since the last start that the user asked for; picks the back-off step.
    streak: usize,
}

impl Health {
    pub fn record_failure(&mut self, now: Instant) -> Recovery {
        while self
            .recent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= WINDOW)
        {
            self.recent.pop_front();
        }
        self.recent.push_back(now);
        self.streak += 1;
        if self.recent.len() >= MAX_FAILURES_IN_WINDOW {
            Recovery::StayOff
        } else {
            Recovery::RetryAt(now + BACKOFF[(self.streak - 1).min(BACKOFF.len() - 1)])
        }
    }

    /// The user turned the plugin on again: start counting from zero.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backs_off_then_stays_off_after_three_failures_in_ten_minutes() {
        let mut health = Health::default();
        let t0 = Instant::now();
        assert_eq!(
            health.record_failure(t0),
            Recovery::RetryAt(t0 + Duration::from_secs(5))
        );
        let t1 = t0 + Duration::from_secs(10);
        assert_eq!(
            health.record_failure(t1),
            Recovery::RetryAt(t1 + Duration::from_secs(30))
        );
        assert_eq!(
            health.record_failure(t1 + Duration::from_secs(60)),
            Recovery::StayOff
        );
    }

    #[test]
    fn old_failures_leave_the_window_but_keep_the_long_back_off() {
        let mut health = Health::default();
        let t0 = Instant::now();
        health.record_failure(t0);
        health.record_failure(t0 + Duration::from_secs(60));
        let later = t0 + Duration::from_secs(700);
        assert_eq!(
            health.record_failure(later),
            Recovery::RetryAt(later + Duration::from_secs(300))
        );
        health.reset();
        assert_eq!(
            health.record_failure(later),
            Recovery::RetryAt(later + Duration::from_secs(5))
        );
    }
}
