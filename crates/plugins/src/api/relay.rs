//! `relay` (FR-9.57 … FR-9.63): plugin data through the server community's relay. Needs
//! `peers.relay`.
//!
//! This module holds what only the plugin's call can tell (permission, message size) and the
//! pure rules the service applies: the `relay.status()` order, the per-plugin rate and which
//! path a message takes. The service ([`crate::api::io::PluginIo`]) owns the state; the engine
//! carries 1:1 messages through a relay pair session. Payloads are opaque bytes: never parsed,
//! rewritten or logged (FR-9.94).

use std::time::{Duration, Instant};

use crate::messaging::Target;
use crate::messaging::envelope::MAX_PAYLOAD;
use crate::runtime::PluginState;
use crate::runtime::wit::types::Error;

/// The permission every `relay` call needs.
pub const PERMISSION: &str = "peers.relay";
/// Largest message through the relay (FR-9.60).
pub const MAX_MESSAGE: usize = 64 * 1024;
/// Largest message the Mumble plugin-data fallback carries (FR-9.61).
pub const MAX_FALLBACK: usize = MAX_PAYLOAD;
/// 256 kbit/s sustained per plugin (FR-9.60).
pub const SUSTAINED_BYTES_PER_SEC: u32 = 256 * 1000 / 8;
/// The burst on top of the sustained rate (FR-9.60).
pub const BURST_BYTES: u32 = 1024 * 1024;
/// The plugin API the relay envelope names for a 1.1 plugin (FR-9.62). `relay` exists from 1.1
/// on, so every sender speaks it.
pub const ENVELOPE_API: (u8, u8) = (1, 1);

/// `relay.status()` (WIT `relay-status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayStatus {
    Available,
    /// The relay does not allow plugin data, or the P2P mode or a missing client certificate
    /// rule it out.
    NotAllowed,
    NoRelay,
    /// The global switch or the plugin's `peers.relay` is off.
    SwitchedOff,
}

/// What the engine reports for one server (mirrors the engine's `PluginRelayAvailability`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Availability {
    #[default]
    NoRelay,
    /// The relay does not announce `plugin_data`.
    UseNotAllowed,
    /// The P2P mode does not allow the relay, or there is no client certificate.
    NotAllowedHere,
    /// `kbps`: the relay's per-sender cap, when announced.
    Available { kbps: Option<u32> },
}

/// `relay.status()` in the order of FR-9.57's conditions: the relay announces `plugin_data`,
/// the global switch, the plugin's permission, the P2P mode and the client certificate.
pub fn status(availability: Availability, switch_on: bool, permitted: bool) -> RelayStatus {
    match availability {
        Availability::NoRelay => RelayStatus::NoRelay,
        Availability::UseNotAllowed => RelayStatus::NotAllowed,
        _ if !switch_on || !permitted => RelayStatus::SwitchedOff,
        Availability::NotAllowedHere => RelayStatus::NotAllowed,
        Availability::Available { .. } => RelayStatus::Available,
    }
}

/// How a message leaves (FR-9.59, FR-9.61).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// Through the relay pair session to the one receiver.
    Relay,
    /// Through the sender's relay broadcast to the channel; members that did not join it get
    /// the fallback when the message fits (FR-9.61).
    Broadcast,
    /// Through Mumble plugin data, under the `peers.messages` rate rules (FR-6.36).
    Fallback,
}

/// The path for a message of `len` bytes, or `None` for `relay-unavailable`.
pub fn path(status: RelayStatus, target: Target, len: usize, fallback: bool) -> Option<Path> {
    if status == RelayStatus::Available {
        Some(match target {
            Target::Direct(_) => Path::Relay,
            Target::Channel => Path::Broadcast,
        })
    } else if fallback && len <= MAX_FALLBACK {
        Some(Path::Fallback)
    } else {
        None
    }
}

/// Channel messages a sender's relay broadcast holds while it starts (FR-9.60); the engine keeps
/// the same limits. Only messages that reached nobody through Mumble count.
pub const START_QUEUE_MESSAGES: usize = 8;
/// Plugin data bytes the start queue holds.
pub const START_QUEUE_BYTES: usize = 256 * 1024;
/// How long the engine waits for a broadcast to start before the queued messages fail.
pub const START_TIMEOUT: Duration = Duration::from_secs(8);

/// The sender's relay broadcast as the engine reports it (mirrors the engine's
/// `PluginBroadcastState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BroadcastState {
    Live,
    /// Could not start or was lost; the engine tries again after `retry_in`.
    Down {
        retry_in: Duration,
    },
    /// Ended; the next channel send starts anew.
    Ended,
}

/// What the service knows about the sender's relay broadcast on one server, so a channel
/// message that cannot arrive returns an error instead of "sent".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Broadcast {
    /// Not started; the next channel send starts it.
    #[default]
    Idle,
    /// The engine is starting it and queues the messages sent meanwhile.
    Starting {
        since: Instant,
        messages: usize,
        bytes: usize,
    },
    Live,
    /// Down until `until`; the engine drops channel sends until then.
    Down {
        until: Instant,
    },
}

impl Broadcast {
    /// What the engine reported, at `now`.
    pub fn reported(state: BroadcastState, now: Instant) -> Self {
        match state {
            BroadcastState::Live => Self::Live,
            BroadcastState::Down { retry_in } => Self::Down {
                until: now + retry_in,
            },
            BroadcastState::Ended => Self::Idle,
        }
    }

    /// Takes a channel message of `len` bytes; `fell_back` when every receiver outside the
    /// broadcast got it through Mumble plugin data. Refused when it would reach nobody: the
    /// broadcast is down or did not start in time (`relay-unavailable`), or the start queue is
    /// full (`rate-limited`).
    pub fn accept(&mut self, len: usize, fell_back: bool, now: Instant) -> Result<(), SendError> {
        let queued = usize::from(!fell_back);
        let queued_bytes = if fell_back { 0 } else { len };
        match self {
            Self::Idle => {}
            Self::Down { until } if *until <= now => {}
            Self::Live => return Ok(()),
            Self::Down { .. } if fell_back => return Ok(()),
            Self::Down { .. } => return Err(SendError::Unavailable),
            Self::Starting { .. } if fell_back => return Ok(()),
            Self::Starting { since, .. } if now.duration_since(*since) >= START_TIMEOUT => {
                return Err(SendError::Unavailable);
            }
            Self::Starting {
                messages, bytes, ..
            } => {
                if *messages >= START_QUEUE_MESSAGES || *bytes + len > START_QUEUE_BYTES {
                    return Err(SendError::Error(Error::RateLimited));
                }
                *messages += 1;
                *bytes += len;
                return Ok(());
            }
        }
        *self = Self::Starting {
            since: now,
            messages: queued,
            bytes: queued_bytes,
        };
        Ok(())
    }
}

/// Why a `relay` send was refused: a 1.0 error or the 1.1 `relay-unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    Error(Error),
    Unavailable,
}

impl From<Error> for SendError {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

/// One plugin's relay rate: 256 kbit/s sustained with a 1 MiB burst (FR-9.60).
#[derive(Debug, Clone)]
pub struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    pub fn new(now: Instant) -> Self {
        Self {
            tokens: f64::from(BURST_BYTES),
            last: now,
        }
    }

    /// Takes `bytes` if the bucket holds them; a refused message costs nothing.
    pub fn take(&mut self, bytes: usize, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * f64::from(SUSTAINED_BYTES_PER_SEC))
            .min(f64::from(BURST_BYTES));
        let bytes = bytes as f64;
        if bytes > self.tokens {
            return false;
        }
        self.tokens -= bytes;
        true
    }
}

/// `relay.status`.
pub(crate) fn status_of(state: &PluginState) -> RelayStatus {
    if !state.ctx.grants.allows(&state.id, PERMISSION) {
        return RelayStatus::SwitchedOff;
    }
    state
        .io()
        .map_or(RelayStatus::NoRelay, |io| io.relay_status(&state.id))
}

/// `relay.send-channel` and `relay.send-direct`.
pub(crate) fn send(
    state: &PluginState,
    target: Target,
    data: Vec<u8>,
    fallback: bool,
) -> Result<(), SendError> {
    state.ctx.grants.check(&state.id, PERMISSION)?;
    if data.len() > MAX_MESSAGE {
        return Err(Error::TooLarge.into());
    }
    state.io()?.relay_send(&state.id, target, data, fallback)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const AVAILABLE: Availability = Availability::Available { kbps: Some(512) };

    #[test]
    fn status_follows_the_fr_9_57_order() {
        use Availability::*;
        use RelayStatus as S;
        let cases = [
            (NoRelay, true, true, S::NoRelay),
            (NoRelay, false, false, S::NoRelay),
            (UseNotAllowed, true, true, S::NotAllowed),
            (UseNotAllowed, false, true, S::NotAllowed),
            (AVAILABLE, false, true, S::SwitchedOff),
            (AVAILABLE, true, false, S::SwitchedOff),
            (NotAllowedHere, false, true, S::SwitchedOff),
            (NotAllowedHere, true, true, S::NotAllowed),
            (AVAILABLE, true, true, S::Available),
            (Available { kbps: None }, true, true, S::Available),
        ];
        for (availability, switch, permitted, expected) in cases {
            assert_eq!(
                status(availability, switch, permitted),
                expected,
                "{availability:?} switch={switch} permitted={permitted}"
            );
        }
    }

    #[test]
    fn channel_sends_queue_while_the_broadcast_starts_up_to_eight_messages() {
        let now = Instant::now();
        let mut broadcast = Broadcast::default();
        for _ in 0..START_QUEUE_MESSAGES {
            assert_eq!(broadcast.accept(10, false, now), Ok(()));
        }
        assert_eq!(
            broadcast.accept(10, false, now),
            Err(SendError::Error(Error::RateLimited))
        );
        // A message every receiver got through Mumble is not queued.
        assert_eq!(broadcast.accept(10, true, now), Ok(()));
        broadcast = Broadcast::reported(BroadcastState::Live, now);
        assert_eq!(broadcast.accept(MAX_MESSAGE, false, now), Ok(()));
    }

    #[test]
    fn the_start_queue_holds_at_most_256_kib() {
        let now = Instant::now();
        let mut broadcast = Broadcast::default();
        for _ in 0..4 {
            assert_eq!(broadcast.accept(MAX_MESSAGE, false, now), Ok(()));
        }
        assert_eq!(
            broadcast.accept(1, false, now),
            Err(SendError::Error(Error::RateLimited))
        );
    }

    #[test]
    fn a_broadcast_that_does_not_start_or_is_down_refuses_relay_only_messages() {
        let now = Instant::now();
        let mut broadcast = Broadcast::default();
        assert_eq!(broadcast.accept(10, false, now), Ok(()));
        let late = now + START_TIMEOUT;
        assert_eq!(
            broadcast.accept(10, false, late),
            Err(SendError::Unavailable)
        );
        let retry_in = Duration::from_secs(30);
        broadcast = Broadcast::reported(BroadcastState::Down { retry_in }, now);
        assert_eq!(
            broadcast.accept(10, false, now),
            Err(SendError::Unavailable)
        );
        // The fallback reached everyone: sent.
        assert_eq!(broadcast.accept(10, true, now), Ok(()));
        // Time to try again: the send starts the broadcast anew.
        assert_eq!(broadcast.accept(10, false, now + retry_in), Ok(()));
        assert!(matches!(broadcast, Broadcast::Starting { messages: 1, .. }));
        broadcast = Broadcast::reported(BroadcastState::Ended, now);
        assert_eq!(broadcast, Broadcast::Idle);
    }

    #[test]
    fn direct_messages_take_the_relay_when_it_is_available() {
        let direct = Target::Direct(3);
        assert_eq!(
            path(RelayStatus::Available, direct, MAX_MESSAGE, false),
            Some(Path::Relay)
        );
        // Without the relay only small messages that allow it fall back (FR-9.61).
        for status in [
            RelayStatus::NoRelay,
            RelayStatus::NotAllowed,
            RelayStatus::SwitchedOff,
        ] {
            assert_eq!(
                path(status, direct, MAX_FALLBACK, true),
                Some(Path::Fallback)
            );
            assert_eq!(path(status, direct, MAX_FALLBACK + 1, true), None);
            assert_eq!(path(status, direct, 10, false), None);
        }
    }

    #[test]
    fn channel_messages_take_the_broadcast_when_the_relay_is_available() {
        let channel = Target::Channel;
        assert_eq!(
            path(RelayStatus::Available, channel, MAX_MESSAGE, false),
            Some(Path::Broadcast)
        );
        assert_eq!(
            path(RelayStatus::NoRelay, channel, MAX_FALLBACK, true),
            Some(Path::Fallback)
        );
        assert_eq!(
            path(RelayStatus::NoRelay, channel, MAX_FALLBACK + 1, true),
            None
        );
        assert_eq!(path(RelayStatus::SwitchedOff, channel, 10, false), None);
    }

    #[test]
    fn the_bucket_allows_a_1_mib_burst_then_256_kbit_per_second() {
        let start = Instant::now();
        let mut bucket = Bucket::new(start);
        for _ in 0..16 {
            assert!(bucket.take(MAX_MESSAGE, start));
        }
        assert!(!bucket.take(1, start), "the burst is used up");
        // Two seconds refill 64 000 bytes: not quite one full message.
        let later = start + Duration::from_secs(2);
        assert!(!bucket.take(MAX_MESSAGE, later));
        assert!(bucket.take(64_000, later), "a refused take costs nothing");
        let much_later = later + Duration::from_secs(3600);
        assert!(bucket.take(BURST_BYTES as usize, much_later));
        assert!(!bucket.take(1, much_later), "never more than the burst");
    }
}
