//! The relay-data envelope (FR-9.62): what one plugin message looks like inside the encrypted
//! plugin-data stream of a relay pair session.
//!
//! The relay only forwards QUIC packets of the end-to-end encrypted connection, so everything
//! here (the plugin id included) is visible to the two clients only (FR-9.59).
//!
//! Layout: API major (1 byte), API minor (1), per-sender sequence (8, big-endian), plugin id
//! length (1), plugin id (UTF-8), payload. The payload is the plugin's opaque bytes: never
//! parsed, rewritten or logged (FR-9.94). On the stream every envelope is preceded by its
//! length (4 bytes, big-endian).

/// Largest plugin payload of one relay message (FR-9.60).
pub const MAX_DATA: usize = 64 * 1024;
/// Longest plugin id the envelope carries; ids are far shorter (manifest rules).
const MAX_ID: usize = u8::MAX as usize;
/// API major, API minor, sequence and id length.
const FIXED: usize = 1 + 1 + 8 + 1;
/// Largest envelope a receiver reads; a longer length prefix ends the stream.
pub const MAX_ENVELOPE: usize = FIXED + MAX_ID + MAX_DATA;

/// One plugin message, still untrusted when decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub api_major: u8,
    pub api_minor: u8,
    /// Counts this sender's messages to one receiver for one plugin, from 1.
    pub seq: u64,
    pub plugin: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnvelopeError {
    #[error("payload larger than {MAX_DATA} bytes")]
    TooLarge,
    #[error("invalid plugin id")]
    BadPluginId,
    #[error("truncated envelope")]
    Truncated,
}

/// The envelope bytes without the length prefix.
pub fn encode(envelope: &Envelope) -> Result<Vec<u8>, EnvelopeError> {
    if envelope.data.len() > MAX_DATA {
        return Err(EnvelopeError::TooLarge);
    }
    let id = envelope.plugin.as_bytes();
    if id.is_empty() || id.len() > MAX_ID {
        return Err(EnvelopeError::BadPluginId);
    }
    let mut bytes = Vec::with_capacity(FIXED + id.len() + envelope.data.len());
    bytes.push(envelope.api_major);
    bytes.push(envelope.api_minor);
    bytes.extend_from_slice(&envelope.seq.to_be_bytes());
    bytes.push(id.len() as u8);
    bytes.extend_from_slice(id);
    bytes.extend_from_slice(&envelope.data);
    Ok(bytes)
}

/// Reads an envelope received from another client.
pub fn decode(bytes: &[u8]) -> Result<Envelope, EnvelopeError> {
    if bytes.len() < FIXED {
        return Err(EnvelopeError::Truncated);
    }
    let (fixed, rest) = bytes.split_at(FIXED);
    let id_len = usize::from(fixed[10]);
    if id_len == 0 {
        return Err(EnvelopeError::BadPluginId);
    }
    if rest.len() < id_len {
        return Err(EnvelopeError::Truncated);
    }
    let (id, data) = rest.split_at(id_len);
    if data.len() > MAX_DATA {
        return Err(EnvelopeError::TooLarge);
    }
    let plugin = std::str::from_utf8(id).map_err(|_| EnvelopeError::BadPluginId)?;
    let mut seq = [0u8; 8];
    seq.copy_from_slice(&fixed[2..10]);
    Ok(Envelope {
        api_major: fixed[0],
        api_minor: fixed[1],
        seq: u64::from_be_bytes(seq),
        plugin: plugin.to_owned(),
        data: data.to_vec(),
    })
}

/// The 4-byte length prefix in front of an envelope on the stream.
pub fn length_prefix(envelope: &[u8]) -> [u8; 4] {
    // MAX_ENVELOPE fits easily; encode() bounds every envelope.
    (envelope.len() as u32).to_be_bytes()
}

/// The envelope length a prefix announces, `None` when it is longer than any valid envelope.
pub fn announced_len(prefix: [u8; 4]) -> Option<usize> {
    let len = u32::from_be_bytes(prefix) as usize;
    (len <= MAX_ENVELOPE).then_some(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(data: Vec<u8>) -> Envelope {
        Envelope {
            api_major: 1,
            api_minor: 1,
            seq: 7,
            plugin: "test-relay".into(),
            data,
        }
    }

    #[test]
    fn round_trips_and_keeps_the_payload_byte_for_byte() {
        let original = envelope((0..=255u8).cycle().take(MAX_DATA).collect());
        let bytes = encode(&original).expect("valid");
        assert_eq!(&bytes[..11], &[1, 1, 0, 0, 0, 0, 0, 0, 0, 7, 10]);
        assert_eq!(&bytes[11..21], b"test-relay");
        assert_eq!(decode(&bytes).expect("decoded"), original);
    }

    #[test]
    fn an_empty_payload_is_a_message_too() {
        let original = envelope(Vec::new());
        assert_eq!(
            decode(&encode(&original).expect("valid")).expect("decoded"),
            original
        );
    }

    #[test]
    fn refuses_more_than_64_kib_both_ways() {
        let big = envelope(vec![0; MAX_DATA + 1]);
        assert_eq!(encode(&big), Err(EnvelopeError::TooLarge));
        let mut bytes = encode(&envelope(vec![0; MAX_DATA])).expect("valid");
        bytes.push(0);
        assert_eq!(decode(&bytes), Err(EnvelopeError::TooLarge));
    }

    #[test]
    fn refuses_bad_ids_and_truncated_frames() {
        let mut no_id = envelope(vec![1]);
        no_id.plugin.clear();
        assert_eq!(encode(&no_id), Err(EnvelopeError::BadPluginId));
        let bytes = encode(&envelope(vec![1, 2])).expect("valid");
        assert_eq!(decode(&bytes[..5]), Err(EnvelopeError::Truncated));
        assert_eq!(decode(&bytes[..15]), Err(EnvelopeError::Truncated));
        let mut bad_utf8 = bytes.clone();
        bad_utf8[11] = 0xff;
        assert_eq!(decode(&bad_utf8), Err(EnvelopeError::BadPluginId));
        let mut zero_len = bytes;
        zero_len[10] = 0;
        assert_eq!(decode(&zero_len), Err(EnvelopeError::BadPluginId));
    }

    #[test]
    fn length_prefix_bounds_the_read() {
        let bytes = encode(&envelope(vec![9; 3])).expect("valid");
        assert_eq!(announced_len(length_prefix(&bytes)), Some(bytes.len()));
        assert_eq!(announced_len((MAX_ENVELOPE as u32 + 1).to_be_bytes()), None);
    }
}
