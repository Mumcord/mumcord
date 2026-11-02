//! Outgoing rate rules for plugin messages (FR-6.36).
//!
//! Per plugin: 1 message per 2 s sustained, burst 3. A game burst raises one plugin to 4 per
//! second for 10 s, at most once per minute per plugin and only one plugin at a time. All plugins
//! together: 1 per second sustained, burst 5, refilled at 4 per second while a game burst runs.
//! A message over a limit is refused (`rate-limited`), never queued.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::SendError;

/// One token in bucket units; fractional tokens accumulate as units.
const TOKEN: u64 = 1_000_000;

const PLUGIN_RATE_MILLI: u64 = 500;
const PLUGIN_CAPACITY: u64 = 3;
const BURST_RATE_MILLI: u64 = 4_000;
const BURST_CAPACITY: u64 = 4;
const GLOBAL_RATE_MILLI: u64 = 1_000;
const GLOBAL_BURST_RATE_MILLI: u64 = 4_000;
const GLOBAL_CAPACITY: u64 = 5;

/// How long a game burst lasts.
pub const BURST_DURATION: Duration = Duration::from_secs(10);
/// Minimum time between two game bursts of the same plugin, measured from their starts.
pub const BURST_COOLDOWN: Duration = Duration::from_secs(60);

/// A token bucket in integer units so tests are exact.
#[derive(Debug, Clone)]
struct Bucket {
    units: u64,
    capacity: u64,
    last: Instant,
}

impl Bucket {
    fn full(capacity: u64, now: Instant) -> Self {
        Self {
            units: capacity * TOKEN,
            capacity,
            last: now,
        }
    }

    /// Adds what accrued between the last refill and `until` at `rate_milli` tokens per 1000 s.
    fn refill(&mut self, until: Instant, rate_milli: u64) {
        let elapsed = until.saturating_duration_since(self.last);
        if elapsed.is_zero() {
            return;
        }
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        let gained = micros.saturating_mul(rate_milli) / 1_000;
        self.units = self.units.saturating_add(gained).min(self.capacity * TOKEN);
        self.last = until;
    }

    fn has_token(&self) -> bool {
        self.units >= TOKEN
    }

    fn take(&mut self) {
        self.units -= TOKEN;
    }
}

#[derive(Debug)]
struct PluginRate {
    normal: Bucket,
    /// Present while this plugin's game burst runs.
    burst: Option<Bucket>,
    last_burst_start: Option<Instant>,
}

impl PluginRate {
    fn new(now: Instant) -> Self {
        Self {
            normal: Bucket::full(PLUGIN_CAPACITY, now),
            burst: None,
            last_burst_start: None,
        }
    }
}

#[derive(Debug)]
struct ActiveBurst {
    plugin: String,
    ends: Instant,
}

/// The send limits of all plugins on one connection. Time is passed in (`now`) so the rules are
/// testable; callers pass monotonic instants.
#[derive(Debug)]
pub struct RateLimiter {
    plugins: HashMap<String, PluginRate>,
    global: Bucket,
    burst: Option<ActiveBurst>,
}

impl RateLimiter {
    pub fn new(now: Instant) -> Self {
        Self {
            plugins: HashMap::new(),
            global: Bucket::full(GLOBAL_CAPACITY, now),
            burst: None,
        }
    }

    /// Takes one send slot for `plugin`, or refuses with `RateLimited` without using any slot.
    pub fn try_send(&mut self, plugin: &str, now: Instant) -> Result<(), SendError> {
        self.advance(now);
        let in_burst = self.burst.as_ref().is_some_and(|b| b.plugin == plugin);
        let global_rate = self.global_rate();
        self.global.refill(now, global_rate);
        let rate = self
            .plugins
            .entry(plugin.to_owned())
            .or_insert_with(|| PluginRate::new(now));
        rate.normal.refill(now, PLUGIN_RATE_MILLI);
        let own = match (&mut rate.burst, in_burst) {
            (Some(bucket), true) => {
                bucket.refill(now, BURST_RATE_MILLI);
                bucket
            }
            _ => &mut rate.normal,
        };
        if !own.has_token() || !self.global.has_token() {
            return Err(SendError::RateLimited);
        }
        own.take();
        self.global.take();
        Ok(())
    }

    /// Starts a game burst for `plugin`. Refused with `RateLimited` if another plugin's burst runs
    /// or this plugin started one less than [`BURST_COOLDOWN`] ago.
    pub fn start_game_burst(&mut self, plugin: &str, now: Instant) -> Result<(), SendError> {
        self.advance(now);
        if self.burst.is_some() {
            return Err(SendError::RateLimited);
        }
        let rate = self
            .plugins
            .entry(plugin.to_owned())
            .or_insert_with(|| PluginRate::new(now));
        if rate
            .last_burst_start
            .is_some_and(|t| now.saturating_duration_since(t) < BURST_COOLDOWN)
        {
            return Err(SendError::RateLimited);
        }
        // Settle the global bucket at the normal rate before the faster rate applies.
        self.global.refill(now, GLOBAL_RATE_MILLI);
        rate.last_burst_start = Some(now);
        rate.burst = Some(Bucket::full(BURST_CAPACITY, now));
        self.burst = Some(ActiveBurst {
            plugin: plugin.to_owned(),
            ends: now + BURST_DURATION,
        });
        Ok(())
    }

    /// The plugin whose game burst runs at `now`, if any.
    pub fn bursting_plugin(&mut self, now: Instant) -> Option<&str> {
        self.advance(now);
        self.burst.as_ref().map(|b| b.plugin.as_str())
    }

    /// Drops a plugin's state (disabled or removed). Its burst, if running, ends.
    pub fn forget(&mut self, plugin: &str, now: Instant) {
        self.advance(now);
        if self.burst.as_ref().is_some_and(|b| b.plugin == plugin) {
            self.global.refill(now, GLOBAL_BURST_RATE_MILLI);
            self.burst = None;
        }
        self.plugins.remove(plugin);
    }

    fn global_rate(&self) -> u64 {
        if self.burst.is_some() {
            GLOBAL_BURST_RATE_MILLI
        } else {
            GLOBAL_RATE_MILLI
        }
    }

    /// Ends an expired burst, crediting the global bucket at the burst rate up to its end.
    fn advance(&mut self, now: Instant) {
        let Some(burst) = self.burst.take_if(|b| b.ends <= now) else {
            return;
        };
        self.global.refill(burst.ends, GLOBAL_BURST_RATE_MILLI);
        if let Some(rate) = self.plugins.get_mut(&burst.plugin) {
            rate.burst = None;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Tries to send every `step` for `span`, returning how many went through.
    fn spam(
        limiter: &mut RateLimiter,
        plugin: &str,
        from: Instant,
        span: Duration,
        step: Duration,
    ) -> usize {
        let mut sent = 0;
        let mut t = from;
        while t < from + span {
            if limiter.try_send(plugin, t).is_ok() {
                sent += 1;
            }
            t += step;
        }
        sent
    }

    #[test]
    fn per_plugin_one_per_two_seconds_burst_three() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(t0);
        for _ in 0..3 {
            assert_eq!(limiter.try_send("spam", t0), Ok(()));
        }
        assert_eq!(limiter.try_send("spam", t0), Err(SendError::RateLimited));
        assert_eq!(
            limiter.try_send("spam", t0 + ms(1_999)),
            Err(SendError::RateLimited)
        );
        assert_eq!(limiter.try_send("spam", t0 + ms(2_000)), Ok(()));
        // Sustained: 30 s of spamming at 10/s gets one through every 2 s (at 4 s … 30 s).
        let sent = spam(
            &mut limiter,
            "spam",
            t0 + ms(2_000),
            Duration::from_secs(30),
            ms(100),
        );
        assert_eq!(sent, 14);
    }

    #[test]
    fn global_one_per_second_burst_five() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(t0);
        let plugins = ["a", "b", "c", "d", "e", "f"];
        let ok = plugins
            .iter()
            .filter(|p| limiter.try_send(p, t0).is_ok())
            .count();
        assert_eq!(ok, 5);
        assert_eq!(
            limiter.try_send("f", t0 + ms(999)),
            Err(SendError::RateLimited)
        );
        assert_eq!(limiter.try_send("f", t0 + ms(1_000)), Ok(()));
    }

    #[test]
    fn refusal_by_the_global_bucket_keeps_the_plugin_slot() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(t0);
        for p in ["a", "b", "c", "d", "e"] {
            limiter.try_send(p, t0).unwrap();
        }
        // "f" is refused by the global bucket; its own 3 slots stay available.
        assert!(limiter.try_send("f", t0).is_err());
        for i in 1..=3 {
            assert_eq!(limiter.try_send("f", t0 + Duration::from_secs(i)), Ok(()));
        }
    }

    #[test]
    fn game_burst_four_per_second_for_ten_seconds_then_falls_back() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(t0);
        assert_eq!(limiter.start_game_burst("game", t0), Ok(()));
        assert_eq!(limiter.bursting_plugin(t0), Some("game"));
        // Burst bucket starts full (4), then 4 per second for 10 s.
        let sent = spam(&mut limiter, "game", t0, BURST_DURATION, ms(50));
        assert_eq!(sent, 4 + 39);
        assert_eq!(limiter.bursting_plugin(t0 + BURST_DURATION), None);
        // Back to the normal bucket, which refilled to 3 meanwhile; then 1 per 2 s.
        let after = t0 + BURST_DURATION;
        let sent = spam(&mut limiter, "game", after, Duration::from_secs(10), ms(50));
        assert_eq!(sent, 3 + 4);
    }

    #[test]
    fn second_burst_within_a_minute_is_refused() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(t0);
        limiter.start_game_burst("game", t0).unwrap();
        assert_eq!(
            limiter.start_game_burst("game", t0 + ms(5_000)),
            Err(SendError::RateLimited)
        );
        assert_eq!(
            limiter.start_game_burst("game", t0 + ms(59_999)),
            Err(SendError::RateLimited)
        );
        assert_eq!(
            limiter.start_game_burst("game", t0 + BURST_COOLDOWN),
            Ok(())
        );
    }

    #[test]
    fn only_one_plugin_bursts_at_a_time() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(t0);
        limiter.start_game_burst("a", t0).unwrap();
        assert_eq!(
            limiter.start_game_burst("b", t0 + ms(9_999)),
            Err(SendError::RateLimited)
        );
        assert_eq!(limiter.start_game_burst("b", t0 + BURST_DURATION), Ok(()));
    }

    #[test]
    fn other_plugins_keep_their_normal_limit_during_a_burst() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(t0);
        limiter.start_game_burst("game", t0).unwrap();
        for _ in 0..3 {
            assert_eq!(limiter.try_send("other", t0), Ok(()));
        }
        assert_eq!(limiter.try_send("other", t0), Err(SendError::RateLimited));
    }

    #[test]
    fn forgetting_a_bursting_plugin_ends_the_burst() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(t0);
        limiter.start_game_burst("a", t0).unwrap();
        limiter.forget("a", t0 + ms(1));
        assert_eq!(limiter.start_game_burst("b", t0 + ms(2)), Ok(()));
    }
}
