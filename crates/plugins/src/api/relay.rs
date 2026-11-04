//! `relay` (FR-9.57 … FR-9.63): plugin data through the server community's relay. Needs
//! `peers.relay`.
//!
//! This module holds what only the plugin's call can tell (permission, message size) and the
//! pure rules the service applies: the `relay.status()` order, the per-plugin rate and which
//! path a message takes. The service ([`crate::api::io::PluginIo`]) owns the state; the engine
//! carries 1:1 messages through a relay pair session. Payloads are opaque bytes: never parsed,
//! rewritten or logged (FR-9.94).

use std::time::Instant;

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
    /// Through Mumble plugin data, under the `peers.messages` rate rules (FR-6.36).
    Fallback,
}

/// The path for a message of `len` bytes, or `None` for `relay-unavailable`. Channel sends
/// use the fallback until relay broadcasts carry plugin data.
pub fn path(status: RelayStatus, target: Target, len: usize, fallback: bool) -> Option<Path> {
    if status == RelayStatus::Available && matches!(target, Target::Direct(_)) {
        Some(Path::Relay)
    } else if fallback && len <= MAX_FALLBACK {
        Some(Path::Fallback)
    } else {
        None
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
    fn channel_messages_fall_back_until_relay_broadcasts_exist() {
        let channel = Target::Channel;
        assert_eq!(
            path(RelayStatus::Available, channel, 997, true),
            Some(Path::Fallback)
        );
        assert_eq!(path(RelayStatus::Available, channel, 998, true), None);
        assert_eq!(path(RelayStatus::Available, channel, 10, false), None);
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
