//! One QR code of a transfer:
//! `'M' | version(1) | transfer id(4) | index(1) | total(1) | chunk | CRC-32(4, little endian)`,
//! written as Base45 text. The checksum covers everything before it, so a part that a scanner
//! library read or transcoded wrongly is noticed at once instead of after the last code.

use super::envelope::{FORMAT_VERSION, ID_LEN};
use super::{TransferError, base45};

/// Largest part a reader accepts, in bytes before the text encoding (1,536 characters).
pub const MAX_PART_BYTES: usize = 1024;

/// Bytes of a part that are not chunk: marker, version, id, index, total and checksum.
pub(crate) const PART_OVERHEAD: usize = 2 + ID_LEN + 2 + CRC_LEN;
const PART_MAGIC: u8 = b'M';
const CRC_LEN: usize = 4;
const MAX_PART_CHARS: usize = MAX_PART_BYTES / 2 * 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Part {
    pub id: [u8; ID_LEN],
    pub index: u8,
    pub total: u8,
    pub chunk: Vec<u8>,
}

impl Part {
    pub(crate) fn encode(&self) -> String {
        let mut bytes = Vec::with_capacity(PART_OVERHEAD + self.chunk.len());
        bytes.push(PART_MAGIC);
        bytes.push(FORMAT_VERSION);
        bytes.extend_from_slice(&self.id);
        bytes.push(self.index);
        bytes.push(self.total);
        bytes.extend_from_slice(&self.chunk);
        let checksum = crc32fast::hash(&bytes);
        bytes.extend_from_slice(&checksum.to_le_bytes());
        base45::encode(&bytes)
    }

    /// Decodes what a scanner returned. The text is untrusted: a phone scans whatever is shown
    /// to it.
    pub(crate) fn decode(text: &str) -> Result<Self, TransferError> {
        // Scanners may add a line end.
        let text = text.trim_matches(['\r', '\n']);
        if text.len() > MAX_PART_CHARS {
            return Err(TransferError::MalformedPart);
        }
        let bytes = base45::decode(text).ok_or(TransferError::MalformedPart)?;
        if bytes.len() <= PART_OVERHEAD || bytes[0] != PART_MAGIC {
            return Err(TransferError::MalformedPart);
        }
        let (body, checksum) = bytes.split_at(bytes.len() - CRC_LEN);
        if crc32fast::hash(body).to_le_bytes() != checksum {
            return Err(TransferError::ChecksumMismatch);
        }
        if body[1] != FORMAT_VERSION {
            return Err(TransferError::UnsupportedVersion);
        }
        let mut id = [0u8; ID_LEN];
        id.copy_from_slice(&body[2..2 + ID_LEN]);
        let index = body[2 + ID_LEN];
        let total = body[3 + ID_LEN];
        if index >= total {
            return Err(TransferError::MalformedPart);
        }
        Ok(Self {
            id,
            index,
            total,
            chunk: body[4 + ID_LEN..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn part() -> Part {
        Part {
            id: [9, 8, 7, 6],
            index: 2,
            total: 5,
            chunk: (0..=255).collect(),
        }
    }

    #[test]
    fn round_trips_as_qr_alphanumeric_text() {
        let text = part().encode();
        assert!(
            text.bytes()
                .all(|b| b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:".contains(&b))
        );
        assert_eq!(Part::decode(&text), Ok(part()));
        assert_eq!(Part::decode(&format!("{text}\r\n")), Ok(part()));
    }

    #[test]
    fn the_largest_part_is_accepted_and_a_larger_one_is_not() {
        let mut large = part();
        large.chunk = vec![0xAB; MAX_PART_BYTES - PART_OVERHEAD];
        assert_eq!(Part::decode(&large.encode()), Ok(large.clone()));
        large.chunk.push(0);
        large.chunk.push(0);
        assert_eq!(
            Part::decode(&large.encode()),
            Err(TransferError::MalformedPart)
        );
    }

    #[test]
    fn refuses_foreign_and_damaged_text() {
        for text in ["", "https://example.org", "HELLO WORLD", "BB8"] {
            assert_eq!(
                Part::decode(text),
                Err(TransferError::MalformedPart),
                "{text:?}"
            );
        }
        let raw = |change: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = base45::decode(&part().encode()).expect("base45");
            change(&mut bytes);
            Part::decode(&base45::encode(&bytes))
        };
        assert_eq!(
            raw(&|bytes| bytes[20] ^= 1),
            Err(TransferError::ChecksumMismatch)
        );
        assert_eq!(
            raw(&|bytes| *bytes.last_mut().expect("checksum") ^= 1),
            Err(TransferError::ChecksumMismatch)
        );
        assert_eq!(
            raw(&|bytes| bytes.truncate(bytes.len() - 2)),
            Err(TransferError::ChecksumMismatch)
        );
        assert_eq!(
            raw(&|bytes| bytes[0] = b'X'),
            Err(TransferError::MalformedPart)
        );

        // Well-formed and checksummed, but not acceptable.
        let mut newer = base45::decode(&part().encode()).expect("base45");
        newer.truncate(newer.len() - CRC_LEN);
        newer[1] = FORMAT_VERSION + 1;
        let checksum = crc32fast::hash(&newer);
        newer.extend_from_slice(&checksum.to_le_bytes());
        assert_eq!(
            Part::decode(&base45::encode(&newer)),
            Err(TransferError::UnsupportedVersion)
        );
        let mut beyond = part();
        beyond.index = 5;
        assert_eq!(
            Part::decode(&beyond.encode()),
            Err(TransferError::MalformedPart)
        );
        let mut empty = part();
        empty.chunk.clear();
        assert_eq!(
            Part::decode(&empty.encode()),
            Err(TransferError::MalformedPart)
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn decoding_never_panics(text in "[0-9A-Z $%*+./:-]{0,200}") {
            let _ = Part::decode(&text);
        }

        #[test]
        fn decoding_never_panics_on_any_bytes(
            bytes in proptest::collection::vec(any::<u8>(), 0..64),
        ) {
            let _ = Part::decode(&base45::encode(&bytes));
        }
    }
}
