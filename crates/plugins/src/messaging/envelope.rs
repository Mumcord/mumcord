//! Wire format of plugin messages (FR-6.34).
//!
//! Plugin data id `mumcord.p.<plugin-id>`; payload byte 0 is the envelope version, byte 1 the
//! plugin API major, byte 2 the plugin's own message kind, and the rest the plugin payload.
//! Bit 7 of byte 1 ([`DIRECT_FLAG`]) is set when the sender addressed only the receiving user
//! (`peers.send` with a direct target); the low 7 bits are the API major. Both are part of
//! envelope version 1, which is unreleased, so no version bump is needed.
//! The sender is the Mumble session that sent the frame; nothing in the envelope claims identity.
//!
//! Host frames share the data id but carry [`HOST_FRAME_VERSION`] in byte 0 and a host kind in
//! byte 1; the rest is a host document (stage announcements, FR-9.47). They never reach a plugin:
//! this build takes them out before the plugin path, and older builds drop them as an unknown
//! envelope version. Plugins keep all 256 message kinds.

use super::SendError;

/// Prefix of the Mumble plugin data id that carries one plugin's messages.
pub const DATA_ID_PREFIX: &str = "mumcord.p.";
/// The only envelope version this build reads and writes.
pub const ENVELOPE_VERSION: u8 = 1;
/// Byte 0 of a host frame: bit 7 marks the host, the low bits are its version (1).
pub const HOST_FRAME_VERSION: u8 = 0x81;
/// Host kind of the stage frames (`stage::announce`).
pub const HOST_KIND_STAGE: u8 = 1;
/// Host kind of a `relay` message sent through the Mumble fallback (FR-9.61): the body is the
/// plugin's payload, delivered as `relay-message`.
pub const HOST_KIND_RELAY: u8 = 2;
/// Bit 7 of the API major byte: the message was sent to one user, not the whole channel.
pub const DIRECT_FLAG: u8 = 0x80;
/// The bits of byte 1 that hold the plugin API major.
const API_MAJOR_MASK: u8 = !DIRECT_FLAG;
/// Bytes in front of the plugin payload.
pub const HEADER_LEN: usize = 3;
/// Hard ceiling for a plugin payload, independent of the manifest's `messaging.max_payload`.
pub const MAX_PAYLOAD: usize = 997;

/// One decoded plugin message, still untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub api_major: u8,
    pub direct: bool,
    pub kind: u8,
    pub payload: Vec<u8>,
}

/// Why an incoming frame was not a valid envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("plugin message is shorter than its header")]
    Truncated,
    #[error("unsupported plugin message envelope version")]
    UnknownVersion,
    #[error("plugin message payload is too large")]
    TooLarge,
}

/// The plugin data id for a plugin, e.g. `mumcord.p.echolings`.
pub fn data_id(plugin_id: &str) -> String {
    format!("{DATA_ID_PREFIX}{plugin_id}")
}

/// The plugin id carried by a data id, or `None` if it is not a plugin message id.
pub fn plugin_id_of(data_id: &str) -> Option<&str> {
    data_id
        .strip_prefix(DATA_ID_PREFIX)
        .filter(|id| crate::manifest::is_valid_id(id))
}

/// Encodes a message. `max_payload` is the manifest's `messaging.max_payload`; the effective
/// limit is the smaller of it and [`MAX_PAYLOAD`]. `api_major` uses 7 bits; higher bits are
/// dropped so they can never be mistaken for the direct flag.
pub fn encode(
    api_major: u8,
    direct: bool,
    kind: u8,
    payload: &[u8],
    max_payload: u32,
) -> Result<Vec<u8>, SendError> {
    let limit = MAX_PAYLOAD.min(usize::try_from(max_payload).unwrap_or(usize::MAX));
    if payload.len() > limit {
        return Err(SendError::TooLarge);
    }
    let flagged_major = (api_major & API_MAJOR_MASK) | if direct { DIRECT_FLAG } else { 0 };
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(&[ENVELOPE_VERSION, flagged_major, kind]);
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

/// A host frame: byte 0 [`HOST_FRAME_VERSION`], byte 1 the host kind, then the body. The body's
/// encoder bounds its size.
pub fn encode_host(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(2 + body.len());
    bytes.extend_from_slice(&[HOST_FRAME_VERSION, kind]);
    bytes.extend_from_slice(body);
    bytes
}

/// The host kind and body of a host frame, or `None` for anything else (a plugin envelope).
pub fn decode_host(bytes: &[u8]) -> Option<(u8, &[u8])> {
    match bytes {
        [HOST_FRAME_VERSION, kind, body @ ..] => Some((*kind, body)),
        _ => None,
    }
}

/// Decodes a received frame.
pub fn decode(bytes: &[u8]) -> Result<Envelope, DecodeError> {
    let [version, api_major, kind, payload @ ..] = bytes else {
        return Err(DecodeError::Truncated);
    };
    if *version != ENVELOPE_VERSION {
        return Err(DecodeError::UnknownVersion);
    }
    if payload.len() > MAX_PAYLOAD {
        return Err(DecodeError::TooLarge);
    }
    Ok(Envelope {
        api_major: *api_major & API_MAJOR_MASK,
        direct: *api_major & DIRECT_FLAG != 0,
        kind: *kind,
        payload: payload.to_vec(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let bytes = encode(1, false, 7, b"hello", 997).unwrap();
        assert_eq!(&bytes[..3], &[1, 1, 7]);
        let env = decode(&bytes).unwrap();
        assert_eq!(
            env,
            Envelope {
                api_major: 1,
                direct: false,
                kind: 7,
                payload: b"hello".to_vec()
            }
        );
    }

    #[test]
    fn direct_flag_round_trips_in_bit_7_of_byte_1() {
        let bytes = encode(1, true, 7, b"x", 997).unwrap();
        assert_eq!(&bytes[..3], &[1, 0x81, 7]);
        let env = decode(&bytes).unwrap();
        assert!(env.direct);
        assert_eq!(env.api_major, 1);
        assert!(
            !decode(&encode(1, false, 7, b"x", 997).unwrap())
                .unwrap()
                .direct
        );
    }

    #[test]
    fn api_major_cannot_spoof_the_direct_flag() {
        let bytes = encode(0x81, false, 0, &[], 997).unwrap();
        assert_eq!(bytes[1], 1);
        assert!(!decode(&bytes).unwrap().direct);
    }

    #[test]
    fn empty_payload_is_valid() {
        let env = decode(&encode(1, false, 0, &[], 997).unwrap()).unwrap();
        assert!(env.payload.is_empty());
    }

    #[test]
    fn payload_limit_is_the_smaller_of_manifest_and_hard_cap() {
        assert_eq!(
            encode(1, false, 0, &[0; 998], 4096),
            Err(SendError::TooLarge)
        );
        assert!(encode(1, false, 0, &[0; 997], 4096).is_ok());
        assert_eq!(encode(1, false, 0, &[0; 11], 10), Err(SendError::TooLarge));
        assert!(encode(1, false, 0, &[0; 10], 10).is_ok());
    }

    #[test]
    fn decode_rejects_bad_frames() {
        assert_eq!(decode(&[1, 1]), Err(DecodeError::Truncated));
        assert_eq!(decode(&[2, 1, 0]), Err(DecodeError::UnknownVersion));
        let mut big = vec![1, 1, 0];
        big.extend_from_slice(&[0; 998]);
        assert_eq!(decode(&big), Err(DecodeError::TooLarge));
    }

    #[test]
    fn host_frames_round_trip_and_are_never_plugin_envelopes() {
        let bytes = encode_host(HOST_KIND_STAGE, b"body");
        assert_eq!(&bytes[..2], &[0x81, 1]);
        assert_eq!(decode_host(&bytes), Some((HOST_KIND_STAGE, &b"body"[..])));
        assert_eq!(decode(&bytes), Err(DecodeError::UnknownVersion));
        assert_eq!(decode_host(&encode(1, false, 1, b"x", 997).unwrap()), None);
        assert_eq!(decode_host(&[0x81]), None);
    }

    #[test]
    fn data_ids() {
        assert_eq!(data_id("echolings"), "mumcord.p.echolings");
        assert_eq!(plugin_id_of("mumcord.p.echolings"), Some("echolings"));
        assert_eq!(plugin_id_of("mumcord.plugins"), None);
        assert_eq!(plugin_id_of("mumcord.p.Bad!"), None);
        assert_eq!(plugin_id_of("mumcord.p."), None);
    }
}
