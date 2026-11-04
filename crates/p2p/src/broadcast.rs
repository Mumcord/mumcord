//! Wire format of a screen-share broadcast through the community relay (spec 07, FR-7.41–7.45,
//! section 6.6). Pure encoding and decoding; the relay link that carries it is in `relay.rs`.
//!
//! A broadcast has one publisher (the sharer) and up to `max_viewers` subscribers. Each side
//! opens its own outer relay link and sends a [`Bind`] with its broadcast token and role. The
//! relay refuses with the bind error codes of spec 04 plus `use_not_allowed` and `broadcast_full`
//! ([`crate::RelayRefusal`]).
//!
//! After `ready` every unit on the link (media frame or control message) is a 14-byte clear
//! [`Header`] followed by its payload. Media payloads are end-to-end encrypted by the sharer, so
//! the relay only reads the header: enough to drop temporal layers, wait for keyframes and route
//! control messages. The byte layout and test vectors are in the crate README so the relay
//! (mumpeer) can mirror it.
//!
//! Everything received is untrusted: the header is fully checked before any payload is read, so
//! a bad size never makes the reader allocate.

use serde::{Deserialize, Serialize};

use crate::peer::{PEER_VERSION, PeerError, RelayToken};

/// The header version this side writes and reads; another version ends the link, since the
/// layout may differ.
pub const HEADER_VERSION: u8 = 1;
/// Length of the clear [`Header`] in bytes.
pub const HEADER_LEN: usize = 14;
/// Highest temporal layer (same as the direct screen-share stream).
pub const MAX_TEMPORAL_LAYER: u8 = 3;
/// Room in an encrypted media payload for the encryption data (authentication tag and
/// counter), on top of the encoded frame.
pub const ENCRYPTION_OVERHEAD: u32 = 64;
/// Largest encrypted video payload: a 4 MiB frame (far above a 1440p keyframe) plus overhead.
pub const MAX_VIDEO_PAYLOAD: u32 = 4 * 1024 * 1024 + ENCRYPTION_OVERHEAD;
/// Largest encrypted audio payload: a multi-frame Opus packet plus overhead.
pub const MAX_AUDIO_PAYLOAD: u32 = 4 * 1024 + ENCRYPTION_OVERHEAD;
/// Largest encrypted plugin data payload: 64 KiB (mumpeer's default `max_unit_bytes`) plus overhead.
pub const MAX_DATA_PAYLOAD: u32 = 64 * 1024 + ENCRYPTION_OVERHEAD;
/// Largest opaque body of a `keyreq` or `key` control message.
pub const MAX_KEY_BODY: u32 = 1024;
/// Size of the subscriber slot number that prefixes `keyreq` (added by the relay), `key` (set
/// by the publisher) and `gone`.
pub const SLOT_LEN: u32 = 2;
/// Size of the tile height in a `want` message (u16, big endian).
pub const HEIGHT_LEN: u32 = 2;

const KIND_VIDEO: u8 = 1;
const KIND_AUDIO: u8 = 2;
const KIND_CONTROL: u8 = 3;
const KIND_DATA: u8 = 4;
const FLAG_KEYFRAME: u8 = 0b1;
const CODEC_VP9: u8 = 1;
const CODEC_AV1: u8 = 2;
const CODEC_OPUS: u8 = 3;

/// Which end of a broadcast a relay link binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindRole {
    /// The sharer (`pub`): sends media once, receives control messages.
    Publisher,
    /// A viewer (`sub`): receives media, sends control messages.
    Subscriber,
}

impl BindRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Publisher => "pub",
            Self::Subscriber => "sub",
        }
    }
}

/// The relay `bind` message, `{"v":1,"t":"bind","tok":"…","role":"pub"|"sub"}`. A bind without
/// `role` is a pair bind (spec 04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub token: RelayToken,
    /// `None` for a pair token, the broadcast role otherwise.
    pub role: Option<BindRole>,
}

#[derive(Serialize, Deserialize)]
struct BindWire {
    v: u32,
    t: String,
    tok: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    role: Option<String>,
}

impl Bind {
    /// The bind message as sent on the relay link (one JSON object, no line end).
    pub fn encode(&self) -> Vec<u8> {
        let wire = BindWire {
            v: PEER_VERSION,
            t: "bind".to_owned(),
            tok: self.token.expose().to_owned(),
            role: self.role.map(|role| role.as_str().to_owned()),
        };
        // Strings and integers only, so serialising cannot fail.
        serde_json::to_vec(&wire).unwrap_or_default()
    }

    /// Reads a bind message; unknown fields are ignored.
    pub fn decode(bytes: &[u8]) -> Result<Self, PeerError> {
        let wire: BindWire = serde_json::from_slice(bytes).map_err(|_| PeerError::Malformed)?;
        if wire.v < PEER_VERSION {
            return Err(PeerError::UnsupportedVersion(wire.v));
        }
        if wire.t != "bind" {
            return Err(PeerError::UnknownType);
        }
        let role = match wire.role.as_deref() {
            None => None,
            Some("pub") => Some(BindRole::Publisher),
            Some("sub") => Some(BindRole::Subscriber),
            Some(_) => return Err(PeerError::Malformed),
        };
        Ok(Self {
            token: RelayToken::parse(&wire.tok)?,
            role,
        })
    }
}

/// Video codec of a broadcast frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    Vp9,
    Av1,
}

/// Control messages of a broadcast; the payload layout of each is in the crate README.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    /// Subscriber → publisher: opaque key request; the relay prepends the subscriber's slot.
    KeyRequest = 1,
    /// Publisher → one subscriber: slot, then the opaque key answer; the relay strips the slot.
    Key = 2,
    /// Subscriber → relay → publisher: send a keyframe soon (rate-limited by the relay).
    KeyframeRequest = 3,
    /// Subscriber → relay: the highest temporal layer this subscriber wants (one byte).
    Layers = 4,
    /// Relay → publisher: the subscriber in this slot left.
    Gone = 5,
    /// Subscriber → relay → publisher: the tile height the subscriber draws (u16); the relay
    /// prepends the subscriber's slot (camera sharing, spec 08).
    Want = 6,
}

impl ControlKind {
    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            1 => Self::KeyRequest,
            2 => Self::Key,
            3 => Self::KeyframeRequest,
            4 => Self::Layers,
            5 => Self::Gone,
            6 => Self::Want,
            _ => return None,
        })
    }

    /// Smallest and largest payload this control message may carry. `keyreq` is checked
    /// against the larger form with the slot, which only the publisher receives.
    fn payload_range(self) -> (u32, u32) {
        match self {
            Self::KeyRequest | Self::Key => (0, SLOT_LEN + MAX_KEY_BODY),
            Self::KeyframeRequest => (0, 0),
            Self::Layers => (1, 1),
            Self::Want => (HEIGHT_LEN, SLOT_LEN + HEIGHT_LEN),
            Self::Gone => (SLOT_LEN, SLOT_LEN),
        }
    }
}

/// What a unit carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    /// An encrypted video frame.
    Video {
        codec: VideoCodec,
        keyframe: bool,
        temporal_layer: u8,
    },
    /// An encrypted Opus packet of the share sound.
    Audio,
    /// An encrypted plugin data unit (spec 09). The relay forwards it without looking inside and
    /// treats it as the lowest priority.
    Data,
    /// A control message (not encrypted by this layer; `keyreq`/`key` bodies are protected by
    /// the key exchange itself).
    Control(ControlKind),
}

/// The clear header in front of every broadcast unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub content: Content,
    /// Id of the share key that encrypted the payload; changes on every rekey (FR-7.44).
    /// Always 0 for control messages.
    pub key_id: u8,
    /// Counter of the publisher's media units (video and audio share one counter, so key id and
    /// sequence never repeat under one key). Always 0 for control messages.
    pub sequence: u32,
    /// Bytes of payload that follow the header.
    pub payload_len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    #[error("broadcast header shorter than {HEADER_LEN} bytes")]
    Truncated,
    #[error("unsupported broadcast header version {0}")]
    UnsupportedVersion(u8),
    #[error("unknown broadcast unit kind {0}")]
    BadKind(u8),
    #[error("invalid broadcast header flags")]
    BadFlags,
    #[error("invalid temporal layer {0}")]
    BadLayer(u8),
    #[error("invalid codec or control type {0}")]
    BadCodec(u8),
    #[error("control message with a key id or sequence")]
    BadControl,
    #[error("payload of {0} bytes is too large or too small for this unit")]
    BadSize(u32),
}

impl Header {
    /// Writes the header; fails for values the receiver would reject.
    pub fn encode(&self) -> Result<[u8; HEADER_LEN], HeaderError> {
        self.validate()?;
        let (kind, flags, layer, codec) = match self.content {
            Content::Video {
                codec,
                keyframe,
                temporal_layer,
            } => (
                KIND_VIDEO,
                if keyframe { FLAG_KEYFRAME } else { 0 },
                temporal_layer,
                match codec {
                    VideoCodec::Vp9 => CODEC_VP9,
                    VideoCodec::Av1 => CODEC_AV1,
                },
            ),
            Content::Audio => (KIND_AUDIO, 0, 0, CODEC_OPUS),
            Content::Data => (KIND_DATA, 0, 0, 0),
            Content::Control(control) => (KIND_CONTROL, 0, 0, control as u8),
        };
        let mut out = [0u8; HEADER_LEN];
        out[0] = HEADER_VERSION;
        out[1] = kind;
        out[2] = flags;
        out[3] = layer;
        out[4] = codec;
        out[5] = self.key_id;
        out[6..10].copy_from_slice(&self.sequence.to_be_bytes());
        out[10..14].copy_from_slice(&self.payload_len.to_be_bytes());
        Ok(out)
    }

    /// Reads and checks the header at the start of `bytes` (extra bytes are ignored).
    pub fn decode(bytes: &[u8]) -> Result<Self, HeaderError> {
        let Some(bytes) = bytes.get(..HEADER_LEN) else {
            return Err(HeaderError::Truncated);
        };
        if bytes[0] != HEADER_VERSION {
            return Err(HeaderError::UnsupportedVersion(bytes[0]));
        }
        let (flags, layer, codec) = (bytes[2], bytes[3], bytes[4]);
        let content = match bytes[1] {
            KIND_VIDEO => {
                if flags & !FLAG_KEYFRAME != 0 {
                    return Err(HeaderError::BadFlags);
                }
                Content::Video {
                    codec: match codec {
                        CODEC_VP9 => VideoCodec::Vp9,
                        CODEC_AV1 => VideoCodec::Av1,
                        other => return Err(HeaderError::BadCodec(other)),
                    },
                    keyframe: flags & FLAG_KEYFRAME != 0,
                    temporal_layer: layer,
                }
            }
            KIND_AUDIO => {
                if codec != CODEC_OPUS {
                    return Err(HeaderError::BadCodec(codec));
                }
                Content::Audio
            }
            KIND_DATA => {
                if codec != 0 {
                    return Err(HeaderError::BadCodec(codec));
                }
                Content::Data
            }
            KIND_CONTROL => {
                Content::Control(ControlKind::from_byte(codec).ok_or(HeaderError::BadCodec(codec))?)
            }
            other => return Err(HeaderError::BadKind(other)),
        };
        if !matches!(content, Content::Video { .. }) {
            if flags != 0 {
                return Err(HeaderError::BadFlags);
            }
            if layer != 0 {
                return Err(HeaderError::BadLayer(layer));
            }
        }
        let header = Self {
            content,
            key_id: bytes[5],
            sequence: u32::from_be_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]),
            payload_len: u32::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]),
        };
        header.validate()?;
        Ok(header)
    }

    fn validate(&self) -> Result<(), HeaderError> {
        let (min, max) = match self.content {
            Content::Video { temporal_layer, .. } => {
                if temporal_layer > MAX_TEMPORAL_LAYER {
                    return Err(HeaderError::BadLayer(temporal_layer));
                }
                (1, MAX_VIDEO_PAYLOAD)
            }
            Content::Audio => (1, MAX_AUDIO_PAYLOAD),
            Content::Data => (1, MAX_DATA_PAYLOAD),
            Content::Control(control) => {
                if self.key_id != 0 || self.sequence != 0 {
                    return Err(HeaderError::BadControl);
                }
                control.payload_range()
            }
        };
        if (min..=max).contains(&self.payload_len) {
            Ok(())
        } else {
            Err(HeaderError::BadSize(self.payload_len))
        }
    }
}

/// Splits the leading subscriber slot off a `keyreq` (as the publisher receives it), `key` (as
/// the publisher sends it) or `gone` payload.
pub fn split_slot(payload: &[u8]) -> Option<(u16, &[u8])> {
    let (slot, rest) = payload.split_first_chunk::<2>()?;
    Some((u16::from_be_bytes(*slot), rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA_";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn control(kind: ControlKind, payload_len: u32) -> Header {
        Header {
            content: Content::Control(kind),
            key_id: 0,
            sequence: 0,
            payload_len,
        }
    }

    /// The vectors printed in the crate README; mumpeer mirrors them.
    fn vectors() -> Vec<(&'static str, Header)> {
        vec![
            (
                "0101010002020000000100000400",
                Header {
                    content: Content::Video {
                        codec: VideoCodec::Av1,
                        keyframe: true,
                        temporal_layer: 0,
                    },
                    key_id: 2,
                    sequence: 1,
                    payload_len: 1024,
                },
            ),
            (
                "0101000201070000002a000000c8",
                Header {
                    content: Content::Video {
                        codec: VideoCodec::Vp9,
                        keyframe: false,
                        temporal_layer: 2,
                    },
                    key_id: 7,
                    sequence: 42,
                    payload_len: 200,
                },
            ),
            (
                "010200000302000000020000005c",
                Header {
                    content: Content::Audio,
                    key_id: 2,
                    sequence: 2,
                    payload_len: 92,
                },
            ),
            (
                "010400000003000000070000015e",
                Header {
                    content: Content::Data,
                    key_id: 3,
                    sequence: 7,
                    payload_len: 350,
                },
            ),
            (
                "0103000001000000000000000062",
                control(ControlKind::KeyRequest, 98),
            ),
            (
                "0103000002000000000000000064",
                control(ControlKind::Key, 100),
            ),
            (
                "0103000003000000000000000000",
                control(ControlKind::KeyframeRequest, 0),
            ),
            (
                "0103000004000000000000000001",
                control(ControlKind::Layers, 1),
            ),
            (
                "0103000005000000000000000002",
                control(ControlKind::Gone, 2),
            ),
            (
                "0103000006000000000000000002",
                control(ControlKind::Want, 2),
            ),
            (
                "0103000006000000000000000004",
                control(ControlKind::Want, 4),
            ),
        ]
    }

    #[test]
    fn matches_the_readme_test_vectors() {
        for (expected, header) in vectors() {
            let bytes = header.encode().expect("encode");
            assert_eq!(hex(&bytes), expected);
            assert_eq!(Header::decode(&bytes), Ok(header));
        }
    }

    #[test]
    fn decode_ignores_the_payload_after_the_header() {
        let (_, header) = vectors()[0];
        let mut bytes = header.encode().expect("encode").to_vec();
        bytes.extend_from_slice(&[0xff; 8]);
        assert_eq!(Header::decode(&bytes), Ok(header));
    }

    #[test]
    fn rejects_bad_headers() {
        let valid = vectors()[0].1.encode().expect("encode");
        let with = |index: usize, value: u8| {
            let mut bytes = valid;
            bytes[index] = value;
            bytes
        };
        let size = |kind: u8, codec: u8, len: u32| {
            let mut bytes = valid;
            bytes[1] = kind;
            bytes[2] = 0;
            bytes[4] = codec;
            bytes[5] = 0;
            bytes[6..10].copy_from_slice(&[0; 4]);
            bytes[10..14].copy_from_slice(&len.to_be_bytes());
            bytes
        };
        let cases = [
            (with(0, 2), HeaderError::UnsupportedVersion(2)),
            (with(1, 0), HeaderError::BadKind(0)),
            (with(1, 5), HeaderError::BadKind(5)),
            (with(2, 0b10), HeaderError::BadFlags),
            (with(3, 4), HeaderError::BadLayer(4)),
            (with(4, 3), HeaderError::BadCodec(3)),
            (with(4, 0), HeaderError::BadCodec(0)),
            (size(KIND_VIDEO, CODEC_VP9, 0), HeaderError::BadSize(0)),
            (
                size(KIND_VIDEO, CODEC_VP9, MAX_VIDEO_PAYLOAD + 1),
                HeaderError::BadSize(MAX_VIDEO_PAYLOAD + 1),
            ),
            (
                size(KIND_AUDIO, CODEC_AV1, 10),
                HeaderError::BadCodec(CODEC_AV1),
            ),
            (
                size(KIND_AUDIO, CODEC_OPUS, MAX_AUDIO_PAYLOAD + 1),
                HeaderError::BadSize(MAX_AUDIO_PAYLOAD + 1),
            ),
            (size(KIND_DATA, 1, 10), HeaderError::BadCodec(1)),
            (size(KIND_DATA, 0, 0), HeaderError::BadSize(0)),
            (
                size(KIND_DATA, 0, MAX_DATA_PAYLOAD + 1),
                HeaderError::BadSize(MAX_DATA_PAYLOAD + 1),
            ),
            (size(KIND_CONTROL, 7, 0), HeaderError::BadCodec(7)),
            (size(KIND_CONTROL, 6, 0), HeaderError::BadSize(0)),
            (size(KIND_CONTROL, 6, 5), HeaderError::BadSize(5)),
            (size(KIND_CONTROL, 3, 1), HeaderError::BadSize(1)),
            (size(KIND_CONTROL, 4, 2), HeaderError::BadSize(2)),
            (size(KIND_CONTROL, 5, 1), HeaderError::BadSize(1)),
            (size(KIND_CONTROL, 1, 1027), HeaderError::BadSize(1027)),
            (size(KIND_CONTROL, 2, 1027), HeaderError::BadSize(1027)),
        ];
        for (bytes, expected) in cases {
            assert_eq!(Header::decode(&bytes), Err(expected), "{}", hex(&bytes));
        }
        assert_eq!(Header::decode(&valid[..13]), Err(HeaderError::Truncated));

        let mut audio_flag = size(KIND_AUDIO, CODEC_OPUS, 10);
        audio_flag[2] = FLAG_KEYFRAME;
        assert_eq!(Header::decode(&audio_flag), Err(HeaderError::BadFlags));
        let mut control_layer = size(KIND_CONTROL, 3, 0);
        control_layer[3] = 1;
        assert_eq!(
            Header::decode(&control_layer),
            Err(HeaderError::BadLayer(1))
        );
        let mut control_key = size(KIND_CONTROL, 3, 0);
        control_key[5] = 1;
        assert_eq!(Header::decode(&control_key), Err(HeaderError::BadControl));
        let mut control_seq = size(KIND_CONTROL, 3, 0);
        control_seq[9] = 1;
        assert_eq!(Header::decode(&control_seq), Err(HeaderError::BadControl));
    }

    #[test]
    fn encode_rejects_what_decode_would() {
        let mut header = vectors()[0].1;
        header.content = Content::Video {
            codec: VideoCodec::Vp9,
            keyframe: false,
            temporal_layer: 4,
        };
        assert_eq!(header.encode(), Err(HeaderError::BadLayer(4)));
        let mut key = control(ControlKind::Key, 10);
        key.key_id = 1;
        assert_eq!(key.encode(), Err(HeaderError::BadControl));
    }

    #[test]
    fn splits_the_slot() {
        assert_eq!(split_slot(&[0x00, 0x05, 9, 9]), Some((5, &[9u8, 9][..])));
        assert_eq!(split_slot(&[0x01, 0x00]), Some((256, &[][..])));
        assert_eq!(split_slot(&[1]), None);
    }

    #[test]
    fn bind_messages() {
        let token = RelayToken::parse(TOKEN).expect("token");
        let bind = Bind {
            token: token.clone(),
            role: Some(BindRole::Subscriber),
        };
        let bytes = bind.encode();
        assert_eq!(
            String::from_utf8(bytes.clone()).expect("utf8"),
            format!(r#"{{"v":1,"t":"bind","tok":"{TOKEN}","role":"sub"}}"#)
        );
        assert_eq!(Bind::decode(&bytes), Ok(bind));

        // A pair bind (spec 04) has no role.
        let pair = format!(r#"{{"v":1,"t":"bind","tok":"{TOKEN}"}}"#);
        assert_eq!(
            Bind::decode(pair.as_bytes()),
            Ok(Bind { token, role: None })
        );
        assert_eq!(
            Bind {
                token: RelayToken::parse(TOKEN).expect("token"),
                role: None
            }
            .encode(),
            pair.as_bytes()
        );

        let newer = format!(r#"{{"v":2,"t":"bind","tok":"{TOKEN}","role":"pub","x":1}}"#);
        assert_eq!(
            Bind::decode(newer.as_bytes()).map(|bind| bind.role),
            Ok(Some(BindRole::Publisher))
        );
        for (input, expected) in [
            (
                format!(r#"{{"v":1,"t":"bind","tok":"{TOKEN}","role":"admin"}}"#),
                PeerError::Malformed,
            ),
            (
                format!(r#"{{"v":1,"t":"ready","tok":"{TOKEN}"}}"#),
                PeerError::UnknownType,
            ),
            (
                format!(r#"{{"v":0,"t":"bind","tok":"{TOKEN}"}}"#),
                PeerError::UnsupportedVersion(0),
            ),
            (
                r#"{"v":1,"t":"bind","tok":"short"}"#.to_owned(),
                PeerError::BadToken,
            ),
        ] {
            assert_eq!(Bind::decode(input.as_bytes()), Err(expected), "{input}");
        }
        let debug = format!("{:?}", Bind::decode(&bytes));
        assert!(!debug.contains(TOKEN), "{debug}");
    }
}
