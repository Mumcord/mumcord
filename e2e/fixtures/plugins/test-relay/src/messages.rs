//! The test message: a fixed header plus a patterned body, so the receiver can check that a
//! message arrived whole and in order.
//!
//! Layout (little endian): `b"TR"`, message version (u8), reserved (u8), sequence (u32), body
//! checksum (u32, FNV-1a), body.

pub const VERSION: u8 = 1;
const MAGIC: &[u8; 2] = b"TR";
const HEADER: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub struct Received {
    pub seq: u32,
    pub size: usize,
    pub intact: bool,
}

fn checksum(body: &[u8]) -> u32 {
    body.iter().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

/// A message of exactly `size` bytes (at least the header).
pub fn encode(seq: u32, size: usize) -> Vec<u8> {
    let body: Vec<u8> = (0..size.saturating_sub(HEADER))
        .map(|i| (i % 251) as u8)
        .collect();
    let mut out = Vec::with_capacity(HEADER + body.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(0);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&checksum(&body).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// `None` for anything that is not a test message of a version this plugin knows. The data is
/// untrusted; newer versions are ignored rather than misread.
pub fn decode(data: &[u8]) -> Option<Received> {
    let header = data.get(..HEADER)?;
    if &header[..2] != MAGIC || header[2] != VERSION {
        return None;
    }
    let seq = u32::from_le_bytes(header[4..8].try_into().ok()?);
    let sum = u32::from_le_bytes(header[8..12].try_into().ok()?);
    Some(Received {
        seq,
        size: data.len(),
        intact: checksum(&data[HEADER..]) == sum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_size_and_sequence() {
        for size in [HEADER, 900, 65_536] {
            let data = encode(7, size);
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
    fn damage_and_foreign_data_are_detected() {
        let mut data = encode(1, 900);
        data[500] ^= 1;
        assert!(!decode(&data).expect("still a test message").intact);
        assert_eq!(decode(b"hello"), None);
        let mut newer = encode(1, 100);
        newer[2] = VERSION + 1;
        assert_eq!(decode(&newer), None);
    }
}
