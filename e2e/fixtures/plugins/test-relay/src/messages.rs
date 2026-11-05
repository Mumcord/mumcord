//! The test message as a Bovnar document (spec 09 FR-9.93): a sequence number, a checksum and a
//! patterned body, so the receiver can check that a message arrived whole and in order.
//!
//! The document is built and read with the SDK's `bovnar` helpers. The layout version is 1; a
//! newer sender may add fields, which this version ignores.

use mumcord_plugin_sdk::bovnar::{self, Message, RELAY_LIMIT};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestMessage {
    pub seq: u32,
    /// FNV-1a of `body` as 8 hex digits. A fixed width keeps the document size predictable (a
    /// number is written as text, so its length would depend on its value).
    pub checksum: String,
    pub body: String,
}

impl Message for TestMessage {
    fn version() -> u32 {
        VERSION
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Received {
    pub seq: u32,
    pub size: usize,
    pub intact: bool,
}

fn checksum(body: &str) -> String {
    let sum = body.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    format!("{sum:08x}")
}

fn with_body_len(seq: u32, len: usize) -> TestMessage {
    let body: String = (0..len)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    TestMessage {
        seq,
        checksum: checksum(&body),
        body,
    }
}

fn document_len(message: &TestMessage) -> Option<usize> {
    bovnar::encode(message, RELAY_LIMIT).ok().map(|d| d.len())
}

/// A message whose document is larger than the relay limit: its body alone fills the limit.
pub fn oversized(seq: u32) -> TestMessage {
    with_body_len(seq, RELAY_LIMIT)
}

/// A message whose Bovnar document is exactly `size` bytes (at least the empty message). The
/// document length grows with the body by one byte per character plus a length field that may
/// widen, so the body length is corrected until the document fits.
pub fn message(seq: u32, size: usize) -> Option<TestMessage> {
    let empty = document_len(&with_body_len(seq, 0))?;
    let mut len = size.saturating_sub(empty);
    for _ in 0..8 {
        let candidate = with_body_len(seq, len);
        let actual = document_len(&candidate)?;
        if actual == size {
            return Some(candidate);
        }
        len = (len + size).checked_sub(actual)?;
    }
    None
}

/// `None` for anything the SDK drops (it logs the reason and size) and for a layout version this
/// plugin does not know. The data is untrusted; newer versions are ignored rather than misread.
pub fn decode(data: &[u8]) -> Option<Received> {
    accept(bovnar::on_document::<TestMessage>(data)?, data.len())
}

fn accept(received: bovnar::Received<TestMessage>, size: usize) -> Option<Received> {
    if received.version > VERSION {
        return None;
    }
    let m = received.message;
    Some(Received {
        seq: m.seq,
        size,
        intact: checksum(&m.body) == m.checksum,
    })
}

/// A payload that is not a Bovnar document, sent raw to prove the SDK drops it while the host
/// delivers the bytes unchanged (spec 09 AC-9.42).
pub fn malformed() -> Vec<u8> {
    (0..300).map(|i| (i % 251) as u8 ^ 0xA5).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_plugin_sdk::bovnar::{Error, MUMBLE_LIMIT};

    /// `decode` without the SDK's log line, which needs the host and cannot run natively.
    fn decode(data: &[u8]) -> Option<Received> {
        accept(bovnar::decode::<TestMessage>(data).ok()?, data.len())
    }

    fn bytes(seq: u32, size: usize) -> Vec<u8> {
        bovnar::encode(&message(seq, size).expect("fits"), RELAY_LIMIT).expect("encodes")
    }

    #[test]
    fn documents_have_the_exact_size() {
        for size in [60, 900, MUMBLE_LIMIT, RELAY_LIMIT] {
            let data = bytes(7, size);
            assert_eq!(data.len(), size);
            assert_eq!(
                decode(&data),
                Some(Received {
                    seq: 7,
                    size,
                    intact: true
                })
            );
        }
    }

    #[test]
    fn an_oversized_message_does_not_encode() {
        assert_eq!(
            bovnar::encode(&oversized(1), RELAY_LIMIT).unwrap_err(),
            Error::TooLarge
        );
    }

    #[test]
    fn damage_foreign_data_and_newer_versions_are_detected() {
        let mut m = message(1, 900).expect("fits");
        m.checksum = "00000000".to_owned();
        let damaged = bovnar::encode(&m, RELAY_LIMIT).expect("encodes");
        assert!(!decode(&damaged).expect("still a test message").intact);
        assert_eq!(decode(b"hello"), None);
        assert_eq!(decode(&malformed()), None);
        assert_eq!(
            bovnar::decode::<TestMessage>(&malformed()).unwrap_err(),
            Error::Malformed
        );
    }

    #[test]
    fn unknown_fields_are_ignored_and_newer_layouts_are_not_misread() {
        #[derive(Serialize, Deserialize)]
        struct Newer {
            seq: u32,
            checksum: String,
            body: String,
            added_later: String,
        }
        impl Message for Newer {
            fn version() -> u32 {
                2
            }
        }
        let body = "abc".to_owned();
        let newer = Newer {
            seq: 3,
            checksum: checksum(&body),
            body,
            added_later: "x".to_owned(),
        };
        let data = bovnar::encode(&newer, RELAY_LIMIT).expect("encodes");
        let parsed = bovnar::decode::<TestMessage>(&data).expect("unknown field is ignored");
        assert_eq!((parsed.version, parsed.message.seq), (2, 3));
        assert_eq!(accept(parsed, data.len()), None);
    }
}
