//! Setup of a screen-share broadcast through the community relay (spec 07, FR-7.41–7.45,
//! FR-7.89–7.98, section 6.6): the relay `bind` with a role and the shared slot helper.
//!
//! A broadcast has one publisher (the sharer) and up to `max_viewers` subscribers. Each side
//! opens its own outer relay link and sends a [`Bind`] with its broadcast token and role. The
//! relay refuses with the bind error codes of spec 04 plus `use_not_allowed` and `broadcast_full`
//! ([`crate::RelayRefusal`]).
//!
//! The old clear 14-byte header and its unit types live in [`legacy`]. They are kept for 0.8.x
//! peers and old relays and are used only by the fallback selection (FR-7.98). New links use the
//! Bovnar media profile; this module re-exports the legacy items only so existing 0.8.x-path code
//! keeps compiling.

use serde::{Deserialize, Serialize};

use crate::peer::{PEER_VERSION, PeerError, RelayToken};

pub mod legacy;

// Backward-compatible re-exports of the old wire types. New code paths must not use them
// (FR-7.98); they stay here for the current 0.8.x path and the fallback selection.
pub use legacy::{
    Content, ControlKind, ENCRYPTION_OVERHEAD, HEADER_LEN, HEADER_VERSION, HEIGHT_LEN, Header,
    HeaderError, MAX_AUDIO_PAYLOAD, MAX_DATA_PAYLOAD, MAX_KEY_BODY, MAX_TEMPORAL_LAYER,
    MAX_VIDEO_PAYLOAD, SLOT_LEN, VideoCodec,
};

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

/// Splits the leading subscriber slot off a `keyreq` (as the publisher receives it), `key` (as
/// the publisher sends it) or `gone` payload. Used by both broadcast formats.
pub fn split_slot(payload: &[u8]) -> Option<(u16, &[u8])> {
    let (slot, rest) = payload.split_first_chunk::<2>()?;
    Some((u16::from_be_bytes(*slot), rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA_";

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
