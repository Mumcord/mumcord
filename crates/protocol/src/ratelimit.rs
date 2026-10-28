//! Client-side pacing for messages the server rate-limits.
//!
//! Mumble servers (1.4+) pass `Version`, own `UserState`, `TextMessage`, `ChannelState` and `ACL`
//! messages through a per-user leaky bucket (defaults: 5 tokens, 1 drained per second), and plugin
//! data through a second one (`pluginmessagelimit`/`pluginmessageburst`: 15 tokens, 4 drained
//! per second); anything above is dropped silently. The server drains whole tokens only and
//! restarts its timer on each drain, losing the remainder, so its effective rate can be below the
//! nominal one. This mirrors that model with headroom and a slower drain, so bursts are delayed,
//! not lost.

use std::time::Duration;

use tokio::time::Instant;

use crate::framing::MessageType;

/// Which of the server's buckets a message counts against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Limit {
    Messages,
    PluginData,
}

/// The server bucket a message type counts against, if any.
pub(crate) fn limit_of(kind: u16) -> Option<Limit> {
    match MessageType::from_u16(kind)? {
        MessageType::Version
        | MessageType::UserState
        | MessageType::TextMessage
        | MessageType::ChannelState
        | MessageType::Acl => Some(Limit::Messages),
        MessageType::PluginDataTransmission => Some(Limit::PluginData),
        _ => None,
    }
}

pub(crate) struct LeakyBucket {
    tokens: u32,
    timer: Instant,
    max_tokens: u32,
    /// Slower than the server's drain so network jitter cannot make us look faster than allowed.
    token_interval: Duration,
}

impl LeakyBucket {
    pub(crate) fn new(limit: Limit, now: Instant) -> Self {
        let (max_tokens, token_interval) = match limit {
            // Server: 5 tokens, one per 1000 ms.
            Limit::Messages => (4, Duration::from_millis(1200)),
            // Server: 15 tokens, one per 250 ms.
            Limit::PluginData => (12, Duration::from_millis(300)),
        };
        Self {
            tokens: 0,
            timer: now,
            max_tokens,
            token_interval,
        }
    }

    fn drained(&self, now: Instant) -> u32 {
        let elapsed = now.saturating_duration_since(self.timer);
        u32::try_from(elapsed.as_millis() / self.token_interval.as_millis()).unwrap_or(u32::MAX)
    }

    /// Time until one more message may be sent (zero if it may go now).
    pub(crate) fn wait_time(&self, now: Instant) -> Duration {
        let drained = self.drained(now);
        let level = self.tokens.saturating_sub(drained);
        if level < self.max_tokens {
            return Duration::ZERO;
        }
        let needed = drained + (level - self.max_tokens + 1);
        (self.timer + self.token_interval * needed).saturating_duration_since(now)
    }

    /// Messages that may still go at once.
    pub(crate) fn tokens_left(&self, now: Instant) -> u32 {
        let level = self.tokens.saturating_sub(self.drained(now));
        self.max_tokens.saturating_sub(level)
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
        let mut bucket = LeakyBucket::new(Limit::Messages, start);
        let interval = bucket.token_interval;
        for _ in 0..bucket.max_tokens {
            assert_eq!(bucket.wait_time(start), Duration::ZERO);
            bucket.take(start);
        }
        assert_eq!(bucket.wait_time(start), interval);
        assert_eq!(bucket.tokens_left(start), 0);
        let later = start + Duration::from_millis(500);
        assert_eq!(
            bucket.wait_time(later),
            interval - Duration::from_millis(500)
        );
        let later = start + interval;
        assert_eq!(bucket.wait_time(later), Duration::ZERO);
        bucket.take(later);
        // Timer restarted on the drain: the next slot is a full interval later.
        assert_eq!(bucket.wait_time(later), interval);
    }

    /// Murmur's `LeakyBucket::ratelimit`: drains `elapsed * rate / 1000` whole tokens, restarts its
    /// timer only when at least one drained, and drops a message that would exceed the burst.
    struct ServerBucket {
        tokens: u64,
        timer: Duration,
        per_sec: u64,
        max: u64,
    }

    impl ServerBucket {
        fn accepts(&mut self, at: Duration) -> bool {
            let elapsed = u64::try_from((at - self.timer).as_millis()).expect("millis");
            let drain = elapsed * self.per_sec / 1000;
            if drain > 0 {
                self.timer = at;
            }
            self.tokens = self.tokens.saturating_sub(drain);
            if self.tokens + 1 > self.max {
                return false;
            }
            self.tokens += 1;
            true
        }
    }

    /// Sends `count` messages as fast as the client bucket allows; the server sees each one
    /// `jitter(i)` later, in order (TCP), so a delayed message bunches up with the next ones.
    fn flood(limit: Limit, server: &mut ServerBucket, count: u32, jitter: fn(u32) -> u64) {
        let start = Instant::now();
        let mut bucket = LeakyBucket::new(limit, start);
        let mut now = start;
        let mut seen = Duration::ZERO;
        for i in 0..count {
            now += bucket.wait_time(now);
            bucket.take(now);
            seen = seen.max(now - start + Duration::from_millis(jitter(i)));
            assert!(server.accepts(seen), "message {i} dropped at {seen:?}");
        }
    }

    #[test]
    fn plugin_data_floods_stay_within_murmurs_default_limit() {
        let server = || ServerBucket {
            tokens: 0,
            timer: Duration::ZERO,
            per_sec: 4,
            max: 15,
        };
        flood(Limit::PluginData, &mut server(), 500, |_| 0);
        // Network jitter: some messages arrive up to 40 ms late.
        flood(Limit::PluginData, &mut server(), 500, |i| {
            u64::from(i % 3) * 20
        });
    }

    #[test]
    fn plugin_data_is_spaced_at_least_250_ms_after_the_burst() {
        let start = Instant::now();
        let mut bucket = LeakyBucket::new(Limit::PluginData, start);
        let mut now = start;
        let mut sent = Vec::new();
        for _ in 0..100 {
            now += bucket.wait_time(now);
            bucket.take(now);
            sent.push(now);
        }
        let burst = usize::try_from(bucket.max_tokens).expect("small");
        assert!(
            sent[..burst].iter().all(|t| *t == start),
            "burst goes out at once"
        );
        for pair in sent[burst..].windows(2) {
            assert!(pair[1] - pair[0] >= Duration::from_millis(250), "{pair:?}");
        }
    }

    #[test]
    fn server_limited_types() {
        assert_eq!(
            limit_of(MessageType::UserState as u16),
            Some(Limit::Messages)
        );
        assert_eq!(
            limit_of(MessageType::TextMessage as u16),
            Some(Limit::Messages)
        );
        assert_eq!(
            limit_of(MessageType::PluginDataTransmission as u16),
            Some(Limit::PluginData)
        );
        assert_eq!(limit_of(MessageType::UdpTunnel as u16), None);
        assert_eq!(limit_of(MessageType::Ping as u16), None);
    }
}
