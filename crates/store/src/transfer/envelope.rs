//! The encrypted stream of a transfer: a 44-byte header followed by the ciphertext.
//!
//! ```text
//! header = "MCT" | version(1) | transfer id(4) | salt(16) | memory KiB(4) | passes(1) | lanes(1)
//!        | code digits(1) | created at, unix seconds(8) | part count(1) | ciphertext length(4)
//! ```
//!
//! Integers are little endian. The ciphertext is the compressed payload sealed with
//! ChaCha20-Poly1305 (16-byte tag appended); the header is its associated data.

use argon2::{Algorithm, Argon2, Block, Params, Version};
use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec_with_limit;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use zeroize::{Zeroize, Zeroizing};

use super::payload::{MAX_PLAINTEXT_BYTES, TransferPayload};
use super::{CLOCK_SKEW_SECS, MAX_CODE_DIGITS, MIN_CODE_DIGITS, TransferError, VALIDITY_SECS};

pub(crate) const HEADER_LEN: usize = 44;
pub(crate) const FORMAT_VERSION: u8 = 1;
pub(crate) const ID_LEN: usize = 4;
pub(crate) const SALT_LEN: usize = 16;
const MAGIC: &[u8; 3] = b"MCT";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
/// DEFLATE never grows data by more than a few bytes per 64 KiB; this bounds the ciphertext a
/// reader accepts before it has decrypted anything.
const MAX_CIPHERTEXT_BYTES: usize = MAX_PLAINTEXT_BYTES + 1024;
const DEFLATE_LEVEL: u8 = 9;

/// Cost of the key derivation (Argon2id, one lane). Carried in the header of every transfer,
/// so the default can change without a new format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Working memory in KiB.
    pub memory_kib: u32,
    /// Passes over the memory.
    pub passes: u8,
}

impl KdfParams {
    /// 128 MiB, 4 passes: about 0.2 s on a desktop PC and one to two seconds on a mid-range
    /// phone. More memory would cost an attacker more, but a phone that cannot get the memory
    /// fails the import, which is worse.
    pub const DEFAULT: Self = Self {
        memory_kib: 128 * 1024,
        passes: 4,
    };
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The parameters a writer may use and a reader accepts. The lower bound keeps a forged or
/// buggy transfer from switching the slow derivation off, the upper bound keeps one from
/// exhausting the phone's memory or time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KdfBand {
    pub min_memory_kib: u32,
    pub max_memory_kib: u32,
    pub min_passes: u8,
    pub max_passes: u8,
}

impl KdfBand {
    pub(crate) const PRODUCTION: Self = Self {
        min_memory_kib: 64 * 1024,
        max_memory_kib: 256 * 1024,
        min_passes: 3,
        max_passes: 8,
    };

    /// Lets tests derive keys in microseconds; never used outside tests.
    #[cfg(test)]
    pub(crate) const TEST: Self = Self {
        min_memory_kib: 8,
        max_memory_kib: 256 * 1024,
        min_passes: 1,
        max_passes: 8,
    };

    pub(crate) fn allows(&self, params: KdfParams) -> bool {
        (self.min_memory_kib..=self.max_memory_kib).contains(&params.memory_kib)
            && (self.min_passes..=self.max_passes).contains(&params.passes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    pub id: [u8; ID_LEN],
    pub salt: [u8; SALT_LEN],
    pub kdf: KdfParams,
    pub code_digits: u8,
    pub created_at: u64,
    pub total_parts: u8,
    pub ciphertext_len: u32,
}

impl Header {
    pub(crate) fn to_bytes(self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..3].copy_from_slice(MAGIC);
        out[3] = FORMAT_VERSION;
        out[4..8].copy_from_slice(&self.id);
        out[8..24].copy_from_slice(&self.salt);
        out[24..28].copy_from_slice(&self.kdf.memory_kib.to_le_bytes());
        out[28] = self.kdf.passes;
        out[29] = 1;
        out[30] = self.code_digits;
        out[31..39].copy_from_slice(&self.created_at.to_le_bytes());
        out[39] = self.total_parts;
        out[40..44].copy_from_slice(&self.ciphertext_len.to_le_bytes());
        out
    }

    /// Parses and checks an untrusted header. It is not authenticated yet at this point; the
    /// decryption later fails if any byte of it was changed.
    pub(crate) fn parse(bytes: &[u8], band: &KdfBand) -> Result<Self, TransferError> {
        let bytes: &[u8; HEADER_LEN] = bytes
            .get(..HEADER_LEN)
            .and_then(|head| head.try_into().ok())
            .ok_or(TransferError::Malformed)?;
        if &bytes[0..3] != MAGIC {
            return Err(TransferError::Malformed);
        }
        if bytes[3] != FORMAT_VERSION {
            return Err(TransferError::UnsupportedVersion);
        }
        let header = Self {
            id: array(bytes, 4),
            salt: array(bytes, 8),
            kdf: KdfParams {
                memory_kib: u32::from_le_bytes(array(bytes, 24)),
                passes: bytes[28],
            },
            code_digits: bytes[30],
            created_at: u64::from_le_bytes(array(bytes, 31)),
            total_parts: bytes[39],
            ciphertext_len: u32::from_le_bytes(array(bytes, 40)),
        };
        let lanes = bytes[29];
        let ciphertext_len = header.ciphertext_len as usize;
        if lanes != 1
            || !band.allows(header.kdf)
            || !(MIN_CODE_DIGITS..=MAX_CODE_DIGITS).contains(&header.code_digits)
            || header.total_parts == 0
            || !(TAG_LEN..=MAX_CIPHERTEXT_BYTES).contains(&ciphertext_len)
        {
            return Err(TransferError::Malformed);
        }
        Ok(header)
    }

    /// The validity rule of the reader, see the module documentation.
    pub(crate) fn check_time(&self, now: u64) -> Result<(), TransferError> {
        if self.created_at > now.saturating_add(CLOCK_SKEW_SECS) {
            return Err(TransferError::ClockSkew);
        }
        let last_valid = self
            .created_at
            .saturating_add(VALIDITY_SECS + CLOCK_SKEW_SECS);
        if now > last_valid {
            return Err(TransferError::Expired);
        }
        Ok(())
    }
}

/// The `N` header bytes from `start`; every caller passes a field that lies inside the header.
fn array<const N: usize>(bytes: &[u8; HEADER_LEN], start: usize) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[start..start + N]);
    out
}

/// Length of the stream (header and ciphertext) for a compressed payload of `compressed` bytes.
pub(crate) fn stream_len(compressed: usize) -> usize {
    HEADER_LEN + compressed + TAG_LEN
}

pub(crate) fn compress(payload: &TransferPayload) -> Result<Zeroizing<Vec<u8>>, TransferError> {
    let plain = payload.encode()?;
    Ok(Zeroizing::new(compress_to_vec(&plain, DEFLATE_LEVEL)))
}

/// Encrypts `compressed` under `code` and returns header and ciphertext as one stream.
/// `header.ciphertext_len` must already be `compressed.len()` plus the tag.
pub(crate) fn seal(
    header: Header,
    code: &str,
    mut compressed: Zeroizing<Vec<u8>>,
) -> Result<Vec<u8>, TransferError> {
    let header_bytes = header.to_bytes();
    let (key, nonce) = derive(code, &header)?;
    key.seal_in_place_append_tag(nonce, Aad::from(&header_bytes), &mut *compressed)
        .map_err(|_| TransferError::Malformed)?;
    let mut stream = Vec::with_capacity(HEADER_LEN + compressed.len());
    stream.extend_from_slice(&header_bytes);
    stream.extend_from_slice(&compressed);
    Ok(stream)
}

/// What opening a complete stream with a code can end in.
pub(crate) enum Opened {
    Payload(TransferPayload),
    /// The decryption failed: the code is wrong (or the stream was changed, which the part
    /// checksums make unlikely).
    WrongCode,
}

/// Decrypts, decompresses and decodes. `Err` means the content is authentic but unusable, or
/// that the key derivation failed.
pub(crate) fn open(
    header: &Header,
    code: &str,
    ciphertext: &[u8],
) -> Result<Opened, TransferError> {
    let (key, nonce) = derive(code, header)?;
    let mut buffer = Zeroizing::new(ciphertext.to_vec());
    let Ok(compressed) = key.open_in_place(nonce, Aad::from(&header.to_bytes()), &mut buffer)
    else {
        return Ok(Opened::WrongCode);
    };
    let plain = decompress_to_vec_with_limit(compressed, MAX_PLAINTEXT_BYTES)
        .map(Zeroizing::new)
        .map_err(|_| TransferError::Malformed)?;
    TransferPayload::decode(&plain).map(Opened::Payload)
}

/// Argon2id over the code: 32 bytes of key and 12 bytes of nonce. Salt and key are used for one
/// message only, so the derived nonce can never repeat under a key.
fn derive(code: &str, header: &Header) -> Result<(LessSafeKey, Nonce), TransferError> {
    let params = Params::new(
        header.kdf.memory_kib,
        u32::from(header.kdf.passes),
        1,
        Some(KEY_LEN + NONCE_LEN),
    )
    .map_err(|_| TransferError::Malformed)?;
    // The working memory is allocated here, not inside the library, so that a phone short on
    // memory gets an error instead of the process being ended.
    let block_count = params.block_count();
    let mut memory: Vec<Block> = Vec::new();
    memory
        .try_reserve_exact(block_count)
        .map_err(|_| TransferError::OutOfMemory)?;
    memory.resize(block_count, Block::default());

    let mut output = Zeroizing::new([0u8; KEY_LEN + NONCE_LEN]);
    let result = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(
            code.as_bytes(),
            &header.salt,
            &mut output[..],
            &mut memory,
        );
    // The last blocks determine the key.
    memory.zeroize();
    result.map_err(|_| TransferError::Malformed)?;

    let (key, nonce) = output.split_at(KEY_LEN);
    let key = UnboundKey::new(&CHACHA20_POLY1305, key).map_err(|_| TransferError::Malformed)?;
    let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| TransferError::Malformed)?;
    Ok((LessSafeKey::new(key), nonce))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Instant;

    use super::super::payload::tests::sample;
    use super::*;

    /// Fast parameters for tests that are not about the key derivation itself.
    pub(crate) const FAST_KDF: KdfParams = KdfParams {
        memory_kib: 8,
        passes: 1,
    };

    pub(crate) fn header(compressed: usize) -> Header {
        Header {
            id: [1, 2, 3, 4],
            salt: [7; SALT_LEN],
            kdf: FAST_KDF,
            code_digits: 6,
            created_at: 1_800_000_000,
            total_parts: 3,
            ciphertext_len: (compressed + TAG_LEN) as u32,
        }
    }

    fn sealed() -> (Header, Vec<u8>) {
        let compressed = compress(&sample(1, Some(2))).expect("compress");
        let header = header(compressed.len());
        let stream = seal(header, "123456", compressed).expect("seal");
        (header, stream)
    }

    #[test]
    fn header_round_trips_and_is_44_bytes() {
        let header = header(100);
        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(Header::parse(&bytes, &KdfBand::TEST), Ok(header));
        assert_eq!(
            Header::parse(&bytes[..HEADER_LEN - 1], &KdfBand::TEST),
            Err(TransferError::Malformed)
        );
    }

    #[test]
    fn seals_and_opens() {
        let (header, stream) = sealed();
        assert_eq!(stream.len(), HEADER_LEN + header.ciphertext_len as usize);
        match open(&header, "123456", &stream[HEADER_LEN..]).expect("open") {
            Opened::Payload(payload) => assert_eq!(payload, sample(1, Some(2))),
            Opened::WrongCode => panic!("the right code was refused"),
        }
        assert!(matches!(
            open(&header, "123457", &stream[HEADER_LEN..]),
            Ok(Opened::WrongCode)
        ));
    }

    #[test]
    fn every_header_byte_is_authenticated() {
        let (header, stream) = sealed();
        let original = header.to_bytes();
        for index in 0..HEADER_LEN {
            let mut bytes = original;
            bytes[index] ^= 1;
            // A changed header either fails the checks or, if it still parses, the decryption.
            if let Ok(changed) = Header::parse(&bytes, &KdfBand::TEST) {
                assert_ne!(changed, header, "byte {index}");
                assert!(
                    matches!(
                        open(&changed, "123456", &stream[HEADER_LEN..]),
                        Ok(Opened::WrongCode)
                    ),
                    "byte {index} is not authenticated"
                );
            }
        }
    }

    #[test]
    fn every_ciphertext_byte_is_authenticated() {
        let (header, stream) = sealed();
        let ciphertext = &stream[HEADER_LEN..];
        // Every 7th byte keeps the test short; the tag covers all of them alike.
        for index in (0..ciphertext.len())
            .step_by(7)
            .chain([ciphertext.len() - 1])
        {
            let mut changed = ciphertext.to_vec();
            changed[index] ^= 0x80;
            assert!(
                matches!(open(&header, "123456", &changed), Ok(Opened::WrongCode)),
                "byte {index}"
            );
        }
        assert!(matches!(
            open(&header, "123456", &ciphertext[..ciphertext.len() - 1]),
            Ok(Opened::WrongCode)
        ));
    }

    #[test]
    fn the_production_band_refuses_weak_and_huge_parameters() {
        let parse = |kdf: KdfParams| {
            let mut header = header(100);
            header.kdf = kdf;
            Header::parse(&header.to_bytes(), &KdfBand::PRODUCTION)
        };
        assert!(parse(KdfParams::DEFAULT).is_ok());
        assert!(
            parse(KdfParams {
                memory_kib: 256 * 1024,
                passes: 8
            })
            .is_ok()
        );
        for (memory_kib, passes) in [
            (8, 1),
            (64 * 1024 - 1, 4),
            (128 * 1024, 2),
            (256 * 1024 + 1, 4),
            (128 * 1024, 9),
            (u32::MAX, 255),
        ] {
            assert_eq!(
                parse(KdfParams { memory_kib, passes }),
                Err(TransferError::Malformed),
                "{memory_kib} KiB, {passes} passes"
            );
        }
    }

    #[test]
    fn header_checks() {
        let good = header(100).to_bytes();
        let parse = |index: usize, value: u8| {
            let mut bytes = good;
            bytes[index] = value;
            Header::parse(&bytes, &KdfBand::TEST)
        };
        assert_eq!(parse(0, b'X'), Err(TransferError::Malformed), "magic");
        assert_eq!(parse(3, 2), Err(TransferError::UnsupportedVersion));
        assert_eq!(parse(29, 2), Err(TransferError::Malformed), "lanes");
        assert_eq!(
            parse(30, 5),
            Err(TransferError::Malformed),
            "code too short"
        );
        assert_eq!(
            parse(30, 11),
            Err(TransferError::Malformed),
            "code too long"
        );
        assert_eq!(parse(39, 0), Err(TransferError::Malformed), "no parts");
        assert_eq!(
            parse(43, 0xff),
            Err(TransferError::Malformed),
            "huge ciphertext"
        );
    }

    #[test]
    fn validity_edges() {
        let header = header(100);
        let created = header.created_at;
        assert_eq!(header.check_time(created), Ok(()));
        assert_eq!(
            header.check_time(created + VALIDITY_SECS + CLOCK_SKEW_SECS),
            Ok(())
        );
        assert_eq!(
            header.check_time(created + VALIDITY_SECS + CLOCK_SKEW_SECS + 1),
            Err(TransferError::Expired)
        );
        // The phone's clock is behind the desktop's.
        assert_eq!(header.check_time(created - CLOCK_SKEW_SECS), Ok(()));
        assert_eq!(
            header.check_time(created - CLOCK_SKEW_SECS - 1),
            Err(TransferError::ClockSkew)
        );
        let mut extreme = header;
        extreme.created_at = u64::MAX;
        assert_eq!(extreme.check_time(0), Err(TransferError::ClockSkew));
        assert_eq!(extreme.check_time(u64::MAX), Ok(()));
    }

    #[test]
    fn a_decompression_bomb_is_refused() {
        // Authentic (sealed with the right code) but it inflates past the limit.
        let bomb = Zeroizing::new(compress_to_vec(&vec![0u8; MAX_PLAINTEXT_BYTES + 1], 9));
        let header = header(bomb.len());
        let stream = seal(header, "123456", bomb).expect("seal");
        assert!(matches!(
            open(&header, "123456", &stream[HEADER_LEN..]),
            Err(TransferError::Malformed)
        ));
    }

    /// Run with `cargo test -p mumcord-store --release -- --ignored kdf_timing --nocapture`.
    #[test]
    #[ignore = "measures the key derivation; needs a release build and 128 MiB of memory"]
    fn kdf_timing() {
        let mut header = header(100);
        header.kdf = KdfParams::DEFAULT;
        let best = (0..3)
            .map(|_| {
                let start = Instant::now();
                derive("123456", &header).expect("derive");
                start.elapsed()
            })
            .min()
            .expect("three runs");
        println!(
            "Argon2id, {} MiB, {} passes, 1 lane: {} ms",
            header.kdf.memory_kib / 1024,
            header.kdf.passes,
            best.as_millis()
        );
        assert!(best.as_secs() < 5, "the key derivation is unusably slow");
    }
}
