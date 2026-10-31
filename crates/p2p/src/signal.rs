//! Signalling messages that introduce two Mumcord clients to each other (FR-2.35).
//!
//! They travel as Mumble plugin data with the id [`DATA_ID`] through the server both users are
//! on; this module only encodes and decodes them, the caller moves the bytes. Small versioned
//! JSON: `{"v":1,"t":"offer","s":"<16 hex>","c":["203.0.113.5:40000","[2001:db8::1]:40000"]}`,
//! `"t":"answer"` with the same fields, or `{"v":1,"t":"decline","s":…,"r":"declined"}`.
//! Offer and answer carry `"rl":1` when the sender can use the relay announced on the server
//! (spec 04, FR-4.13). A sender that hides its address sends no candidates and `"rl":1`. The
//! field is not called `r` because `r` is the decline reason string and released clients would
//! reject a number there.
//! Received messages come from another client and are untrusted: the decoder checks the size
//! before parsing, caps the candidate list and rejects addresses that can never be a peer.

use std::net::{IpAddr, SocketAddr};

use serde::{Deserialize, Serialize};

use mumcord_protocol::PLUGIN_DATA_MAX;

/// Plugin data id of the signalling messages; other ids are not for this crate.
pub const DATA_ID: &str = "mumcord.p2p";
/// The message version this client sends. Any version >= 1 is accepted on receipt and unknown
/// fields are ignored. Minor additions keep v=1 and old clients ignore unknown fields; a breaking change gets a new data id.
pub const VERSION: u32 = 1;
/// Largest message accepted or produced (bytes); standard servers drop bigger plugin data.
pub const MAX_SIGNAL_BYTES: usize = PLUGIN_DATA_MAX;
/// Most address candidates in one offer or answer.
pub const MAX_CANDIDATES: usize = 8;

/// Identifies one connection attempt so an answer can be matched to its offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

impl SessionId {
    pub(crate) fn random() -> Self {
        Self(rand::random())
    }
}

/// Why the other side will not connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclineReason {
    /// The user did not allow direct connections.
    Declined,
    /// Direct connections are allowed but could not be set up on that side.
    Unavailable,
}

/// One signalling message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// Sent by the side that wants a connection: where it can be reached.
    Offer {
        session: SessionId,
        candidates: Vec<SocketAddr>,
        /// The sender can use the relay announced on this server.
        relay: bool,
    },
    /// Reply to an offer: where the answering side can be reached.
    Answer {
        session: SessionId,
        candidates: Vec<SocketAddr>,
        /// The sender can use the relay announced on this server.
        relay: bool,
    },
    /// Reply to an offer: no connection.
    Decline {
        session: SessionId,
        reason: DeclineReason,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignalError {
    #[error("message larger than {MAX_SIGNAL_BYTES} bytes")]
    TooLarge,
    #[error("message is not valid signalling JSON")]
    Malformed,
    #[error("unsupported message version {0}")]
    UnsupportedVersion(u32),
    #[error("unknown message type")]
    UnknownType,
    #[error("invalid session id")]
    BadSession,
    #[error("more than {MAX_CANDIDATES} candidates")]
    TooManyCandidates,
    #[error("invalid address candidate")]
    BadCandidate,
    #[error("unknown decline reason")]
    BadReason,
    #[error("message does not belong to this connection attempt")]
    Unexpected,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    v: u32,
    t: String,
    s: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    c: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    r: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rl: Option<u8>,
}

impl Signal {
    pub fn session(&self) -> SessionId {
        match self {
            Self::Offer { session, .. }
            | Self::Answer { session, .. }
            | Self::Decline { session, .. } => *session,
        }
    }

    /// Encodes the message for plugin data. Fails only when the message would not fit, which
    /// cannot happen for messages built by this crate (at most [`MAX_CANDIDATES`] addresses).
    pub fn encode(&self) -> Result<Vec<u8>, SignalError> {
        let (t, candidates, r, relay) = match self {
            Self::Offer {
                candidates, relay, ..
            } => ("offer", candidates.as_slice(), None, *relay),
            Self::Answer {
                candidates, relay, ..
            } => ("answer", candidates.as_slice(), None, *relay),
            Self::Decline { reason, .. } => {
                ("decline", [].as_slice(), Some(reason_str(*reason)), false)
            }
        };
        if candidates.len() > MAX_CANDIDATES {
            return Err(SignalError::TooManyCandidates);
        }
        let wire = Wire {
            v: VERSION,
            t: t.to_owned(),
            s: format!("{:016x}", self.session().0),
            c: candidates.iter().map(SocketAddr::to_string).collect(),
            r: r.map(str::to_owned),
            rl: relay.then_some(1),
        };
        let bytes = serde_json::to_vec(&wire).map_err(|_| SignalError::Malformed)?;
        if bytes.len() > MAX_SIGNAL_BYTES {
            return Err(SignalError::TooLarge);
        }
        Ok(bytes)
    }

    /// Decodes a received message; anything unexpected is an error, never a panic.
    pub fn decode(bytes: &[u8]) -> Result<Self, SignalError> {
        if bytes.len() > MAX_SIGNAL_BYTES {
            return Err(SignalError::TooLarge);
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|_| SignalError::Malformed)?;
        if wire.v < VERSION {
            return Err(SignalError::UnsupportedVersion(wire.v));
        }
        let session = parse_session(&wire.s)?;
        if wire.c.len() > MAX_CANDIDATES {
            return Err(SignalError::TooManyCandidates);
        }
        let candidates = wire
            .c
            .iter()
            .map(|c| parse_candidate(c))
            .collect::<Result<Vec<_>, _>>()?;
        // Any other value is a future meaning this client does not know, so not "can relay".
        let relay = wire.rl == Some(1);
        match wire.t.as_str() {
            "offer" => Ok(Self::Offer {
                session,
                candidates,
                relay,
            }),
            "answer" => Ok(Self::Answer {
                session,
                candidates,
                relay,
            }),
            "decline" => {
                let reason = match wire.r.as_deref() {
                    Some("declined") => DeclineReason::Declined,
                    Some("unavailable") => DeclineReason::Unavailable,
                    _ => return Err(SignalError::BadReason),
                };
                Ok(Self::Decline { session, reason })
            }
            _ => Err(SignalError::UnknownType),
        }
    }
}

fn reason_str(reason: DeclineReason) -> &'static str {
    match reason {
        DeclineReason::Declined => "declined",
        DeclineReason::Unavailable => "unavailable",
    }
}

fn parse_session(s: &str) -> Result<SessionId, SignalError> {
    let valid = s.len() == 16 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !valid {
        return Err(SignalError::BadSession);
    }
    u64::from_str_radix(s, 16)
        .map(SessionId)
        .map_err(|_| SignalError::BadSession)
}

fn parse_candidate(s: &str) -> Result<SocketAddr, SignalError> {
    let addr: SocketAddr = s.parse().map_err(|_| SignalError::BadCandidate)?;
    if is_possible_peer(addr) {
        Ok(addr)
    } else {
        Err(SignalError::BadCandidate)
    }
}

/// Addresses no peer can ever be reached at. Loopback is allowed here and filtered at dial time,
/// so tests can run two clients on one machine.
fn is_possible_peer(addr: SocketAddr) -> bool {
    if addr.port() == 0 {
        return false;
    }
    match addr.ip() {
        IpAddr::V4(ip) => !(ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast()),
        // Link-local IPv6 needs an interface scope that means nothing on the other machine.
        IpAddr::V6(ip) => {
            !(ip.is_unspecified() || ip.is_multicast() || ip.is_unicast_link_local())
                && ip.to_ipv4_mapped().is_none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> Signal {
        Signal::Offer {
            session: SessionId(0x0123_4567_89ab_cdef),
            candidates: vec![
                "203.0.113.5:40000".parse().expect("addr"),
                "[2001:db8::1]:40001".parse().expect("addr"),
            ],
            relay: false,
        }
    }

    #[test]
    fn round_trips_every_type() {
        let decline = Signal::Decline {
            session: SessionId(7),
            reason: DeclineReason::Declined,
        };
        let answer = Signal::Answer {
            session: SessionId(u64::MAX),
            candidates: vec![],
            relay: true,
        };
        let relay_offer = Signal::Offer {
            session: SessionId(9),
            candidates: vec!["203.0.113.5:40000".parse().expect("addr")],
            relay: true,
        };
        for signal in [offer(), answer, decline, relay_offer] {
            let bytes = signal.encode().expect("encode");
            assert_eq!(Signal::decode(&bytes).expect("decode"), signal);
        }
    }

    #[test]
    fn full_offer_fits_plugin_data() {
        let candidates = (0..MAX_CANDIDATES as u16)
            .map(|i| {
                format!("[2001:db8:ffff:ffff:ffff:ffff:ffff:{i:x}]:65535")
                    .parse()
                    .expect("addr")
            })
            .collect();
        let signal = Signal::Offer {
            session: SessionId(u64::MAX),
            candidates,
            relay: true,
        };
        assert!(signal.encode().expect("encode").len() <= MAX_SIGNAL_BYTES);
    }

    #[test]
    fn rejects_oversized_before_parsing() {
        let mut bytes = offer().encode().expect("encode");
        bytes.resize(MAX_SIGNAL_BYTES + 1, b' ');
        assert_eq!(Signal::decode(&bytes), Err(SignalError::TooLarge));
    }

    #[test]
    fn newer_version_with_extra_fields_decodes() {
        let json = r#"{"v":2,"t":"offer","s":"0000000000000001","c":["203.0.113.5:40000"],"future":{"x":1}}"#;
        assert_eq!(
            Signal::decode(json.as_bytes()),
            Ok(Signal::Offer {
                session: SessionId(1),
                candidates: vec!["203.0.113.5:40000".parse().expect("addr")],
                relay: false,
            })
        );
    }

    #[test]
    fn relay_flag_is_rl_and_absent_when_false() {
        let hidden = Signal::Offer {
            session: SessionId(1),
            candidates: vec![],
            relay: true,
        };
        let json = String::from_utf8(hidden.encode().expect("encode")).expect("utf8");
        assert_eq!(json, r#"{"v":1,"t":"offer","s":"0000000000000001","rl":1}"#);
        let json = String::from_utf8(offer().encode().expect("encode")).expect("utf8");
        assert!(!json.contains("rl"), "{json}");
        let unknown = r#"{"v":1,"t":"answer","s":"0000000000000001","rl":2}"#;
        assert!(matches!(
            Signal::decode(unknown.as_bytes()),
            Ok(Signal::Answer { relay: false, .. })
        ));
    }

    #[test]
    fn rejects_malformed_input() {
        let cases: &[(&str, SignalError)] = &[
            ("", SignalError::Malformed),
            ("not json", SignalError::Malformed),
            ("[1,2]", SignalError::Malformed),
            (r#"{"v":1,"t":"offer"}"#, SignalError::Malformed),
            (
                r#"{"v":0,"t":"offer","s":"0000000000000001"}"#,
                SignalError::UnsupportedVersion(0),
            ),
            (
                r#"{"v":1,"t":"hello","s":"0000000000000001"}"#,
                SignalError::UnknownType,
            ),
            (r#"{"v":1,"t":"offer","s":"1"}"#, SignalError::BadSession),
            (
                r#"{"v":1,"t":"offer","s":"000000000000000G"}"#,
                SignalError::BadSession,
            ),
            (
                r#"{"v":1,"t":"offer","s":"+00000000000000a"}"#,
                SignalError::BadSession,
            ),
            (
                r#"{"v":1,"t":"offer","s":"0000000000000001","c":["nope"]}"#,
                SignalError::BadCandidate,
            ),
            (
                r#"{"v":1,"t":"decline","s":"0000000000000001","r":"because"}"#,
                SignalError::BadReason,
            ),
            (
                r#"{"v":1,"t":"decline","s":"0000000000000001"}"#,
                SignalError::BadReason,
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                Signal::decode(input.as_bytes()).as_ref(),
                Err(expected),
                "{input}"
            );
        }
    }

    #[test]
    fn rejects_too_many_candidates() {
        let c: Vec<String> = (0..=MAX_CANDIDATES)
            .map(|i| format!("\"10.0.0.{i}:1\""))
            .collect();
        let input = format!(
            r#"{{"v":1,"t":"offer","s":"0000000000000001","c":[{}]}}"#,
            c.join(",")
        );
        assert_eq!(
            Signal::decode(input.as_bytes()),
            Err(SignalError::TooManyCandidates)
        );
    }

    #[test]
    fn rejects_impossible_addresses() {
        for addr in [
            "0.0.0.0:1",
            "10.0.0.1:0",
            "224.0.0.1:1",
            "255.255.255.255:1",
            "[::]:1",
            "[ff02::1]:1",
            "[fe80::1]:1",
            "[::ffff:10.0.0.1]:1",
        ] {
            assert_eq!(
                parse_candidate(addr),
                Err(SignalError::BadCandidate),
                "{addr}"
            );
        }
        for addr in [
            "192.168.1.2:5",
            "[2001:db8::1]:5",
            "[fd00::1]:5",
            "127.0.0.1:5",
        ] {
            assert!(parse_candidate(addr).is_ok(), "{addr}");
        }
    }
}
