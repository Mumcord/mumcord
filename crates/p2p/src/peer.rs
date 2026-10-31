//! Relay discovery messages between a relay bot (mumbot `peer`) and Mumcord clients (spec 04).
//!
//! They travel as Mumble plugin data with the id [`PEER_DATA_ID`]; this module only encodes and
//! decodes them, the caller moves the bytes. Small versioned JSON, one message type per `t`:
//! - `relay` (bot → client): where the community relay is and which certificate it presents.
//! - `token-req` (client → bot): asks for a pair token for the client and one peer session.
//! - `token` (bot → both clients of a pair): the token both sides bind at the relay.
//! - `token-err` (bot → requester): why no token was issued.
//!
//! Mumcord and mumbot share this module so both sides use one format. Everything received is
//! untrusted: the decoder checks the size before parsing, ignores unknown fields and validates
//! every field it reads. Tokens are credentials, so [`RelayToken`] never prints its value.

use std::fmt;
use std::net::IpAddr;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use mumcord_protocol::PLUGIN_DATA_MAX;

/// Plugin data id of the relay discovery messages.
pub const PEER_DATA_ID: &str = "mumcord.peer";
/// The message version this side sends. Any version >= 1 is accepted on receipt and unknown
/// fields are ignored; a breaking change gets a new data id.
pub const PEER_VERSION: u32 = 1;
/// Largest message accepted or produced (bytes); standard servers drop bigger plugin data.
pub const MAX_PEER_BYTES: usize = PLUGIN_DATA_MAX;
/// Longest relay host name (a DNS name is at most 253 characters).
const MAX_HOST_LEN: usize = 253;
/// Length of a pair token: 32 random bytes in unpadded base64url.
const TOKEN_LEN: usize = 43;

/// Correlates a `token-req` with its `token` or `token-err` (16 lowercase hex on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

impl RequestId {
    pub fn random() -> Self {
        Self(rand::random())
    }
}

/// Whether the relay bot can currently reach its relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayState {
    Up,
    Down,
}

/// A relay announced by a relay bot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayAnnouncement {
    /// DNS name or IP literal of the relay.
    pub host: String,
    /// QUIC port of the relay.
    pub udp: u16,
    /// TLS-over-TCP port of the relay.
    pub tcp: u16,
    /// SHA-256 of the relay certificate (DER); clients pin the relay to it.
    pub fingerprint: [u8; 32],
    pub state: RelayState,
    /// Increases with every change; an older value than the last seen one is stale.
    pub seq: u32,
}

/// A pair token for the relay. It is a credential: `Debug` and `Display` show `•••`.
#[derive(Clone, PartialEq, Eq)]
pub struct RelayToken(String);

impl RelayToken {
    /// Validates a token string (43 characters of unpadded base64url).
    pub fn parse(s: &str) -> Result<Self, PeerError> {
        let valid = s.len() == TOKEN_LEN
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(PeerError::BadToken)
        }
    }

    /// The token text, for the relay `bind` message only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RelayToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("•••")
    }
}

impl fmt::Display for RelayToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("•••")
    }
}

/// Why the relay bot issued no token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenErrorReason {
    Denied,
    PeerGone,
    PeerNotMumcord,
    RateLimited,
    RelayDown,
    RelayFull,
}

impl TokenErrorReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::PeerGone => "peer-gone",
            Self::PeerNotMumcord => "peer-not-mumcord",
            Self::RateLimited => "rate-limited",
            Self::RelayDown => "relay-down",
            Self::RelayFull => "relay-full",
        }
    }

    fn parse(s: &str) -> Result<Self, PeerError> {
        Ok(match s {
            "denied" => Self::Denied,
            "peer-gone" => Self::PeerGone,
            "peer-not-mumcord" => Self::PeerNotMumcord,
            "rate-limited" => Self::RateLimited,
            "relay-down" => Self::RelayDown,
            "relay-full" => Self::RelayFull,
            _ => return Err(PeerError::BadReason),
        })
    }
}

/// One `mumcord.peer` message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerMessage {
    /// Bot → client: the relay of this server.
    Relay(RelayAnnouncement),
    /// Client → bot: asks for a token for this client and the user with session `peer`.
    TokenRequest { rid: RequestId, peer: u32 },
    /// Bot → both clients: the pair token; `a` requested it, `b` is the peer.
    Token {
        rid: RequestId,
        a: u32,
        b: u32,
        token: RelayToken,
        /// Seconds the token stays valid until both sides bound it.
        ttl: u16,
    },
    /// Bot → requester: no token.
    TokenError {
        rid: RequestId,
        reason: TokenErrorReason,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PeerError {
    #[error("message larger than {MAX_PEER_BYTES} bytes")]
    TooLarge,
    #[error("message is not valid relay discovery JSON")]
    Malformed,
    #[error("unsupported message version {0}")]
    UnsupportedVersion(u32),
    #[error("unknown message type")]
    UnknownType,
    #[error("invalid relay host")]
    BadHost,
    #[error("invalid port")]
    BadPort,
    #[error("invalid relay fingerprint")]
    BadFingerprint,
    #[error("invalid relay state")]
    BadState,
    #[error("invalid request id")]
    BadRequestId,
    #[error("invalid session")]
    BadSession,
    #[error("invalid token")]
    BadToken,
    #[error("unknown token error reason")]
    BadReason,
}

#[derive(Deserialize)]
struct Head {
    v: u32,
    t: String,
}

#[derive(Serialize)]
struct Envelope<'a, B: Serialize> {
    v: u32,
    t: &'a str,
    #[serde(flatten)]
    body: B,
}

#[derive(Serialize, Deserialize)]
struct RelayWire {
    host: String,
    udp: u16,
    tcp: u16,
    fp: String,
    state: String,
    seq: u32,
}

#[derive(Serialize, Deserialize)]
struct TokenRequestWire {
    rid: String,
    peer: u32,
}

#[derive(Serialize, Deserialize)]
struct TokenWire {
    rid: String,
    a: u32,
    b: u32,
    tok: String,
    ttl: u16,
}

#[derive(Serialize, Deserialize)]
struct TokenErrorWire {
    rid: String,
    r: String,
}

impl PeerMessage {
    /// Encodes the message for plugin data. Fails when a field is invalid (for example a host
    /// name the receiver would reject) or the message would not fit.
    pub fn encode(&self) -> Result<Vec<u8>, PeerError> {
        let bytes = match self {
            Self::Relay(relay) => {
                validate_host(&relay.host)?;
                validate_port(relay.udp)?;
                validate_port(relay.tcp)?;
                envelope(
                    "relay",
                    RelayWire {
                        host: relay.host.clone(),
                        udp: relay.udp,
                        tcp: relay.tcp,
                        fp: hex(&relay.fingerprint),
                        state: match relay.state {
                            RelayState::Up => "up",
                            RelayState::Down => "down",
                        }
                        .to_owned(),
                        seq: relay.seq,
                    },
                )?
            }
            Self::TokenRequest { rid, peer } => envelope(
                "token-req",
                TokenRequestWire {
                    rid: rid_str(*rid),
                    peer: *peer,
                },
            )?,
            Self::Token {
                rid,
                a,
                b,
                token,
                ttl,
            } => {
                if a == b {
                    return Err(PeerError::BadSession);
                }
                envelope(
                    "token",
                    TokenWire {
                        rid: rid_str(*rid),
                        a: *a,
                        b: *b,
                        tok: token.0.clone(),
                        ttl: *ttl,
                    },
                )?
            }
            Self::TokenError { rid, reason } => envelope(
                "token-err",
                TokenErrorWire {
                    rid: rid_str(*rid),
                    r: reason.as_str().to_owned(),
                },
            )?,
        };
        if bytes.len() > MAX_PEER_BYTES {
            return Err(PeerError::TooLarge);
        }
        Ok(bytes)
    }

    /// Decodes a received message; anything unexpected is an error, never a panic. Callers
    /// ignore [`PeerError::UnknownType`] (a newer sender may add types).
    pub fn decode(bytes: &[u8]) -> Result<Self, PeerError> {
        if bytes.len() > MAX_PEER_BYTES {
            return Err(PeerError::TooLarge);
        }
        let head: Head = parse(bytes)?;
        if head.v < PEER_VERSION {
            return Err(PeerError::UnsupportedVersion(head.v));
        }
        match head.t.as_str() {
            "relay" => {
                let wire: RelayWire = parse(bytes)?;
                validate_host(&wire.host)?;
                validate_port(wire.udp)?;
                validate_port(wire.tcp)?;
                let state = match wire.state.as_str() {
                    "up" => RelayState::Up,
                    "down" => RelayState::Down,
                    _ => return Err(PeerError::BadState),
                };
                Ok(Self::Relay(RelayAnnouncement {
                    host: wire.host,
                    udp: wire.udp,
                    tcp: wire.tcp,
                    fingerprint: parse_fingerprint(&wire.fp)?,
                    state,
                    seq: wire.seq,
                }))
            }
            "token-req" => {
                let wire: TokenRequestWire = parse(bytes)?;
                Ok(Self::TokenRequest {
                    rid: parse_rid(&wire.rid)?,
                    peer: wire.peer,
                })
            }
            "token" => {
                let wire: TokenWire = parse(bytes)?;
                if wire.a == wire.b {
                    return Err(PeerError::BadSession);
                }
                Ok(Self::Token {
                    rid: parse_rid(&wire.rid)?,
                    a: wire.a,
                    b: wire.b,
                    token: RelayToken::parse(&wire.tok)?,
                    ttl: wire.ttl,
                })
            }
            "token-err" => {
                let wire: TokenErrorWire = parse(bytes)?;
                Ok(Self::TokenError {
                    rid: parse_rid(&wire.rid)?,
                    reason: TokenErrorReason::parse(&wire.r)?,
                })
            }
            _ => Err(PeerError::UnknownType),
        }
    }
}

fn envelope<B: Serialize>(t: &str, body: B) -> Result<Vec<u8>, PeerError> {
    serde_json::to_vec(&Envelope {
        v: PEER_VERSION,
        t,
        body,
    })
    .map_err(|_| PeerError::Malformed)
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, PeerError> {
    serde_json::from_slice(bytes).map_err(|_| PeerError::Malformed)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn rid_str(rid: RequestId) -> String {
    format!("{:016x}", rid.0)
}

fn parse_rid(s: &str) -> Result<RequestId, PeerError> {
    if !is_lower_hex(s, 16) {
        return Err(PeerError::BadRequestId);
    }
    u64::from_str_radix(s, 16)
        .map(RequestId)
        .map_err(|_| PeerError::BadRequestId)
}

fn parse_fingerprint(s: &str) -> Result<[u8; 32], PeerError> {
    if !is_lower_hex(s, 64) {
        return Err(PeerError::BadFingerprint);
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| PeerError::BadFingerprint)?;
    }
    Ok(out)
}

fn validate_port(port: u16) -> Result<(), PeerError> {
    if port == 0 {
        Err(PeerError::BadPort)
    } else {
        Ok(())
    }
}

/// A relay host is an IP literal a client can dial, or a DNS name of LDH labels. Private and
/// loopback addresses are allowed so local test setups work.
fn validate_host(host: &str) -> Result<(), PeerError> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        let unusable = match ip {
            IpAddr::V4(v4) => v4.is_unspecified() || v4.is_multicast() || v4.is_broadcast(),
            IpAddr::V6(v6) => v6.is_unspecified() || v6.is_multicast(),
        };
        return if unusable {
            Err(PeerError::BadHost)
        } else {
            Ok(())
        };
    }
    let name = host.strip_suffix('.').unwrap_or(host);
    let valid = !name.is_empty()
        && host.len() <= MAX_HOST_LEN
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(PeerError::BadHost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA_";
    fn fp() -> String {
        format!("3f{}a9", "0".repeat(60))
    }

    fn relay() -> PeerMessage {
        PeerMessage::Relay(RelayAnnouncement {
            host: "relay.example.org".to_owned(),
            udp: 443,
            tcp: 443,
            fingerprint: [0xab; 32],
            state: RelayState::Up,
            seq: 4,
        })
    }

    fn all() -> Vec<PeerMessage> {
        vec![
            relay(),
            PeerMessage::TokenRequest {
                rid: RequestId(0x0123_4567_89ab_cdef),
                peer: 17,
            },
            PeerMessage::Token {
                rid: RequestId(1),
                a: 3,
                b: u32::MAX,
                token: RelayToken::parse(TOKEN).expect("token"),
                ttl: 120,
            },
            PeerMessage::TokenError {
                rid: RequestId(u64::MAX),
                reason: TokenErrorReason::PeerNotMumcord,
            },
        ]
    }

    #[test]
    fn round_trips_every_type() {
        for message in all() {
            let bytes = message.encode().expect("encode");
            assert!(bytes.len() <= MAX_PEER_BYTES);
            assert_eq!(PeerMessage::decode(&bytes).expect("decode"), message);
        }
    }

    #[test]
    fn decodes_the_spec_example_shape() {
        let fp = fp();
        let json = format!(
            r#"{{"v":1,"t":"relay","host":"relay.example.org","udp":443,"tcp":8443,"fp":"{fp}","state":"down","seq":4}}"#
        );
        let PeerMessage::Relay(relay) = PeerMessage::decode(json.as_bytes()).expect("decode")
        else {
            panic!("not a relay message");
        };
        assert_eq!(relay.tcp, 8443);
        assert_eq!(relay.state, RelayState::Down);
        assert_eq!(relay.fingerprint[0], 0x3f);
        assert_eq!(hex(&relay.fingerprint), fp);
    }

    #[test]
    fn encodes_wire_field_names() {
        let json = String::from_utf8(all()[2].encode().expect("encode")).expect("utf8");
        assert_eq!(
            json,
            format!(
                r#"{{"v":1,"t":"token","rid":"0000000000000001","a":3,"b":4294967295,"tok":"{TOKEN}","ttl":120}}"#
            )
        );
    }

    #[test]
    fn largest_relay_announcement_fits() {
        let host = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(host.len(), MAX_HOST_LEN);
        let message = PeerMessage::Relay(RelayAnnouncement {
            host,
            udp: u16::MAX,
            tcp: u16::MAX,
            fingerprint: [0xff; 32],
            state: RelayState::Down,
            seq: u32::MAX,
        });
        let bytes = message.encode().expect("encode");
        assert_eq!(PeerMessage::decode(&bytes).expect("decode"), message);
    }

    #[test]
    fn newer_version_with_extra_fields_decodes() {
        let json = r#"{"v":3,"t":"token-req","rid":"00000000000000ff","peer":9,"extra":[1,2]}"#;
        assert_eq!(
            PeerMessage::decode(json.as_bytes()),
            Ok(PeerMessage::TokenRequest {
                rid: RequestId(0xff),
                peer: 9
            })
        );
    }

    #[test]
    fn rejects_oversized_before_parsing() {
        let mut bytes = relay().encode().expect("encode");
        bytes.resize(MAX_PEER_BYTES + 1, b' ');
        assert_eq!(PeerMessage::decode(&bytes), Err(PeerError::TooLarge));
    }

    #[test]
    fn token_is_masked_in_debug_and_display() {
        let message = &all()[2];
        let debug = format!("{message:?}");
        assert!(!debug.contains(TOKEN), "{debug}");
        assert!(debug.contains("•••"));
        let token = RelayToken::parse(TOKEN).expect("token");
        assert_eq!(token.to_string(), "•••");
        assert_eq!(token.expose(), TOKEN);
    }

    #[test]
    fn rejects_malformed_input() {
        let fp = fp();
        let relay = |host: &str, udp: u32, fp: &str, state: &str| {
            format!(
                r#"{{"v":1,"t":"relay","host":"{host}","udp":{udp},"tcp":443,"fp":"{fp}","state":"{state}","seq":1}}"#
            )
        };
        let cases: Vec<(String, PeerError)> = vec![
            (String::new(), PeerError::Malformed),
            ("[1]".into(), PeerError::Malformed),
            (r#"{"v":1}"#.into(), PeerError::Malformed),
            (r#"{"v":0,"t":"relay"}"#.into(), PeerError::UnsupportedVersion(0)),
            (r#"{"v":1,"t":"hello"}"#.into(), PeerError::UnknownType),
            (r#"{"v":1,"t":"relay","host":"x"}"#.into(), PeerError::Malformed),
            (relay("0.0.0.0", 443, &fp, "up"), PeerError::BadHost),
            (relay("224.0.0.1", 443, &fp, "up"), PeerError::BadHost),
            (relay("255.255.255.255", 443, &fp, "up"), PeerError::BadHost),
            (relay("::", 443, &fp, "up"), PeerError::BadHost),
            (relay("ff02::1", 443, &fp, "up"), PeerError::BadHost),
            (relay("-bad.example", 443, &fp, "up"), PeerError::BadHost),
            (relay("a..b", 443, &fp, "up"), PeerError::BadHost),
            (relay("a_b.example", 443, &fp, "up"), PeerError::BadHost),
            (relay("", 443, &fp, "up"), PeerError::BadHost),
            (relay(&"a".repeat(64), 443, &fp, "up"), PeerError::BadHost),
            (relay("relay.example", 0, &fp, "up"), PeerError::BadPort),
            (relay("relay.example", 70000, &fp, "up"), PeerError::Malformed),
            (relay("relay.example", 443, &fp[1..], "up"), PeerError::BadFingerprint),
            (relay("relay.example", 443, &fp.to_uppercase(), "up"), PeerError::BadFingerprint),
            (relay("relay.example", 443, &fp, "sideways"), PeerError::BadState),
            (
                r#"{"v":1,"t":"token-req","rid":"1","peer":2}"#.into(),
                PeerError::BadRequestId,
            ),
            (
                r#"{"v":1,"t":"token-req","rid":"000000000000000A","peer":2}"#.into(),
                PeerError::BadRequestId,
            ),
            (
                r#"{"v":1,"t":"token-req","rid":"0000000000000001","peer":-2}"#.into(),
                PeerError::Malformed,
            ),
            (
                r#"{"v":1,"t":"token","rid":"0000000000000001","a":1,"b":2,"tok":"short","ttl":120}"#
                    .into(),
                PeerError::BadToken,
            ),
            (
                format!(r#"{{"v":1,"t":"token","rid":"0000000000000001","a":1,"b":1,"tok":"{TOKEN}","ttl":120}}"#),
                PeerError::BadSession,
            ),
            (
                r#"{"v":1,"t":"token-err","rid":"0000000000000001","r":"nope"}"#.into(),
                PeerError::BadReason,
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                PeerMessage::decode(input.as_bytes()).as_ref(),
                Err(&expected),
                "{input}"
            );
        }
    }

    #[test]
    fn accepts_hosts_for_local_setups() {
        for host in [
            "127.0.0.1",
            "192.168.1.5",
            "::1",
            "fd00::1",
            "relay",
            "relay.example.org.",
        ] {
            assert_eq!(validate_host(host), Ok(()), "{host}");
        }
    }

    #[test]
    fn token_rejects_wrong_alphabet() {
        assert_eq!(
            RelayToken::parse(&TOKEN.replace('_', "+")),
            Err(PeerError::BadToken)
        );
        assert_eq!(
            RelayToken::parse(&TOKEN.replace('_', "=")),
            Err(PeerError::BadToken)
        );
    }

    #[test]
    fn encode_rejects_invalid_fields() {
        let mut message = relay();
        if let PeerMessage::Relay(relay) = &mut message {
            relay.host = "bad host".to_owned();
        }
        assert_eq!(message.encode(), Err(PeerError::BadHost));
    }
}
