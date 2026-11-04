//! End-to-end encryption of broadcast media frames (spec 07, FR-7.43 and FR-7.44).
//!
//! The sharer encrypts every video frame and share-sound packet with the current share key,
//! SFrame-style as in RFC 9605 with the cipher suite `AES_128_GCM_SHA256_128`:
//!
//! - Each share key is a random 16-byte base key with a one-byte key id. The AEAD key and salt
//!   come from the RFC 9605 key schedule (HKDF-SHA256 with the labels `"SFrame 1.0 Secret key "`
//!   / `"SFrame 1.0 Secret salt "` + key id as 8 bytes + suite `0x0004`).
//! - The RFC's key id and counter are the `key_id` and `sequence` fields of the clear
//!   [`Header`] from `mumcord_p2p::broadcast`, so no SFrame header of its own is added. The
//!   nonce is the salt XOR the sequence, and the whole 14-byte clear header is the additional
//!   authenticated data: the relay can read it but not change it unnoticed.
//! - A unit on the wire is the clear header followed by the ciphertext and the 16-byte tag.
//!
//! [`FrameSealer`] (sharer) keeps one sequence counter for video and audio, so a key id and
//! sequence pair never repeats; it refuses to seal once the counter is used up instead of
//! reusing a nonce. [`FrameSealer::rekey`] stages a new key (key id + 1), which is handed to the
//! remaining viewers through the key exchange (`keyx`); the sealer switches to it at the next
//! video keyframe, so viewers always start a key on a decodable frame. [`FrameOpener`] (viewer)
//! holds the few keys it was given; once a frame decrypts with a newer key, the older keys are
//! dropped.
//!
//! Limitation: there is no replay window. The relay could resend or reorder old frames of the
//! current key; that only causes picture or sound glitches, it can't reveal or forge media.

use mumcord_p2p::broadcast::{Content, HEADER_LEN, Header, HeaderError};
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hkdf::{HKDF_SHA256, KeyType, Salt};
use ring::rand::{SecureRandom, SystemRandom};

/// Length of a base key and of the derived AES-128 key.
pub const KEY_LEN: usize = 16;
/// Bytes the encryption adds to a frame (the AES-GCM tag).
pub const TAG_LEN: usize = 16;
/// RFC 9605 cipher suite `AES_128_GCM_SHA256_128`.
const CIPHER_SUITE: u16 = 0x0004;
/// Most keys a viewer keeps at once: the current one plus keys staged by quick rekeys in a row.
const MAX_OPENER_KEYS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("no secure random numbers available")]
    Random,
    #[error("the frame counter is used up; the share must restart")]
    CounterExhausted,
    #[error("control messages are not encrypted by the frame layer")]
    NotMedia,
    #[error("invalid broadcast header: {0}")]
    Header(#[from] HeaderError),
    #[error("payload size does not match the header")]
    SizeMismatch,
    #[error("no key with id {0}")]
    UnknownKey(u8),
    #[error("the frame failed authentication")]
    Authentication,
}

/// One share key as handed to a viewer: its id and the base key the frame key is derived from.
#[derive(Clone, PartialEq, Eq)]
pub struct ShareKey {
    pub id: u8,
    pub base: [u8; KEY_LEN],
}

impl std::fmt::Debug for ShareKey {
    // The base key never ends up in a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShareKey")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl ShareKey {
    /// A fresh random key with the given id.
    pub fn generate(id: u8) -> Result<Self, CryptoError> {
        let mut base = [0u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut base)
            .map_err(|_| CryptoError::Random)?;
        Ok(Self { id, base })
    }
}

/// The AEAD key and salt derived from one share key.
struct FrameKey {
    id: u8,
    aead: LessSafeKey,
    salt: [u8; NONCE_LEN],
}

impl FrameKey {
    fn derive(key: &ShareKey) -> Self {
        let (aead_key, salt) = derive_key_salt(u64::from(key.id), &key.base);
        Self {
            id: key.id,
            aead: aead_key,
            salt,
        }
    }

    fn nonce(&self, sequence: u32) -> Nonce {
        Nonce::assume_unique_for_key(nonce(&self.salt, u64::from(sequence)))
    }
}

struct Len(usize);

impl KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

/// RFC 9605 section 4.4.2 `derive_key_salt` for `AES_128_GCM_SHA256_128`.
fn derive_key_salt(kid: u64, base_key: &[u8]) -> (LessSafeKey, [u8; NONCE_LEN]) {
    let (key, salt) = derive_key_salt_bytes(kid, base_key);
    let aead = UnboundKey::new(&AES_128_GCM, &key).expect("AES-128 takes a 16-byte key");
    (LessSafeKey::new(aead), salt)
}

fn derive_key_salt_bytes(kid: u64, base_key: &[u8]) -> ([u8; KEY_LEN], [u8; NONCE_LEN]) {
    let secret = Salt::new(HKDF_SHA256, &[]).extract(base_key);
    let mut suffix = [0u8; 10];
    suffix[..8].copy_from_slice(&kid.to_be_bytes());
    suffix[8..].copy_from_slice(&CIPHER_SUITE.to_be_bytes());
    let mut key = [0u8; KEY_LEN];
    let mut salt = [0u8; NONCE_LEN];
    // Expanding to 16 or 12 bytes is far below HKDF-SHA256's 8160-byte limit, so it can't fail.
    secret
        .expand(&[b"SFrame 1.0 Secret key ", &suffix], Len(KEY_LEN))
        .and_then(|okm| okm.fill(&mut key))
        .expect("HKDF output length is valid");
    secret
        .expand(&[b"SFrame 1.0 Secret salt ", &suffix], Len(NONCE_LEN))
        .and_then(|okm| okm.fill(&mut salt))
        .expect("HKDF output length is valid");
    (key, salt)
}

fn nonce(salt: &[u8; NONCE_LEN], counter: u64) -> [u8; NONCE_LEN] {
    let mut out = *salt;
    for (byte, ctr) in out[NONCE_LEN - 8..].iter_mut().zip(counter.to_be_bytes()) {
        *byte ^= ctr;
    }
    out
}

/// Encrypts the sharer's media units.
pub struct FrameSealer {
    current: FrameKey,
    pending: Option<ShareKey>,
    /// The share key behind `current`, for viewers admitted later.
    current_share: ShareKey,
    next_sequence: Option<u32>,
}

impl FrameSealer {
    /// Starts a share with a fresh key (key id 0).
    pub fn new() -> Result<Self, CryptoError> {
        let key = ShareKey::generate(0)?;
        Ok(Self {
            current: FrameKey::derive(&key),
            pending: None,
            current_share: key,
            next_sequence: Some(0),
        })
    }

    /// The keys a newly admitted viewer needs: the current key and, during a rekey, the staged
    /// one.
    pub fn keys(&self) -> Vec<ShareKey> {
        let mut keys = vec![self.current_share.clone()];
        keys.extend(self.pending.clone());
        keys
    }

    /// Id of the key frames are sealed with right now.
    pub fn current_key_id(&self) -> u8 {
        self.current.id
    }

    /// Stages a new key after a viewer left (FR-7.44) and returns it, to be sent to every
    /// remaining viewer. The sealer switches to it at the next video keyframe. A rekey while
    /// another is still staged replaces the staged key, so the viewer who just left never gets
    /// the key that will be used.
    pub fn rekey(&mut self) -> Result<ShareKey, CryptoError> {
        let last = self.pending.as_ref().map_or(self.current.id, |key| key.id);
        let key = ShareKey::generate(last.wrapping_add(1))?;
        self.pending = Some(key.clone());
        Ok(key)
    }

    /// Encrypts one encoded frame into `out` (cleared first) as clear header + ciphertext + tag.
    /// A video keyframe switches to a staged key first. Returns the header that was written.
    pub fn seal(
        &mut self,
        content: Content,
        frame: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<Header, CryptoError> {
        if matches!(content, Content::Control(_)) {
            return Err(CryptoError::NotMedia);
        }
        let sequence = self.next_sequence.ok_or(CryptoError::CounterExhausted)?;
        let payload_len = u32::try_from(frame.len() + TAG_LEN)
            .map_err(|_| CryptoError::Header(HeaderError::BadSize(u32::MAX)))?;
        let keyframe = matches!(content, Content::Video { keyframe: true, .. });
        let switch = keyframe && self.pending.is_some();
        let key_id = if switch {
            self.pending.as_ref().map_or(self.current.id, |key| key.id)
        } else {
            self.current.id
        };
        let header = Header {
            content,
            key_id,
            sequence,
            payload_len,
        };
        let clear = header.encode()?;
        if switch && let Some(key) = self.pending.take() {
            self.current = FrameKey::derive(&key);
            self.current_share = key;
        }
        out.clear();
        out.reserve(HEADER_LEN + frame.len() + TAG_LEN);
        out.extend_from_slice(&clear);
        out.extend_from_slice(frame);
        let tag = self
            .current
            .aead
            .seal_in_place_separate_tag(
                self.current.nonce(sequence),
                Aad::from(clear),
                &mut out[HEADER_LEN..],
            )
            .map_err(|_| CryptoError::Authentication)?;
        out.extend_from_slice(tag.as_ref());
        self.next_sequence = sequence.checked_add(1);
        Ok(header)
    }
}

/// Decrypts media units on the viewer side.
#[derive(Default)]
pub struct FrameOpener {
    /// Oldest first.
    keys: Vec<FrameKey>,
}

impl FrameOpener {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a key from the key exchange. A key with the same id replaces the old one; beyond
    /// the limit the oldest key is dropped.
    pub fn add_key(&mut self, key: &ShareKey) {
        self.keys.retain(|known| known.id != key.id);
        if self.keys.len() == MAX_OPENER_KEYS {
            self.keys.remove(0);
        }
        self.keys.push(FrameKey::derive(key));
    }

    /// Ids of the keys held, oldest first.
    pub fn key_ids(&self) -> Vec<u8> {
        self.keys.iter().map(|key| key.id).collect()
    }

    /// Checks and decrypts one unit (clear header + ciphertext + tag) in place and returns its
    /// header and the decrypted frame. A frame that decrypts with a newer key retires all
    /// older ones.
    pub fn open<'a>(&mut self, unit: &'a mut [u8]) -> Result<(Header, &'a [u8]), CryptoError> {
        let header = Header::decode(unit)?;
        if matches!(header.content, Content::Control(_)) {
            return Err(CryptoError::NotMedia);
        }
        let (clear, payload) = unit.split_at_mut(HEADER_LEN);
        if usize::try_from(header.payload_len).ok() != Some(payload.len())
            || payload.len() < TAG_LEN
        {
            return Err(CryptoError::SizeMismatch);
        }
        let index = self
            .keys
            .iter()
            .position(|key| key.id == header.key_id)
            .ok_or(CryptoError::UnknownKey(header.key_id))?;
        let key = &self.keys[index];
        let plain_len = key
            .aead
            .open_in_place(key.nonce(header.sequence), Aad::from(&*clear), payload)
            .map_err(|_| CryptoError::Authentication)?
            .len();
        self.keys.drain(..index);
        Ok((header, &payload[..plain_len]))
    }
}

#[cfg(test)]
mod tests {
    use mumcord_p2p::broadcast::VideoCodec;

    use super::*;

    fn unhex(text: &str) -> Vec<u8> {
        let text: String = text.split_whitespace().collect();
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
            .collect()
    }

    fn video(keyframe: bool) -> Content {
        Content::Video {
            codec: VideoCodec::Av1,
            keyframe,
            temporal_layer: 0,
        }
    }

    /// RFC 9605 appendix C.3, cipher suite 0x0004.
    #[test]
    fn matches_the_rfc_9605_test_vector() {
        let base = unhex("000102030405060708090a0b0c0d0e0f");
        let (key, salt) = derive_key_salt_bytes(0x123, &base);
        assert_eq!(key.to_vec(), unhex("d34f547f4ca4f9a7447006fe7fcbf768"));
        assert_eq!(salt.to_vec(), unhex("75234edefe07819026751816"));
        assert_eq!(
            nonce(&salt, 0x4567).to_vec(),
            unhex("75234edefe07819026755d71")
        );

        let (aead, salt) = derive_key_salt(0x123, &base);
        let aad = unhex("99012345674945544620534672616d65205747");
        let mut data = unhex("64726166742d696574662d736672616d652d656e63");
        aead.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce(&salt, 0x4567)),
            Aad::from(&aad),
            &mut data,
        )
        .expect("seal");
        // The RFC's ct starts with its 5-byte SFrame header `9901234567`.
        assert_eq!(
            data,
            unhex("b7412c2513a1b66dbb48841bbaf17f598751176ad847681a69c6d0b091c07018ce4adb34eb")
        );
    }

    fn opener_for(sealer: &FrameSealer) -> FrameOpener {
        let mut opener = FrameOpener::new();
        for key in sealer.keys() {
            opener.add_key(&key);
        }
        opener
    }

    #[test]
    fn round_trip_video_and_audio_share_one_counter() {
        let mut sealer = FrameSealer::new().expect("sealer");
        let mut opener = opener_for(&sealer);
        let mut unit = Vec::new();

        let header = sealer
            .seal(video(true), b"frame one", &mut unit)
            .expect("seal");
        assert_eq!(header.sequence, 0);
        assert_eq!(unit.len(), HEADER_LEN + 9 + TAG_LEN);
        assert!(
            !unit.windows(9).any(|w| w == b"frame one"),
            "payload is encrypted"
        );
        let (opened, frame) = opener.open(&mut unit).expect("open");
        assert_eq!(opened, header);
        assert_eq!(frame, b"frame one");

        let header = sealer
            .seal(Content::Audio, b"opus", &mut unit)
            .expect("seal");
        assert_eq!(header.sequence, 1);
        assert_eq!(opener.open(&mut unit).expect("open").1, b"opus");
    }

    #[test]
    fn tampering_is_detected() {
        let mut sealer = FrameSealer::new().expect("sealer");
        let mut opener = opener_for(&sealer);
        let mut unit = Vec::new();
        sealer
            .seal(video(false), b"payload", &mut unit)
            .expect("seal");

        // Header byte 2 (keyframe flag), a payload byte and the last tag byte.
        for index in [2, HEADER_LEN + 1, unit.len() - 1] {
            let mut bad = unit.clone();
            bad[index] ^= 1;
            assert_eq!(
                opener.open(&mut bad).expect_err("must fail"),
                CryptoError::Authentication
            );
        }
        let mut short = unit[..unit.len() - 1].to_vec();
        assert_eq!(
            opener.open(&mut short).expect_err("must fail"),
            CryptoError::SizeMismatch
        );
        assert_eq!(opener.open(&mut unit).expect("intact unit").1, b"payload");
    }

    #[test]
    fn rejects_control_units_and_unknown_keys() {
        let mut sealer = FrameSealer::new().expect("sealer");
        let mut unit = Vec::new();
        let control = Content::Control(mumcord_p2p::broadcast::ControlKind::KeyframeRequest);
        assert_eq!(
            sealer.seal(control, b"", &mut unit).expect_err("must fail"),
            CryptoError::NotMedia
        );
        sealer.seal(Content::Audio, b"x", &mut unit).expect("seal");
        assert_eq!(
            FrameOpener::new().open(&mut unit).expect_err("must fail"),
            CryptoError::UnknownKey(0)
        );
    }

    #[test]
    fn rekey_switches_at_the_next_keyframe_and_locks_out_the_old_key() {
        let mut sealer = FrameSealer::new().expect("sealer");
        let mut stays = opener_for(&sealer);
        let mut leaves = opener_for(&sealer);
        let old_key = sealer.keys()[0].clone();
        let mut unit = Vec::new();

        let new_key = sealer.rekey().expect("rekey");
        assert_eq!(new_key.id, 1);
        stays.add_key(&new_key);

        // Until the next keyframe the old key stays in use.
        for content in [video(false), Content::Audio] {
            let header = sealer.seal(content, b"old", &mut unit).expect("seal");
            assert_eq!(header.key_id, 0);
            assert_eq!(leaves.open(&mut unit.clone()).expect("old key").1, b"old");
            assert_eq!(stays.open(&mut unit).expect("old key").1, b"old");
        }

        let header = sealer.seal(video(true), b"new", &mut unit).expect("seal");
        assert_eq!(header.key_id, 1);
        assert_eq!(sealer.current_key_id(), 1);
        assert_eq!(sealer.keys(), vec![new_key]);
        assert_eq!(
            leaves.open(&mut unit.clone()).expect_err("must fail"),
            CryptoError::UnknownKey(1)
        );
        assert_eq!(stays.open(&mut unit).expect("new key").1, b"new");
        assert_eq!(stays.key_ids(), vec![1], "the old key is retired");

        // Even a former viewer who relabels the old key with the new id can't open new frames.
        sealer
            .seal(Content::Audio, b"after", &mut unit)
            .expect("seal");
        let mut former = FrameOpener::new();
        former.add_key(&ShareKey {
            id: 1,
            base: old_key.base,
        });
        assert_eq!(
            former.open(&mut unit).expect_err("must fail"),
            CryptoError::Authentication
        );
    }

    #[test]
    fn a_second_rekey_before_the_switch_replaces_the_staged_key() {
        let mut sealer = FrameSealer::new().expect("sealer");
        let first = sealer.rekey().expect("rekey");
        let second = sealer.rekey().expect("rekey");
        assert_eq!((first.id, second.id), (1, 2));
        assert_eq!(sealer.keys().len(), 2);
        assert_eq!(sealer.keys()[1], second);

        let mut viewer = FrameOpener::new();
        viewer.add_key(&second);
        let mut unit = Vec::new();
        assert_eq!(
            sealer
                .seal(video(true), b"k", &mut unit)
                .expect("seal")
                .key_id,
            2
        );
        assert_eq!(viewer.open(&mut unit).expect("open").1, b"k");
    }

    #[test]
    fn stops_before_the_counter_wraps() {
        let mut sealer = FrameSealer::new().expect("sealer");
        sealer.next_sequence = Some(u32::MAX);
        let mut unit = Vec::new();
        assert_eq!(
            sealer
                .seal(Content::Audio, b"a", &mut unit)
                .expect("last")
                .sequence,
            u32::MAX
        );
        assert_eq!(
            sealer
                .seal(Content::Audio, b"a", &mut unit)
                .expect_err("must fail"),
            CryptoError::CounterExhausted
        );
    }

    #[test]
    fn opener_keeps_a_bounded_number_of_keys() {
        let mut opener = FrameOpener::new();
        for id in 0..6 {
            opener.add_key(&ShareKey::generate(id).expect("key"));
        }
        assert_eq!(opener.key_ids(), vec![2, 3, 4, 5]);
    }

    #[test]
    fn debug_hides_the_base_key() {
        let key = ShareKey {
            id: 3,
            base: [0xab; KEY_LEN],
        };
        assert!(!format!("{key:?}").to_lowercase().contains("ab"));
    }
}
