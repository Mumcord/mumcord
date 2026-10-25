//! The plugin-data messages of a full-image transfer and the reference sent with the chat
//! message.
//!
//! Every message is binary and starts with a one-byte kind and the transfer id the requester
//! picked, so many chunks fit under the server's [`PLUGIN_DATA_MAX`]:
//!
//! | kind | message       | rest                                   |
//! |------|---------------|----------------------------------------|
//! | 1    | `Get`         | content hash (32 bytes)                |
//! | 2    | `Queued`      | –                                      |
//! | 3    | `Start`       | size in bytes (u32)                    |
//! | 4    | `Chunk`       | chunk index (u32), up to [`CHUNK_LEN`] bytes |
//! | 5    | `Unavailable` | –                                      |
//!
//! Integers are big-endian. Everything received is untrusted: decoding rejects unknown kinds,
//! wrong lengths and oversized chunks.

use mumcord_protocol::PLUGIN_DATA_MAX;
use sha2::{Digest, Sha256};

/// Plugin data id of full-image transfers; other ids are ignored.
pub const DATA_ID: &str = "mumcord.image";
/// Image bytes per chunk: the largest round size that keeps a chunk under the server's limit.
pub const CHUNK_LEN: usize = 960;
/// Kind, transfer id and chunk index.
const CHUNK_HEADER: usize = 9;
const _: () = assert!(CHUNK_HEADER + CHUNK_LEN <= PLUGIN_DATA_MAX);

/// HTML attribute on the chat message's `<img>` that carries the [`FullRef`]. Official clients
/// ignore unknown attributes, so they simply show the thumbnail.
pub const ATTRIBUTE: &str = "data-mumcord-full";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// Asks the sender of a message for the full image with this content hash.
    Get {
        id: u32,
        hash: [u8; 32],
    },
    /// The request waits behind other transfers.
    Queued {
        id: u32,
    },
    /// The transfer begins; `size` bytes follow in chunks.
    Start {
        id: u32,
        size: u32,
    },
    Chunk {
        id: u32,
        index: u32,
        data: Vec<u8>,
    },
    /// The image is not (or no longer) available, or the sender is busy.
    Unavailable {
        id: u32,
    },
}

impl Msg {
    pub fn id(&self) -> u32 {
        match self {
            Self::Get { id, .. }
            | Self::Queued { id }
            | Self::Start { id, .. }
            | Self::Chunk { id, .. }
            | Self::Unavailable { id } => *id,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let (kind, rest): (u8, Vec<u8>) = match self {
            Self::Get { hash, .. } => (1, hash.to_vec()),
            Self::Queued { .. } => (2, Vec::new()),
            Self::Start { size, .. } => (3, size.to_be_bytes().to_vec()),
            Self::Chunk { index, data, .. } => {
                let mut rest = index.to_be_bytes().to_vec();
                rest.extend_from_slice(data);
                (4, rest)
            }
            Self::Unavailable { .. } => (5, Vec::new()),
        };
        let mut out = Vec::with_capacity(5 + rest.len());
        out.push(kind);
        out.extend_from_slice(&self.id().to_be_bytes());
        out.extend_from_slice(&rest);
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        let (&kind, rest) = data.split_first()?;
        let id = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?);
        let rest = &rest[4..];
        let u32_at = |bytes: &[u8]| -> Option<u32> {
            Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
        };
        match kind {
            1 if rest.len() == 32 => Some(Self::Get {
                id,
                hash: rest.try_into().ok()?,
            }),
            2 if rest.is_empty() => Some(Self::Queued { id }),
            3 if rest.len() == 4 => Some(Self::Start {
                id,
                size: u32_at(rest)?,
            }),
            4 if rest.len() > 4 && rest.len() <= 4 + CHUNK_LEN => Some(Self::Chunk {
                id,
                index: u32_at(rest)?,
                data: rest[4..].to_vec(),
            }),
            5 if rest.is_empty() => Some(Self::Unavailable { id }),
            _ => None,
        }
    }
}

/// The chunks a transfer of `size` bytes is split into.
pub fn chunk_count(size: usize) -> usize {
    size.div_ceil(CHUNK_LEN)
}

/// Identifies a full image: its SHA-256 and size. Written as `<64 hex digits>:<size>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FullRef {
    pub hash: [u8; 32],
    pub size: u32,
}

impl FullRef {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            hash: Sha256::digest(bytes).into(),
            size: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
        }
    }

    /// Whether `bytes` are exactly the image this reference names.
    pub fn matches(&self, bytes: &[u8]) -> bool {
        usize::try_from(self.size).is_ok_and(|size| size == bytes.len())
            && Sha256::digest(bytes).as_slice() == self.hash
    }

    pub fn hash_hex(&self) -> String {
        self.hash.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Accepts only the exact format [`Self::to_string`] writes, with a size up to `max`.
    pub fn parse(text: &str, max: usize) -> Option<Self> {
        let (hex, size) = text.split_once(':')?;
        if hex.len() != 64 || size.is_empty() || size.len() > 10 {
            return None;
        }
        if !size.bytes().all(|b| b.is_ascii_digit()) || size.starts_with('0') {
            return None;
        }
        let size: u32 = size.parse().ok()?;
        if usize::try_from(size).ok()? > max {
            return None;
        }
        let mut hash = [0u8; 32];
        for (i, byte) in hash.iter_mut().enumerate() {
            let pair = hex.get(2 * i..2 * i + 2)?;
            if !pair
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return None;
            }
            *byte = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(Self { hash, size })
    }
}

impl std::fmt::Display for FullRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.hash_hex(), self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let messages = [
            Msg::Get {
                id: 7,
                hash: [9; 32],
            },
            Msg::Queued { id: 1 },
            Msg::Start {
                id: u32::MAX,
                size: 123_456,
            },
            Msg::Chunk {
                id: 2,
                index: 40,
                data: vec![1; CHUNK_LEN],
            },
            Msg::Unavailable { id: 3 },
        ];
        for msg in messages {
            let encoded = msg.encode();
            assert!(encoded.len() <= PLUGIN_DATA_MAX, "{msg:?}");
            assert_eq!(Msg::decode(&encoded), Some(msg));
        }
    }

    #[test]
    fn broken_messages_are_rejected() {
        let chunk = Msg::Chunk {
            id: 1,
            index: 0,
            data: vec![0; CHUNK_LEN],
        }
        .encode();
        let mut oversized = chunk.clone();
        oversized.push(0);
        for data in [
            &[][..],
            &[1, 0, 0],
            &[9, 0, 0, 0, 1],
            &[1, 0, 0, 0, 1, 2, 3],
            &[2, 0, 0, 0, 1, 0],
            &[3, 0, 0, 0, 1, 0, 0],
            &[4, 0, 0, 0, 1, 0, 0, 0, 0],
            &oversized,
        ] {
            assert_eq!(Msg::decode(data), None, "{data:?}");
        }
    }

    #[test]
    fn chunks_cover_the_whole_image() {
        assert_eq!(chunk_count(0), 0);
        assert_eq!(chunk_count(1), 1);
        assert_eq!(chunk_count(CHUNK_LEN), 1);
        assert_eq!(chunk_count(CHUNK_LEN + 1), 2);
    }

    #[test]
    fn references_round_trip_and_verify() {
        let r = FullRef::of(b"image bytes");
        let text = r.to_string();
        assert_eq!(text.len(), 64 + 1 + 2);
        assert_eq!(FullRef::parse(&text, 1000), Some(r));
        assert!(r.matches(b"image bytes"));
        assert!(!r.matches(b"image bytez"));
        assert!(!r.matches(b"image bytes!"));
    }

    #[test]
    fn only_exact_references_parse() {
        let hex = "ab".repeat(32);
        assert!(FullRef::parse(&format!("{hex}:10"), 10).is_some());
        for bad in [
            format!("{hex}:11"),
            format!("{hex}:"),
            format!("{hex}:010"),
            format!("{hex}:-1"),
            format!("{hex}:1 "),
            format!("{}:10", "AB".repeat(32)),
            format!("{}:10", "ab".repeat(31)),
            format!("{}g:10", "ab".repeat(31) + "a"),
            format!("{hex}10"),
            "\"><script>:1".to_owned(),
        ] {
            assert_eq!(FullRef::parse(&bad, 10), None, "{bad}");
        }
    }
}
