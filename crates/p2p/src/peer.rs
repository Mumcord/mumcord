//! Relay discovery messages between a relay bot (mumbot `peer`) and Mumcord clients (spec 04).
//!
//! They travel as Mumble plugin data with the id [`PEER_DATA_ID`]; this module only encodes and
//! decodes them, the caller moves the bytes. Small versioned JSON, one message type per `t`:
//! - `relay` (bot → client): where the community relay is and which certificate it presents.
//! - `token-req` (client → bot): asks for a pair token for the client and one peer session.
//! - `token` (bot → both clients of a pair): the token both sides bind at the relay.
//! - `token-err` (bot → requester): why no token was issued.
//! - `bcast-req`, `bcast`, `bcast-admit`, `bcast-tok`, `bcast-kick`, `bcast-err`: a screen-share
//!   broadcast through the relay (spec 07, section 6.6); the relay link itself is in
//!   [`crate::broadcast`].
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
/// The `mv` value of a relay or client that speaks the Bovnar media profile (FR-7.96).
pub const MEDIA_VERSION: u8 = 2;

/// The hello feature of clients that can send and show cameras (spec 08). Hello features live in
/// the engine's hello; this is the wire name it announces.
pub const CAMERA_FEATURE: &str = "camera";
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

/// A broadcast at the relay (16 lowercase hex on the wire), assigned by the relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BroadcastId(pub u64);

/// What a relay connection is used for (spec 07, FR-7.1). Wire names `img`, `file`, `scr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelayUse {
    /// Full-size image downloads.
    Images,
    /// File transfers.
    Files,
    /// Screen sharing (one copy per viewer and broadcasts).
    ScreenShare,
    /// Plugin data between Mumcord clients (spec 09). Wire `plg`; mumpeer calls it `plugin_data`.
    PluginData,
    /// Camera sharing (spec 08). Wire `cam`; mumpeer calls it `camera`.
    Camera,
}

impl RelayUse {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Images => "img",
            Self::Files => "file",
            Self::ScreenShare => "scr",
            Self::PluginData => "plg",
            Self::Camera => "cam",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "img" => Self::Images,
            "file" => Self::Files,
            "scr" => Self::ScreenShare,
            "plg" => Self::PluginData,
            "cam" => Self::Camera,
            _ => return None,
        })
    }
}

/// What a relay broadcast carries (wire `k` on `bcast-tok`). A sender can run one broadcast of
/// each kind at once, so a viewer needs the kind to hand a token to the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BroadcastKind {
    /// Screen sharing (spec 07), wire `scr`; also what a bot that sends no `k` means.
    ScreenShare,
    /// Plugin data (spec 09), wire `plg`.
    PluginData,
    /// Camera sharing (spec 08), wire `cam`.
    Camera,
}

impl BroadcastKind {
    pub fn as_str(self) -> &'static str {
        self.relay_use().as_str()
    }

    /// The relay use the broadcast counts against.
    pub fn relay_use(self) -> RelayUse {
        match self {
            Self::ScreenShare => RelayUse::ScreenShare,
            Self::PluginData => RelayUse::PluginData,
            Self::Camera => RelayUse::Camera,
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match RelayUse::parse(s)? {
            RelayUse::ScreenShare => Self::ScreenShare,
            RelayUse::PluginData => Self::PluginData,
            RelayUse::Camera => Self::Camera,
            RelayUse::Images | RelayUse::Files => return None,
        })
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
    /// Uses the relay allows (wire `u`). `None` (an older bot) means all but plugin data are
    /// allowed (plugin data is off unless announced, FR-9.84). Use names
    /// this version doesn't know are dropped on receipt.
    pub uses: Option<Vec<RelayUse>>,
    /// Most viewers per broadcast (wire `bc`); `None` means the relay offers no broadcasts.
    pub broadcast: Option<u16>,
    /// Most kilobits per second one sender may send as plugin data (wire `pk`); `None` means the
    /// relay announces no limit (an older bot, or plugin data is off).
    pub plugin_data_kbps: Option<u32>,
    /// The relay only serves registered users (wire `reg`, #601), so a guest's client can say so
    /// before trying. `false` when not announced.
    pub registered_only: bool,
    /// The relay speaks the Bovnar media profile (wire `mv >= MEDIA_VERSION`, FR-7.96).
    pub media: bool,
}

impl RelayAnnouncement {
    /// Whether the relay may be used for `relay_use`. An announcement without `u` comes from a
    /// bot older than plugin data: it allows every use except [`RelayUse::PluginData`].
    pub fn allows(&self, relay_use: RelayUse) -> bool {
        match &self.uses {
            Some(uses) => uses.contains(&relay_use),
            None => relay_use != RelayUse::PluginData,
        }
    }
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
    /// The relay does not allow the requested use (spec 07, FR-7.5).
    UseNotAllowed,
    /// The relay only serves registered users and the requester is a guest (#406).
    NotRegistered,
    /// The relay only serves members of certain groups, and the requester is in none (#601).
    GroupNotAllowed,
    /// The bot has not seen this client's hello (for example after the bot restarted); the
    /// client says hello again by reconnecting (#601).
    NoHello,
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
            Self::UseNotAllowed => "use-not-allowed",
            Self::NotRegistered => "not-registered",
            Self::GroupNotAllowed => "group-not-allowed",
            Self::NoHello => "no-hello",
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
            "use-not-allowed" => Self::UseNotAllowed,
            "not-registered" => Self::NotRegistered,
            "group-not-allowed" => Self::GroupNotAllowed,
            "no-hello" => Self::NoHello,
            // A reason a newer bot names is still a refusal (#601).
            _ => Self::Denied,
        })
    }
}

/// Why the relay bot set up no broadcast or admitted no viewer (`bcast-err`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BroadcastErrorReason {
    Denied,
    /// Sharer and viewer are not in the same channel.
    NotSameChannel,
    /// The broadcast has no room for another viewer.
    Full,
    RelayDown,
    /// The relay runs as many broadcasts as it allows.
    RelayFull,
    /// The relay does not allow screen sharing.
    UseNotAllowed,
    RateLimited,
    /// The sender's channel already has as many camera broadcasts as the bot allows (only for
    /// `k = cam`).
    RoomFull,
    /// The relay only serves registered users and the sender or viewer is a guest (#601).
    NotRegistered,
    /// The relay only serves members of certain groups (#601).
    GroupNotAllowed,
    /// The sender is in no channel the bot can see (#601).
    NotInChannel,
    /// The bot has not seen this client's hello; reconnecting sends it again (#601).
    NoHello,
}

impl BroadcastErrorReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::NotSameChannel => "not-same-channel",
            Self::Full => "full",
            Self::RelayDown => "relay-down",
            Self::RelayFull => "relay-full",
            Self::UseNotAllowed => "use-not-allowed",
            Self::RateLimited => "rate-limited",
            Self::RoomFull => "room-full",
            Self::NotRegistered => "not-registered",
            Self::GroupNotAllowed => "group-not-allowed",
            Self::NotInChannel => "not-in-channel",
            Self::NoHello => "no-hello",
        }
    }

    fn parse(s: &str) -> Result<Self, PeerError> {
        Ok(match s {
            "denied" => Self::Denied,
            "not-same-channel" => Self::NotSameChannel,
            "full" => Self::Full,
            "relay-down" => Self::RelayDown,
            "relay-full" => Self::RelayFull,
            "use-not-allowed" => Self::UseNotAllowed,
            "rate-limited" => Self::RateLimited,
            "room-full" => Self::RoomFull,
            "not-registered" => Self::NotRegistered,
            "group-not-allowed" => Self::GroupNotAllowed,
            "not-in-channel" => Self::NotInChannel,
            "no-hello" => Self::NoHello,
            // A reason a newer bot names is still a refusal (#601).
            _ => Self::Denied,
        })
    }
}

/// One `mumcord.peer` message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerMessage {
    /// Bot → client: the relay of this server.
    Relay(RelayAnnouncement),
    /// Client → bot: asks for a token for this client and the user with session `peer`.
    TokenRequest {
        rid: RequestId,
        peer: u32,
        /// What the pair will use the relay for (wire `use`); the bot treats `None` (an older
        /// client) as screen sharing.
        relay_use: Option<RelayUse>,
    },
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
    /// Sharer → bot: asks for a broadcast for up to `max` viewers (`bcast-req`). `kind` (wire
    /// `k`) says what the broadcast carries: `scr` or `plg`. `None` means `scr`, which keeps
    /// bots from before plugin data working.
    BroadcastRequest {
        rid: RequestId,
        max: u16,
        kind: Option<RelayUse>,
        /// The publisher will use the Bovnar media profile (wire `mv`, FR-7.96).
        media: bool,
    },
    /// Bot → sharer: the broadcast and its publisher token (`bcast`).
    Broadcast {
        rid: RequestId,
        id: BroadcastId,
        token: RelayToken,
        /// Seconds the token stays valid until it is bound.
        ttl: u16,
        /// The broadcast's format, repeated from `bcast-req` (wire `mv`, FR-7.96).
        media: bool,
    },
    /// Sharer → bot: admit the user with session `viewer` (`bcast-admit`).
    BroadcastAdmit {
        rid: RequestId,
        id: BroadcastId,
        viewer: u32,
    },
    /// Bot → viewer: the subscriber token for the `kind` broadcast of session `sharer`
    /// (`bcast-tok`). Bots from before wire `k` send none, which means screen sharing.
    BroadcastToken {
        id: BroadcastId,
        token: RelayToken,
        ttl: u16,
        sharer: u32,
        kind: BroadcastKind,
        /// The broadcast's format, from the publisher's `mv` (FR-7.96).
        media: bool,
    },
    /// Sharer → bot: remove the user with session `viewer` from the broadcast (`bcast-kick`).
    BroadcastKick { id: BroadcastId, viewer: u32 },
    /// Bot → requester: no broadcast or no admission (`bcast-err`).
    BroadcastError {
        rid: RequestId,
        reason: BroadcastErrorReason,
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
    /// No longer produced: an unknown reason now reads as `denied` (#601). Kept so code that
    /// matches on it still builds.
    #[error("unknown error reason")]
    BadReason,
    #[error("invalid broadcast id")]
    BadBroadcastId,
    #[error("unknown relay use")]
    BadUse,
    #[error("invalid viewer limit")]
    BadViewers,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    u: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bc: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pk: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    reg: bool,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    mv: u8,
}

#[derive(Serialize, Deserialize)]
struct TokenRequestWire {
    rid: String,
    peer: u32,
    #[serde(rename = "use", default, skip_serializing_if = "Option::is_none")]
    relay_use: Option<String>,
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
struct ErrorWire {
    rid: String,
    r: String,
}

#[derive(Serialize, Deserialize)]
struct BroadcastRequestWire {
    rid: String,
    max: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    k: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    mv: u8,
}

#[derive(Serialize, Deserialize)]
struct BroadcastWire {
    rid: String,
    id: String,
    tok: String,
    ttl: u16,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    mv: u8,
}

#[derive(Serialize, Deserialize)]
struct BroadcastAdmitWire {
    rid: String,
    id: String,
    viewer: u32,
}

#[derive(Serialize, Deserialize)]
struct BroadcastTokenWire {
    id: String,
    tok: String,
    ttl: u16,
    sharer: u32,
    #[serde(default)]
    k: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    mv: u8,
}

#[derive(Serialize, Deserialize)]
struct BroadcastKickWire {
    id: String,
    viewer: u32,
}

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

fn media_version(media: bool) -> u8 {
    if media { MEDIA_VERSION } else { 0 }
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
                if relay.broadcast == Some(0) {
                    return Err(PeerError::BadViewers);
                }
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
                        u: relay
                            .uses
                            .as_ref()
                            .map(|uses| uses.iter().map(|u| u.as_str().to_owned()).collect()),
                        bc: relay.broadcast,
                        pk: relay.plugin_data_kbps,
                        reg: relay.registered_only,
                        mv: media_version(relay.media),
                    },
                )?
            }
            Self::TokenRequest {
                rid,
                peer,
                relay_use,
            } => envelope(
                "token-req",
                TokenRequestWire {
                    rid: hex16(rid.0),
                    peer: *peer,
                    relay_use: relay_use.map(|u| u.as_str().to_owned()),
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
                        rid: hex16(rid.0),
                        a: *a,
                        b: *b,
                        tok: token.0.clone(),
                        ttl: *ttl,
                    },
                )?
            }
            Self::TokenError { rid, reason } => envelope(
                "token-err",
                ErrorWire {
                    rid: hex16(rid.0),
                    r: reason.as_str().to_owned(),
                },
            )?,
            Self::BroadcastRequest {
                rid,
                max,
                kind,
                media,
            } => {
                if *max == 0 {
                    return Err(PeerError::BadViewers);
                }
                if matches!(kind, Some(RelayUse::Images | RelayUse::Files)) {
                    return Err(PeerError::BadUse);
                }
                envelope(
                    "bcast-req",
                    BroadcastRequestWire {
                        rid: hex16(rid.0),
                        max: *max,
                        k: kind.map(|k| k.as_str().to_owned()),
                        mv: media_version(*media),
                    },
                )?
            }
            Self::Broadcast {
                rid,
                id,
                token,
                ttl,
                media,
            } => envelope(
                "bcast",
                BroadcastWire {
                    rid: hex16(rid.0),
                    id: hex16(id.0),
                    tok: token.0.clone(),
                    ttl: *ttl,
                    mv: media_version(*media),
                },
            )?,
            Self::BroadcastAdmit { rid, id, viewer } => envelope(
                "bcast-admit",
                BroadcastAdmitWire {
                    rid: hex16(rid.0),
                    id: hex16(id.0),
                    viewer: *viewer,
                },
            )?,
            Self::BroadcastToken {
                id,
                token,
                ttl,
                sharer,
                kind,
                media,
            } => envelope(
                "bcast-tok",
                BroadcastTokenWire {
                    id: hex16(id.0),
                    tok: token.0.clone(),
                    ttl: *ttl,
                    sharer: *sharer,
                    k: Some(kind.as_str().to_owned()),
                    mv: media_version(*media),
                },
            )?,
            Self::BroadcastKick { id, viewer } => envelope(
                "bcast-kick",
                BroadcastKickWire {
                    id: hex16(id.0),
                    viewer: *viewer,
                },
            )?,
            Self::BroadcastError { rid, reason } => envelope(
                "bcast-err",
                ErrorWire {
                    rid: hex16(rid.0),
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
                    uses: wire
                        .u
                        .map(|uses| uses.iter().filter_map(|u| RelayUse::parse(u)).collect()),
                    // A limit of zero viewers is no broadcast at all.
                    broadcast: wire.bc.filter(|&max| max > 0),
                    // A limit of zero kbit/s would allow nothing; treat it as not announced.
                    plugin_data_kbps: wire.pk.filter(|&kbps| kbps > 0),
                    registered_only: wire.reg,
                    media: wire.mv >= MEDIA_VERSION,
                }))
            }
            "token-req" => {
                let wire: TokenRequestWire = parse(bytes)?;
                Ok(Self::TokenRequest {
                    rid: parse_rid(&wire.rid)?,
                    peer: wire.peer,
                    relay_use: wire
                        .relay_use
                        .map(|u| RelayUse::parse(&u).ok_or(PeerError::BadUse))
                        .transpose()?,
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
                let wire: ErrorWire = parse(bytes)?;
                Ok(Self::TokenError {
                    rid: parse_rid(&wire.rid)?,
                    reason: TokenErrorReason::parse(&wire.r)?,
                })
            }
            "bcast-req" => {
                let wire: BroadcastRequestWire = parse(bytes)?;
                if wire.max == 0 {
                    return Err(PeerError::BadViewers);
                }
                let kind = wire
                    .k
                    .map(|k| match RelayUse::parse(&k) {
                        Some(
                            kind
                            @ (RelayUse::ScreenShare | RelayUse::PluginData | RelayUse::Camera),
                        ) => Ok(kind),
                        _ => Err(PeerError::BadUse),
                    })
                    .transpose()?;
                Ok(Self::BroadcastRequest {
                    rid: parse_rid(&wire.rid)?,
                    max: wire.max,
                    kind,
                    media: wire.mv >= MEDIA_VERSION,
                })
            }
            "bcast" => {
                let wire: BroadcastWire = parse(bytes)?;
                Ok(Self::Broadcast {
                    rid: parse_rid(&wire.rid)?,
                    id: parse_broadcast_id(&wire.id)?,
                    token: RelayToken::parse(&wire.tok)?,
                    ttl: wire.ttl,
                    media: wire.mv >= MEDIA_VERSION,
                })
            }
            "bcast-admit" => {
                let wire: BroadcastAdmitWire = parse(bytes)?;
                Ok(Self::BroadcastAdmit {
                    rid: parse_rid(&wire.rid)?,
                    id: parse_broadcast_id(&wire.id)?,
                    viewer: wire.viewer,
                })
            }
            "bcast-tok" => {
                let wire: BroadcastTokenWire = parse(bytes)?;
                Ok(Self::BroadcastToken {
                    id: parse_broadcast_id(&wire.id)?,
                    token: RelayToken::parse(&wire.tok)?,
                    ttl: wire.ttl,
                    sharer: wire.sharer,
                    kind: wire
                        .k
                        .map(|k| BroadcastKind::parse(&k).ok_or(PeerError::BadUse))
                        .transpose()?
                        .unwrap_or(BroadcastKind::ScreenShare),
                    media: wire.mv >= MEDIA_VERSION,
                })
            }
            "bcast-kick" => {
                let wire: BroadcastKickWire = parse(bytes)?;
                Ok(Self::BroadcastKick {
                    id: parse_broadcast_id(&wire.id)?,
                    viewer: wire.viewer,
                })
            }
            "bcast-err" => {
                let wire: ErrorWire = parse(bytes)?;
                Ok(Self::BroadcastError {
                    rid: parse_rid(&wire.rid)?,
                    reason: BroadcastErrorReason::parse(&wire.r)?,
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

fn hex16(value: u64) -> String {
    format!("{value:016x}")
}

fn parse_hex16(s: &str) -> Option<u64> {
    if !is_lower_hex(s, 16) {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}

fn parse_rid(s: &str) -> Result<RequestId, PeerError> {
    parse_hex16(s).map(RequestId).ok_or(PeerError::BadRequestId)
}

fn parse_broadcast_id(s: &str) -> Result<BroadcastId, PeerError> {
    parse_hex16(s)
        .map(BroadcastId)
        .ok_or(PeerError::BadBroadcastId)
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
            uses: None,
            broadcast: None,
            plugin_data_kbps: None,
            registered_only: false,
            media: false,
        })
    }

    fn all() -> Vec<PeerMessage> {
        vec![
            relay(),
            PeerMessage::TokenRequest {
                rid: RequestId(0x0123_4567_89ab_cdef),
                peer: 17,
                relay_use: None,
            },
            PeerMessage::TokenRequest {
                rid: RequestId(2),
                peer: 18,
                relay_use: Some(RelayUse::Files),
            },
            PeerMessage::Relay(RelayAnnouncement {
                host: "127.0.0.1".to_owned(),
                udp: 44759,
                tcp: 44759,
                fingerprint: [1; 32],
                state: RelayState::Up,
                seq: 1,
                uses: Some(vec![RelayUse::Images, RelayUse::ScreenShare]),
                broadcast: Some(25),
                plugin_data_kbps: Some(512),
                registered_only: false,
                media: false,
            }),
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
            PeerMessage::TokenError {
                rid: RequestId(3),
                reason: TokenErrorReason::UseNotAllowed,
            },
            PeerMessage::BroadcastRequest {
                rid: RequestId(4),
                max: 25,
                kind: None,
                media: false,
            },
            PeerMessage::BroadcastRequest {
                rid: RequestId(6),
                max: 8,
                kind: Some(RelayUse::PluginData),
                media: false,
            },
            PeerMessage::BroadcastRequest {
                rid: RequestId(7),
                max: 8,
                kind: Some(RelayUse::Camera),
                media: false,
            },
            PeerMessage::Broadcast {
                rid: RequestId(4),
                id: BroadcastId(0xfedc_ba98_7654_3210),
                token: RelayToken::parse(TOKEN).expect("token"),
                ttl: 60,
                media: false,
            },
            PeerMessage::BroadcastAdmit {
                rid: RequestId(5),
                id: BroadcastId(1),
                viewer: 9,
            },
            PeerMessage::BroadcastToken {
                id: BroadcastId(1),
                token: RelayToken::parse(TOKEN).expect("token"),
                ttl: 60,
                sharer: 8,
                kind: BroadcastKind::Camera,
                media: false,
            },
            PeerMessage::BroadcastKick {
                id: BroadcastId(1),
                viewer: 9,
            },
            PeerMessage::BroadcastError {
                rid: RequestId(5),
                reason: BroadcastErrorReason::NotSameChannel,
            },
            PeerMessage::BroadcastError {
                rid: RequestId(8),
                reason: BroadcastErrorReason::RoomFull,
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
        let json = String::from_utf8(all()[4].encode().expect("encode")).expect("utf8");
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
            uses: Some(vec![
                RelayUse::Images,
                RelayUse::Files,
                RelayUse::ScreenShare,
                RelayUse::PluginData,
                RelayUse::Camera,
            ]),
            broadcast: Some(u16::MAX),
            plugin_data_kbps: Some(u32::MAX),
            registered_only: false,
            media: false,
        });
        let bytes = message.encode().expect("encode");
        assert_eq!(PeerMessage::decode(&bytes).expect("decode"), message);
    }

    #[test]
    fn camera_wire_names_and_old_bot_compatibility() {
        let req = PeerMessage::decode(
            br#"{"v":1,"t":"bcast-req","rid":"0000000000000001","max":9,"k":"cam"}"#,
        )
        .expect("decode");
        assert_eq!(
            req,
            PeerMessage::BroadcastRequest {
                rid: RequestId(1),
                max: 9,
                kind: Some(RelayUse::Camera),
                media: false,
            }
        );
        let err = PeerMessage::decode(
            br#"{"v":1,"t":"bcast-err","rid":"0000000000000001","r":"room-full"}"#,
        )
        .expect("decode");
        assert_eq!(
            err,
            PeerMessage::BroadcastError {
                rid: RequestId(1),
                reason: BroadcastErrorReason::RoomFull,
            }
        );
        // A bot without camera support announces no `cam`: it is simply not in the list, and an
        // unknown use from a newer bot is dropped rather than failing the announcement.
        let relay = PeerMessage::decode(
            br#"{"v":1,"t":"relay","host":"r.example","udp":1,"tcp":2,"fp":"0000000000000000000000000000000000000000000000000000000000000000","st":"up","seq":1,"u":["scr","cam","zzz"],"bc":9}"#,
        );
        if let Ok(PeerMessage::Relay(a)) = relay {
            assert!(a.allows(RelayUse::Camera));
            assert!(a.allows(RelayUse::ScreenShare));
        }
        assert_eq!(CAMERA_FEATURE, "camera");
    }

    #[test]
    fn newer_version_with_extra_fields_decodes() {
        let json = r#"{"v":3,"t":"token-req","rid":"00000000000000ff","peer":9,"extra":[1,2]}"#;
        assert_eq!(
            PeerMessage::decode(json.as_bytes()),
            Ok(PeerMessage::TokenRequest {
                rid: RequestId(0xff),
                peer: 9,
                relay_use: None,
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
        let message = &all()[4];
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
                r#"{"v":1,"t":"token-req","rid":"0000000000000001","peer":2,"use":"music"}"#
                    .into(),
                PeerError::BadUse,
            ),
            (
                r#"{"v":1,"t":"bcast-req","rid":"0000000000000001","max":0}"#.into(),
                PeerError::BadViewers,
            ),
            (
                r#"{"v":1,"t":"bcast-req","rid":"0000000000000001","max":70000}"#.into(),
                PeerError::Malformed,
            ),
            (
                format!(r#"{{"v":1,"t":"bcast","rid":"0000000000000001","id":"00ff","tok":"{TOKEN}","ttl":60}}"#),
                PeerError::BadBroadcastId,
            ),
            (
                r#"{"v":1,"t":"bcast-tok","id":"0000000000000001","tok":"short","ttl":60,"sharer":1}"#
                    .into(),
                PeerError::BadToken,
            ),
            (
                format!(r#"{{"v":1,"t":"bcast-tok","id":"0000000000000001","tok":"{TOKEN}","ttl":60,"sharer":1,"k":"file"}}"#),
                PeerError::BadUse,
            ),
            (
                format!(r#"{{"v":1,"t":"bcast-tok","id":"0000000000000001","tok":"{TOKEN}","ttl":60,"sharer":1,"k":"vid"}}"#),
                PeerError::BadUse,
            ),
            (
                r#"{"v":1,"t":"bcast-kick","id":"000000000000000G","viewer":1}"#.into(),
                PeerError::BadBroadcastId,
            ),
            (
                r#"{"v":1,"t":"bcast-admit","rid":"0000000000000001","id":"0000000000000001"}"#
                    .into(),
                PeerError::Malformed,
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
        let mut message = relay();
        if let PeerMessage::Relay(relay) = &mut message {
            relay.broadcast = Some(0);
        }
        assert_eq!(message.encode(), Err(PeerError::BadViewers));
        let request = PeerMessage::BroadcastRequest {
            rid: RequestId(1),
            max: 0,
            kind: None,
            media: false,
        };
        assert_eq!(request.encode(), Err(PeerError::BadViewers));
    }

    #[test]
    fn encodes_spec_07_wire_field_names() {
        let encode = |message: PeerMessage| {
            String::from_utf8(message.encode().expect("encode")).expect("utf8")
        };
        assert_eq!(
            encode(PeerMessage::TokenRequest {
                rid: RequestId(1),
                peer: 2,
                relay_use: Some(RelayUse::Images),
            }),
            r#"{"v":1,"t":"token-req","rid":"0000000000000001","peer":2,"use":"img"}"#
        );
        // Without a use the field is left out, exactly as older clients send it.
        assert_eq!(
            encode(PeerMessage::TokenRequest {
                rid: RequestId(1),
                peer: 2,
                relay_use: None,
            }),
            r#"{"v":1,"t":"token-req","rid":"0000000000000001","peer":2}"#
        );
        let announced = encode(all()[3].clone());
        assert!(
            announced.ends_with(r#""seq":1,"u":["img","scr"],"bc":25,"pk":512}"#),
            "{announced}"
        );
        assert!(!encode(relay()).contains(r#""u""#));
        assert!(!encode(relay()).contains(r#""pk""#));
        assert!(
            encode(PeerMessage::BroadcastRequest {
                rid: RequestId(6),
                max: 8,
                kind: Some(RelayUse::PluginData),
                media: false,
            })
            .ends_with(r#""max":8,"k":"plg"}"#)
        );
        assert!(
            !encode(PeerMessage::BroadcastRequest {
                rid: RequestId(6),
                max: 8,
                kind: None,
                media: false,
            })
            .contains(r#""k""#)
        );
        assert_eq!(
            encode(PeerMessage::BroadcastAdmit {
                rid: RequestId(5),
                id: BroadcastId(0xab),
                viewer: 9,
            }),
            r#"{"v":1,"t":"bcast-admit","rid":"0000000000000005","id":"00000000000000ab","viewer":9}"#
        );
        assert_eq!(
            encode(PeerMessage::BroadcastToken {
                id: BroadcastId(1),
                token: RelayToken::parse(TOKEN).expect("token"),
                ttl: 60,
                sharer: 8,
                kind: BroadcastKind::PluginData,
                media: false,
            }),
            format!(
                r#"{{"v":1,"t":"bcast-tok","id":"0000000000000001","tok":"{TOKEN}","ttl":60,"sharer":8,"k":"plg"}}"#
            )
        );
        assert_eq!(
            encode(PeerMessage::BroadcastError {
                rid: RequestId(1),
                reason: BroadcastErrorReason::RelayFull,
            }),
            r#"{"v":1,"t":"bcast-err","rid":"0000000000000001","r":"relay-full"}"#
        );
    }

    #[test]
    fn a_broadcast_token_names_its_kind_and_defaults_to_screen_sharing() {
        let old_bot = format!(
            r#"{{"v":1,"t":"bcast-tok","id":"0000000000000001","tok":"{TOKEN}","ttl":60,"sharer":8}}"#
        );
        let Ok(PeerMessage::BroadcastToken { kind, sharer, .. }) =
            PeerMessage::decode(old_bot.as_bytes())
        else {
            panic!("old bot token must decode");
        };
        assert_eq!((kind, sharer), (BroadcastKind::ScreenShare, 8));
        for (wire, expected) in [
            ("scr", BroadcastKind::ScreenShare),
            ("plg", BroadcastKind::PluginData),
            ("cam", BroadcastKind::Camera),
        ] {
            let bytes = format!(
                r#"{{"v":1,"t":"bcast-tok","id":"0000000000000001","tok":"{TOKEN}","ttl":60,"sharer":8,"k":"{wire}"}}"#
            );
            let message = PeerMessage::decode(bytes.as_bytes()).expect(wire);
            assert!(
                matches!(message, PeerMessage::BroadcastToken { kind, .. } if kind == expected),
                "{wire}"
            );
            assert_eq!(expected.as_str(), wire);
            let again = message.encode().expect("encode");
            assert_eq!(PeerMessage::decode(&again).expect("round trip"), message);
        }
    }

    #[test]
    fn every_error_reason_round_trips() {
        for reason in [
            TokenErrorReason::Denied,
            TokenErrorReason::PeerGone,
            TokenErrorReason::PeerNotMumcord,
            TokenErrorReason::RateLimited,
            TokenErrorReason::RelayDown,
            TokenErrorReason::RelayFull,
            TokenErrorReason::UseNotAllowed,
            TokenErrorReason::NotRegistered,
        ] {
            assert_eq!(TokenErrorReason::parse(reason.as_str()), Ok(reason));
        }
        for reason in [
            BroadcastErrorReason::Denied,
            BroadcastErrorReason::NotSameChannel,
            BroadcastErrorReason::Full,
            BroadcastErrorReason::RelayDown,
            BroadcastErrorReason::RelayFull,
            BroadcastErrorReason::UseNotAllowed,
            BroadcastErrorReason::RateLimited,
        ] {
            assert_eq!(BroadcastErrorReason::parse(reason.as_str()), Ok(reason));
        }
    }

    #[test]
    fn broadcast_request_kind() {
        let decode = |extra: &str| {
            let json =
                format!(r#"{{"v":1,"t":"bcast-req","rid":"0000000000000001","max":4{extra}}}"#);
            PeerMessage::decode(json.as_bytes())
        };
        let kind = |extra: &str| match decode(extra) {
            Ok(PeerMessage::BroadcastRequest { kind, .. }) => kind,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(kind(""), None);
        assert_eq!(kind(r#","k":"scr""#), Some(RelayUse::ScreenShare));
        assert_eq!(kind(r#","k":"plg""#), Some(RelayUse::PluginData));
        assert_eq!(decode(r#","k":"img""#), Err(PeerError::BadUse));
        assert_eq!(decode(r#","k":"holo""#), Err(PeerError::BadUse));
        let request = PeerMessage::BroadcastRequest {
            rid: RequestId(1),
            max: 4,
            kind: Some(RelayUse::Files),
            media: false,
        };
        assert_eq!(request.encode(), Err(PeerError::BadUse));
    }

    #[test]
    fn media_version_is_optional_on_every_capability() {
        let mut announcement = match relay() {
            PeerMessage::Relay(announcement) => announcement,
            other => panic!("unexpected: {other:?}"),
        };
        announcement.media = true;
        let bytes = PeerMessage::Relay(announcement.clone())
            .encode()
            .expect("encode");
        assert!(
            String::from_utf8(bytes.clone())
                .expect("utf8")
                .contains(r#""mv":2"#)
        );
        assert_eq!(
            PeerMessage::decode(&bytes),
            Ok(PeerMessage::Relay(announcement))
        );

        let token = RelayToken::parse(TOKEN).expect("token");
        let request = PeerMessage::BroadcastRequest {
            rid: RequestId(1),
            max: 4,
            kind: None,
            media: true,
        };
        let bytes = request.encode().expect("encode");
        assert!(
            String::from_utf8(bytes.clone())
                .expect("utf8")
                .contains(r#""mv":2"#)
        );
        assert_eq!(PeerMessage::decode(&bytes), Ok(request));

        let broadcast = PeerMessage::Broadcast {
            rid: RequestId(1),
            id: BroadcastId(1),
            token: token.clone(),
            ttl: 60,
            media: true,
        };
        let bytes = broadcast.encode().expect("encode");
        assert!(
            String::from_utf8(bytes.clone())
                .expect("utf8")
                .contains(r#""mv":2"#)
        );
        assert_eq!(PeerMessage::decode(&bytes), Ok(broadcast));

        let token_message = PeerMessage::BroadcastToken {
            id: BroadcastId(1),
            token,
            ttl: 60,
            sharer: 8,
            kind: BroadcastKind::ScreenShare,
            media: true,
        };
        let bytes = token_message.encode().expect("encode");
        assert!(
            String::from_utf8(bytes.clone())
                .expect("utf8")
                .contains(r#""mv":2"#)
        );
        assert_eq!(PeerMessage::decode(&bytes), Ok(token_message));

        let old = PeerMessage::BroadcastRequest {
            rid: RequestId(1),
            max: 4,
            kind: None,
            media: false,
        };
        assert!(
            !String::from_utf8(old.encode().expect("encode"))
                .expect("utf8")
                .contains(r#""mv""#)
        );
    }

    #[test]
    fn relay_uses_from_a_newer_or_older_bot() {
        let fp = fp();
        let decode = |extra: &str| {
            let json = format!(
                r#"{{"v":1,"t":"relay","host":"relay","udp":1,"tcp":1,"fp":"{fp}","state":"up","seq":1{extra}}}"#
            );
            let Ok(PeerMessage::Relay(relay)) = PeerMessage::decode(json.as_bytes()) else {
                panic!("not a relay message: {json}");
            };
            relay
        };
        let old = decode("");
        assert_eq!((old.uses.clone(), old.broadcast), (None, None));
        assert!(old.allows(RelayUse::Files));
        assert!(
            !old.allows(RelayUse::PluginData),
            "plugin data is off unless announced"
        );

        let newer = decode(r#","u":["file","holo"],"bc":0"#);
        assert_eq!(newer.uses, Some(vec![RelayUse::Files]));
        assert_eq!(newer.broadcast, None);
        assert!(newer.allows(RelayUse::Files));
        assert!(!newer.allows(RelayUse::ScreenShare));

        let plugin = decode(r#","u":["plg"],"pk":256"#);
        assert_eq!(plugin.uses, Some(vec![RelayUse::PluginData]));
        assert_eq!(plugin.plugin_data_kbps, Some(256));
        assert_eq!(decode(r#","pk":0"#).plugin_data_kbps, None);
        assert_eq!(old.plugin_data_kbps, None);

        let none = decode(r#","u":[],"bc":4"#);
        assert!(!none.allows(RelayUse::Images));
        assert_eq!(none.broadcast, Some(4));
    }

    #[test]
    fn refusal_reasons_round_trip_and_unknown_ones_read_as_denied() {
        // #601
        for reason in [
            BroadcastErrorReason::NotRegistered,
            BroadcastErrorReason::GroupNotAllowed,
            BroadcastErrorReason::NotInChannel,
            BroadcastErrorReason::NoHello,
        ] {
            assert_eq!(BroadcastErrorReason::parse(reason.as_str()), Ok(reason));
        }
        for reason in [TokenErrorReason::GroupNotAllowed, TokenErrorReason::NoHello] {
            assert_eq!(TokenErrorReason::parse(reason.as_str()), Ok(reason));
        }
        assert_eq!(
            BroadcastErrorReason::parse("later"),
            Ok(BroadcastErrorReason::Denied)
        );
        assert_eq!(
            TokenErrorReason::parse("later"),
            Ok(TokenErrorReason::Denied)
        );
    }

    #[test]
    fn the_registered_only_flag_is_announced_only_when_set() {
        // #601
        let plain = r#"{"v":1,"t":"relay","host":"r.example","udp":1,"tcp":2,"fp":"0000000000000000000000000000000000000000000000000000000000000000","state":"up","seq":1}"#;
        let Ok(PeerMessage::Relay(relay)) = PeerMessage::decode(plain.as_bytes()) else {
            panic!("relay");
        };
        assert!(!relay.registered_only);
        let flagged = plain.replace(r#""seq":1"#, r#""seq":1,"reg":true"#);
        let Ok(PeerMessage::Relay(relay)) = PeerMessage::decode(flagged.as_bytes()) else {
            panic!("relay");
        };
        assert!(relay.registered_only);
        let encoded =
            String::from_utf8(PeerMessage::Relay(relay).encode().expect("encode")).expect("utf8");
        assert!(encoded.contains(r#""reg":true"#));
    }
}
