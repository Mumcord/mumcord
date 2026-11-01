//! Restart policy for crashed plugins (#268): back off 5 s, 30 s, then 5 min; after 3 failures
//! within 10 minutes the plugin stays off until the user turns it on again (FR-6.4).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const BACKOFF: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(300),
];
/// Short back-off used by the E2E hook, so a test sees three restarts in about a second.
#[cfg(debug_assertions)]
const FAST_BACKOFF: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
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

/// The back-off steps for this run. Debug builds honour `MUMCORD_PLUGIN_TEST_FAST_BACKOFF=1`
/// (AC-6.8 E2E); release builds always use [`BACKOFF`].
pub fn backoff_steps() -> [Duration; 3] {
    #[cfg(debug_assertions)]
    if std::env::var_os("MUMCORD_PLUGIN_TEST_FAST_BACKOFF").is_some_and(|v| v == "1") {
        return FAST_BACKOFF;
    }
    BACKOFF
}

#[derive(Debug)]
pub struct Health {
    backoff: [Duration; 3],
    recent: VecDeque<Instant>,
    /// Failures since the last start that the user asked for; picks the back-off step.
    streak: usize,
}

impl Default for Health {
    fn default() -> Self {
        Self::new(BACKOFF)
    }
}

impl Health {
    pub fn new(backoff: [Duration; 3]) -> Self {
        Self {
            backoff,
            recent: VecDeque::new(),
            streak: 0,
        }
    }

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
            Recovery::RetryAt(now + self.backoff[(self.streak - 1).min(self.backoff.len() - 1)])
        }
    }

    /// The user turned the plugin on again: start counting from zero.
    pub fn reset(&mut self) {
        *self = Self::new(self.backoff);
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
