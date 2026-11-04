//! Pacing of the plugin-data stream to one person (FR-9.60, FR-9.86).
//!
//! The plugin's own limit (256 kbit/s, 1 MiB burst) is enforced where the plugin calls, so it
//! can get `rate-limited` at once. This pacer is the second, transport-side limit: the relay
//! drops pair packets above its per-sender cap (`pk`, default 512 kbit/s) instead of queueing
//! them, so the stream writes no faster than that cap and the relay never has to drop.

use std::time::{Duration, Instant};

use super::envelope::MAX_ENVELOPE;

/// The relay's per-sender cap when its announcement names none (mumpeer's default).
pub const DEFAULT_KBPS: u32 = 512;

#[derive(Debug)]
pub struct Pacer {
    /// Bytes per second.
    rate: f64,
    /// At least one whole envelope, so the largest message never waits for a fuller bucket
    /// than the relay's (whose burst is at least 64 KiB as well).
    burst: f64,
    tokens: f64,
    last: Instant,
}

impl Pacer {
    pub fn new(kbps: u32, now: Instant) -> Self {
        let rate = f64::from(kbps.max(1)) * 1000.0 / 8.0;
        let burst = rate.max(MAX_ENVELOPE as f64);
        Self {
            rate,
            burst,
            tokens: burst,
            last: now,
        }
    }

    /// Takes `bytes` and says how long to wait before writing them.
    pub fn delay(&mut self, bytes: usize, now: Instant) -> Duration {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        self.tokens -= bytes as f64;
        if self.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-self.tokens / self.rate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_bucket_sends_at_once_then_paces_to_the_cap() {
        let start = Instant::now();
        let mut pacer = Pacer::new(512, start);
        // 512 kbit/s = 64 000 B/s; the burst is one envelope (just over 64 KiB).
        assert_eq!(pacer.delay(MAX_ENVELOPE, start), Duration::ZERO);
        let wait = pacer.delay(64_000, start);
        assert!((wait.as_secs_f64() - 1.0).abs() < 1e-9, "{wait:?}");
    }

    #[test]
    fn time_refills_the_bucket_up_to_the_burst() {
        let start = Instant::now();
        let mut pacer = Pacer::new(8, start); // 1000 B/s
        assert_eq!(pacer.delay(MAX_ENVELOPE, start), Duration::ZERO);
        let later = start + Duration::from_secs(3600);
        assert_eq!(pacer.delay(MAX_ENVELOPE, later), Duration::ZERO);
        assert!(pacer.delay(1000, later) >= Duration::from_millis(999));
    }
}
