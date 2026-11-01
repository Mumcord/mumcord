//! Plugin messaging rules (FR-6.33 … FR-6.36), free of I/O.
//!
//! The engine moves the raw bytes and removes blocked users before anything reaches this module;
//! the caller supplies the channel members, their announces and who is a friend. Time is passed
//! in as `now` so every rule is testable.

pub mod announce;
pub mod envelope;
pub mod incoming;
pub mod rate;
pub mod recipients;

use std::collections::HashMap;
use std::time::Instant;

use crate::manifest::Visibility;

pub use announce::{AnnounceEntry, AnnounceThrottle, SourceTag};
pub use envelope::{DecodeError, Envelope};
pub use incoming::IncomingLimiter;
pub use rate::RateLimiter;
pub use recipients::{Member, PluginRef, Refusal, Target};

/// Why `peers.send` / `peers.start-game-burst` was refused (the WIT `types.error` subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("not permitted")]
    NotPermitted,
    #[error("message too large")]
    TooLarge,
    #[error("rate limited")]
    RateLimited,
    #[error("no recipients")]
    NoRecipients,
    #[error("hidden by visibility")]
    Hidden,
}

/// Per-plugin messaging counters for Diagnostics. Counts only, never contents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub sent: u64,
    /// Sends refused by a rate limit.
    pub rate_limited: u64,
    /// Sent messages the connection dropped after waiting 10 s behind Mumcord's own traffic.
    pub expired: u64,
    pub received: u64,
    /// Incoming messages over 5 per second per sender.
    pub incoming_rate_dropped: u64,
    /// Incoming frames that were not a valid envelope.
    pub incoming_invalid: u64,
    /// Incoming messages refused by visibility, or from a sender outside the channel or with a
    /// different plugin source.
    pub incoming_refused: u64,
}

/// A plugin's request to send one message.
#[derive(Debug, Clone, Copy)]
pub struct SendRequest<'a> {
    pub plugin: PluginRef<'a>,
    /// Whether `peers.messages` is granted right now (from the permission gate).
    pub permitted: bool,
    pub api_major: u8,
    /// The manifest's `messaging.max_payload`.
    pub max_payload: u32,
    pub target: Target,
    pub kind: u8,
    pub payload: &'a [u8],
}

/// A message ready for the engine's low-priority plugin lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub data_id: String,
    pub receivers: Vec<u32>,
    pub bytes: Vec<u8>,
    pub direct: bool,
}

/// A message to deliver to a plugin as `peer-message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub sender: u32,
    pub kind: u8,
    pub payload: Vec<u8>,
}

/// All messaging state of one connection: rate limits, flood guard and counters.
#[derive(Debug)]
pub struct Messenger {
    rate: RateLimiter,
    incoming: IncomingLimiter,
    counters: HashMap<String, Counters>,
}

impl Messenger {
    pub fn new(now: Instant) -> Self {
        Self {
            rate: RateLimiter::new(now),
            incoming: IncomingLimiter::default(),
            counters: HashMap::new(),
        }
    }

    /// Checks a send in the order permission → visibility → size → recipients → rate, and takes
    /// a rate slot only if everything else passed.
    pub fn send(
        &mut self,
        req: &SendRequest<'_>,
        members: &[Member<'_>],
        now: Instant,
    ) -> Result<Outgoing, SendError> {
        if !req.permitted {
            return Err(SendError::NotPermitted);
        }
        if req.plugin.visibility == Visibility::Nobody {
            return Err(SendError::Hidden);
        }
        let bytes = envelope::encode(req.api_major, req.kind, req.payload, req.max_payload)?;
        let receivers = recipients::select(&req.plugin, req.target, members)?;
        let counters = self.counters.entry(req.plugin.id.to_owned()).or_default();
        if let Err(e) = self.rate.try_send(req.plugin.id, now) {
            counters.rate_limited += 1;
            return Err(e);
        }
        counters.sent += 1;
        Ok(Outgoing {
            data_id: envelope::data_id(req.plugin.id),
            receivers,
            bytes,
            direct: matches!(req.target, Target::Direct(_)),
        })
    }

    /// `peers.start-game-burst`.
    pub fn start_game_burst(
        &mut self,
        plugin: &str,
        permitted: bool,
        now: Instant,
    ) -> Result<(), SendError> {
        if !permitted {
            return Err(SendError::NotPermitted);
        }
        self.rate.start_game_burst(plugin, now)
    }

    /// Filters one received frame for `plugin`. `sender` is the sending session's channel
    /// membership (None if not in the local user's channel). Returns what to deliver, or None if
    /// the message is dropped (and counted).
    pub fn receive(
        &mut self,
        plugin: &PluginRef<'_>,
        sender_session: u32,
        sender: Option<&Member<'_>>,
        bytes: &[u8],
        now: Instant,
    ) -> Option<Delivery> {
        let counters = self.counters.entry(plugin.id.to_owned()).or_default();
        if recipients::accept(plugin, sender).is_err() {
            counters.incoming_refused += 1;
            return None;
        }
        if !self.incoming.admit(sender_session, plugin.id, now) {
            counters.incoming_rate_dropped += 1;
            return None;
        }
        let Ok(envelope) = envelope::decode(bytes) else {
            counters.incoming_invalid += 1;
            return None;
        };
        counters.received += 1;
        Some(Delivery {
            sender: sender_session,
            kind: envelope.kind,
            payload: envelope.payload,
        })
    }

    /// Records that the connection dropped one of this plugin's messages after 10 s.
    pub fn record_expired(&mut self, plugin: &str) {
        self.counters.entry(plugin.to_owned()).or_default().expired += 1;
    }

    pub fn counters(&self, plugin: &str) -> Counters {
        self.counters.get(plugin).copied().unwrap_or_default()
    }

    /// A user left the channel or disconnected.
    pub fn forget_session(&mut self, session: u32) {
        self.incoming.forget_session(session);
    }

    /// A plugin was disabled or removed; its counters stay for Diagnostics.
    pub fn forget_plugin(&mut self, plugin: &str, now: Instant) {
        self.rate.forget(plugin, now);
        self.incoming.forget_plugin(plugin);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use super::*;

    const ID: &str = "echolings";

    fn plugin(visibility: Visibility) -> PluginRef<'static> {
        PluginRef {
            id: ID,
            source: SourceTag::Official,
            visibility,
        }
    }

    fn request(visibility: Visibility, payload: &[u8]) -> SendRequest<'_> {
        SendRequest {
            plugin: plugin(visibility),
            permitted: true,
            api_major: 1,
            max_payload: 997,
            target: Target::Channel,
            kind: 3,
            payload,
        }
    }

    fn peers() -> Vec<AnnounceEntry> {
        vec![AnnounceEntry {
            id: ID.into(),
            api_major: 1,
            source: SourceTag::Official,
        }]
    }

    #[test]
    fn send_builds_the_wire_message() {
        let t0 = Instant::now();
        let entries = peers();
        let members = [Member {
            session: 5,
            is_friend: false,
            plugins: &entries,
        }];
        let mut m = Messenger::new(t0);
        let out = m
            .send(&request(Visibility::Everyone, b"hi"), &members, t0)
            .unwrap();
        assert_eq!(
            out,
            Outgoing {
                data_id: "mumcord.p.echolings".into(),
                receivers: vec![5],
                bytes: vec![1, 1, 3, b'h', b'i'],
                direct: false
            }
        );
        assert_eq!(m.counters(ID).sent, 1);
    }

    #[test]
    fn errors_come_in_spec_order() {
        let t0 = Instant::now();
        let mut m = Messenger::new(t0);
        let big = [0u8; 998];
        let mut req = request(Visibility::Nobody, &big);
        req.permitted = false;
        assert_eq!(m.send(&req, &[], t0), Err(SendError::NotPermitted));
        req.permitted = true;
        assert_eq!(m.send(&req, &[], t0), Err(SendError::Hidden));
        req.plugin.visibility = Visibility::Everyone;
        assert_eq!(m.send(&req, &[], t0), Err(SendError::TooLarge));
        req.payload = b"ok";
        assert_eq!(m.send(&req, &[], t0), Err(SendError::NoRecipients));
        assert_eq!(
            m.counters(ID),
            Counters::default(),
            "refusals before the rate check use no slot"
        );
    }

    #[test]
    fn rate_limit_is_counted() {
        let t0 = Instant::now();
        let entries = peers();
        let members = [Member {
            session: 5,
            is_friend: false,
            plugins: &entries,
        }];
        let mut m = Messenger::new(t0);
        let req = request(Visibility::Everyone, b"x");
        let results: Vec<_> = (0..5).map(|_| m.send(&req, &members, t0)).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 3);
        assert_eq!(results[4], Err(SendError::RateLimited));
        let c = m.counters(ID);
        assert_eq!((c.sent, c.rate_limited), (3, 2));
        m.record_expired(ID);
        assert_eq!(m.counters(ID).expired, 1);
    }

    #[test]
    fn game_burst_needs_permission() {
        let t0 = Instant::now();
        let mut m = Messenger::new(t0);
        assert_eq!(
            m.start_game_burst(ID, false, t0),
            Err(SendError::NotPermitted)
        );
        assert_eq!(m.start_game_burst(ID, true, t0), Ok(()));
        assert_eq!(
            m.start_game_burst(ID, true, t0),
            Err(SendError::RateLimited)
        );
    }

    #[test]
    fn nobody_neither_sends_nor_receives() {
        let t0 = Instant::now();
        let entries = peers();
        let member = Member {
            session: 5,
            is_friend: true,
            plugins: &entries,
        };
        let mut m = Messenger::new(t0);
        assert_eq!(
            m.send(&request(Visibility::Nobody, b"x"), &[member], t0),
            Err(SendError::Hidden)
        );
        assert_eq!(
            m.receive(
                &plugin(Visibility::Nobody),
                5,
                Some(&member),
                &[1, 1, 0],
                t0
            ),
            None
        );
        assert_eq!(m.counters(ID).incoming_refused, 1);
    }

    #[test]
    fn receive_filters_and_counts() {
        let t0 = Instant::now();
        let entries = peers();
        let member = Member {
            session: 5,
            is_friend: false,
            plugins: &entries,
        };
        let everyone = plugin(Visibility::Everyone);
        let mut m = Messenger::new(t0);
        assert_eq!(
            m.receive(&everyone, 5, Some(&member), &[1, 1, 9, 42], t0),
            Some(Delivery {
                sender: 5,
                kind: 9,
                payload: vec![42]
            })
        );
        assert_eq!(m.receive(&everyone, 5, Some(&member), &[9, 1, 9], t0), None);
        assert_eq!(m.receive(&everyone, 6, None, &[1, 1, 9], t0), None);
        assert_eq!(
            m.receive(
                &plugin(Visibility::Friends),
                5,
                Some(&member),
                &[1, 1, 9],
                t0
            ),
            None
        );
        for i in 0..10 {
            m.receive(
                &everyone,
                5,
                Some(&member),
                &[1, 1, 0],
                t0 + Duration::from_millis(i),
            );
        }
        let c = m.counters(ID);
        assert_eq!(
            c.received,
            1 + 3,
            "5 per second including the first and the invalid frame"
        );
        assert_eq!(c.incoming_invalid, 1);
        assert_eq!(c.incoming_refused, 2);
        assert_eq!(c.incoming_rate_dropped, 7);
    }
}
