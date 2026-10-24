//! Client-side pacing for messages the server rate-limits.
//!
//! Mumble servers (1.4+) pass `Version`, own `UserState`, `TextMessage`, `ChannelState` and `ACL`
//! messages through a per-user leaky bucket (defaults: 5 tokens, 1 drained per second) and
//! silently drop anything above it. The server drains whole tokens only and restarts its timer
//! on each drain, losing the remainder, so its effective rate can be below 1/s. This mirrors
//! that model with one token of headroom and a slower drain, so bursts are delayed, not lost.

use std::time::Duration;

use tokio::time::Instant;

use crate::framing::MessageType;

const MAX_TOKENS: u32 = 4;
/// Slower than the server's 1 s so network jitter cannot make us look faster than allowed.
const TOKEN_INTERVAL: Duration = Duration::from_millis(1200);

/// Whether the server applies its message rate limit to this type.
pub(crate) fn is_limited(kind: u16) -> bool {
    matches!(
        MessageType::from_u16(kind),
        Some(
            MessageType::Version
                | MessageType::UserState
                | MessageType::TextMessage
                | MessageType::ChannelState
                | MessageType::Acl
        )
    )
}

pub(crate) struct LeakyBucket {
    tokens: u32,
    timer: Instant,
}

impl LeakyBucket {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            tokens: 0,
            timer: now,
        }
    }

    fn drained(&self, now: Instant) -> u32 {
        let elapsed = now.saturating_duration_since(self.timer);
        u32::try_from(elapsed.as_millis() / TOKEN_INTERVAL.as_millis()).unwrap_or(u32::MAX)
    }

    /// Time until one more message may be sent (zero if it may go now).
    pub(crate) fn wait_time(&self, now: Instant) -> Duration {
        let drained = self.drained(now);
        let level = self.tokens.saturating_sub(drained);
        if level < MAX_TOKENS {
            return Duration::ZERO;
        }
        let needed = drained + (level - MAX_TOKENS + 1);
        (self.timer + TOKEN_INTERVAL * needed).saturating_duration_since(now)
    }

    /// Records a sent message.
    pub(crate) fn take(&mut self, now: Instant) {
        let drained = self.drained(now);
        if drained > 0 {
            self.timer = now;
        }
        self.tokens = self.tokens.saturating_sub(drained) + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_burst_then_paces() {
        let start = Instant::now();
        let mut bucket = LeakyBucket::new(start);
        for _ in 0..MAX_TOKENS {
            assert_eq!(bucket.wait_time(start), Duration::ZERO);
            bucket.take(start);
        }
        assert_eq!(bucket.wait_time(start), TOKEN_INTERVAL);
        let later = start + Duration::from_millis(500);
        assert_eq!(
            bucket.wait_time(later),
            TOKEN_INTERVAL - Duration::from_millis(500)
        );
        let later = start + TOKEN_INTERVAL;
        assert_eq!(bucket.wait_time(later), Duration::ZERO);
        bucket.take(later);
        // Timer restarted on the drain: the next slot is a full interval later.
        assert_eq!(bucket.wait_time(later), TOKEN_INTERVAL);
    }

    #[test]
    fn only_server_limited_types() {
        assert!(is_limited(MessageType::UserState as u16));
        assert!(is_limited(MessageType::TextMessage as u16));
        assert!(!is_limited(MessageType::UdpTunnel as u16));
        assert!(!is_limited(MessageType::Ping as u16));
    }
}
