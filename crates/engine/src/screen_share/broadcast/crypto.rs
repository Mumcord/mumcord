//! End-to-end encryption of broadcast media (spec 07, FR-7.43, FR-7.44 and FR-7.92).
//!
//! Two wire layouts share one RFC 9605 key schedule, cipher suite and rekey flow:
//!
//! - **Media profile (FR-7.92, new-format links).** A relay track message is one clear flags
//!   byte ([`mumcord_p2p::media::FLAG_KEYFRAME`] is the only known bit), an RFC 9605 SFrame
//!   header ([`SFrameHeader`]: compact key id and counter) and the SFrame ciphertext of an
//!   8-byte big-endian capture timestamp in microseconds followed by the encoded frame. The
//!   additional authenticated data is `varint(channel) | flags | SFrame header`, so the relay
//!   can route by channel and react to keyframes, but it cannot change the channel, the flags or
//!   the header unnoticed, and it never sees the timestamp or the frame. [`TrackSealer`] and
//!   [`TrackOpener`] implement this layout. [`encode_direct`] and [`decode_direct`] implement
//!   the direct stream layout of the same profile: the same clear flags and timestamp, no SFrame
//!   layer, because the QUIC TLS connection already protects it.
//! - **Legacy clear header (FR-7.43, old-format links).** The RFC's key id and counter are the
//!   `key_id` and `sequence` fields of the clear [`Header`] from `mumcord_p2p::broadcast`, and
//!   that whole 14-byte header is the additional authenticated data. A unit is the clear header
//!   followed by the ciphertext and the 16-byte tag. [`FrameSealer`] and [`FrameOpener`] keep
//!   this layout for old peers and old relays (FR-7.98); new code uses the media profile.
//!
//! # Keys, nonce and rekey
//!
//! Each share key is a random 16-byte base key with a one-byte key id. The AEAD key and salt
//! come from the RFC 9605 key schedule (HKDF-SHA256 with the labels `"SFrame 1.0 Secret key "` /
//! `"SFrame 1.0 Secret salt "` + key id as 8 bytes + suite `0x0004`). The nonce is the salt XOR
//! the counter. The sealer keeps one counter sequence for all its messages, so a key id and
//! counter pair never repeats; it refuses to seal once the counter is used up instead of reusing
//! a nonce.
//!
//! [`TrackSealer::rekey`] and [`FrameSealer::rekey`] stage a new key (key id + 1), which is
//! handed to the remaining viewers through the key exchange (`keyx`); the sealer switches to it
//! at the next video keyframe, so viewers always start a key on a decodable frame. A viewer that
//! leaves gets no new key. The opener holds the few keys it was given; once a frame decrypts
//! with a newer key, the older keys are dropped. `keys()` hands the current and, during a rekey,
//! the staged key to a late joiner.
//!
//! The media profile delivers messages in order (FR-7.96), so [`TrackOpener`] rejects a counter
//! at or below the last one it opened with that key: a replayed or too-old frame cannot make it
//! show an old picture again. The legacy opener keeps its documented limitation and has no
//! replay window. Errors and logs never contain key material or media content.

use mumcord_p2p::broadcast::{Content, HEADER_LEN, Header, HeaderError};
use mumcord_p2p::media::{self, ChannelUse, MediaError};
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hkdf::{HKDF_SHA256, KeyType, Salt};
use ring::rand::{SecureRandom, SystemRandom};

/// Length of a base key and of the derived AES-128 key.
pub const KEY_LEN: usize = 16;
/// Bytes the encryption adds to a frame (the AES-GCM tag).
pub const TAG_LEN: usize = 16;
/// Bytes of the capture timestamp in front of every track frame (FR-7.92).
pub const TIMESTAMP_LEN: usize = 8;
/// RFC 9605 cipher suite `AES_128_GCM_SHA256_128`.
const CIPHER_SUITE: u16 = 0x0004;
/// Most keys a viewer keeps at once: the current one plus keys staged by quick rekeys in a row.
const MAX_OPENER_KEYS: usize = 4;
/// Longest additional authenticated data of a track message: channel varint, flags byte and
/// SFrame header.
const TRACK_AAD_MAX: usize = 3 + 1 + SFrameHeader::MAX_LEN;

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
    UnknownKey(u64),
    #[error("the frame failed authentication")]
    Authentication,
    #[error("channel {0} does not carry a media track")]
    NotATrackChannel(u16),
    #[error("the flags byte has bits outside the profile")]
    BadFlags,
    #[error("malformed SFrame header")]
    BadSFrameHeader,
    #[error("the counter was replayed or is too old")]
    Replay,
    #[error("the track message is over its profile limit")]
    TooLarge,
    #[error("malformed track message")]
    BadTrack,
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

    fn nonce(&self, counter: u64) -> Nonce {
        Nonce::assume_unique_for_key(nonce(&self.salt, counter))
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

// ---------------------------------------------------------------------------------------------
// Media profile (FR-7.92)
// ---------------------------------------------------------------------------------------------

/// An RFC 9605 SFrame header: compact key id and counter (FR-7.92).
///
/// The header is one config byte followed by the values that do not fit inline. A value below 8
/// travels in three config bits; a larger value is appended as the minimum number of big-endian
/// bytes, with the config field holding the length minus one. Our profile hands out key ids in
/// 0 to 255; the type carries `u64` values because the RFC does, so the conformance vector can
/// be encoded exactly and an out-of-profile key id can be reported as received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SFrameHeader {
    pub key_id: u64,
    pub counter: u64,
}

impl SFrameHeader {
    /// Longest header the RFC allows: a config byte, an 8-byte key id and an 8-byte counter.
    pub const MAX_LEN: usize = 17;

    /// Writes the header into `out` and returns its length.
    pub fn encode(&self, out: &mut [u8; Self::MAX_LEN]) -> usize {
        let mut config = 0u8;
        let key_len = encode_header_field(self.key_id, 0x80, 4, &mut config, out, 1);
        let counter_at = 1 + key_len;
        let counter_len = encode_header_field(self.counter, 0x08, 0, &mut config, out, counter_at);
        out[0] = config;
        counter_at + counter_len
    }

    /// Parses a header from the front of `input` and returns it with the number of bytes it
    /// used. Non-minimal encodings are refused, so every key id and counter has exactly one
    /// valid header, which the AAD then binds.
    pub fn decode(input: &[u8]) -> Result<(Self, usize), CryptoError> {
        let &config = input.first().ok_or(CryptoError::BadSFrameHeader)?;
        let mut at = 1;
        let key_id = decode_header_field(config, 0x80, 4, input, &mut at)?;
        let counter = decode_header_field(config, 0x08, 0, input, &mut at)?;
        Ok((Self { key_id, counter }, at))
    }
}

/// Encodes one header value; returns the bytes appended after the config byte.
fn encode_header_field(
    value: u64,
    extended_flag: u8,
    shift: u32,
    config: &mut u8,
    out: &mut [u8; SFrameHeader::MAX_LEN],
    at: usize,
) -> usize {
    if value < 8 {
        *config |= (value as u8) << shift;
        return 0;
    }
    let bytes = value.to_be_bytes();
    let first = bytes.iter().position(|&byte| byte != 0).unwrap_or(8);
    let len = 8 - first;
    *config |= extended_flag | (((len - 1) as u8) << shift);
    out[at..at + len].copy_from_slice(&bytes[first..]);
    len
}

/// Decodes one header value; returns the value and advances `at` past its bytes.
fn decode_header_field(
    config: u8,
    extended_flag: u8,
    shift: u32,
    input: &[u8],
    at: &mut usize,
) -> Result<u64, CryptoError> {
    let field = u64::from((config >> shift) & 0x07);
    if config & extended_flag == 0 {
        return Ok(field);
    }
    let len = usize::try_from(field).expect("three bits fit usize") + 1;
    let end = at.checked_add(len).ok_or(CryptoError::BadSFrameHeader)?;
    let bytes = input.get(*at..end).ok_or(CryptoError::BadSFrameHeader)?;
    // The RFC requires the minimum number of bytes, so a leading zero is invalid, and a
    // one-byte value that fit the config bits (below 8) is invalid too.
    if bytes[0] == 0 || (len == 1 && bytes[0] < 8) {
        return Err(CryptoError::BadSFrameHeader);
    }
    let value = bytes
        .iter()
        .fold(0u64, |value, &byte| (value << 8) | u64::from(byte));
    *at = end;
    Ok(value)
}

/// The unsigned LEB128 encoding of a channel, as `bovnar::codec` writes it (FR-7.89).
fn channel_varint(channel: u16) -> ([u8; 3], usize) {
    let mut out = [0u8; 3];
    let mut value = u32::from(channel);
    let mut len = 0;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out[len] = if value == 0 { byte } else { byte | 0x80 };
        len += 1;
        if value == 0 {
            return (out, len);
        }
    }
}

/// Builds the AAD of a track message: `varint(channel) | flags | SFrame header`.
fn track_aad<'a>(
    buf: &'a mut [u8; TRACK_AAD_MAX],
    channel: u16,
    flags: u8,
    header: &[u8],
) -> &'a [u8] {
    let (varint, len) = channel_varint(channel);
    buf[..len].copy_from_slice(&varint[..len]);
    buf[len] = flags;
    let end = len + 1 + header.len();
    buf[len + 1..end].copy_from_slice(header);
    &buf[..end]
}

/// Checks that `channel` carries a track message: a video layer, audio or plugin data.
fn check_track_channel(channel: u16) -> Result<(), CryptoError> {
    match media::channel_use(channel) {
        Some(ChannelUse::VideoLayer(_) | ChannelUse::Audio | ChannelUse::PluginData) => Ok(()),
        Some(ChannelUse::Control) => Err(CryptoError::NotMedia),
        Some(ChannelUse::Reserved) | None => Err(CryptoError::NotATrackChannel(channel)),
    }
}

fn is_video_channel(channel: u16) -> bool {
    matches!(media::channel_use(channel), Some(ChannelUse::VideoLayer(_)))
}

/// Checks a whole track message against its channel's profile cap (FR-7.94).
fn check_track_size(channel: u16, len: usize) -> Result<(), CryptoError> {
    match media::message_limit(channel) {
        Some(limit) if len > limit => Err(CryptoError::TooLarge),
        Some(_) => Ok(()),
        None => Err(CryptoError::NotATrackChannel(channel)),
    }
}

fn flags_of(keyframe: bool) -> u8 {
    if keyframe { media::FLAG_KEYFRAME } else { 0 }
}

/// Encrypts the sharer's track messages on a new-format relay link (FR-7.92).
pub struct TrackSealer {
    current: FrameKey,
    pending: Option<ShareKey>,
    /// The share key behind `current`, for viewers admitted later.
    current_share: ShareKey,
    next_counter: Option<u64>,
}

impl TrackSealer {
    /// Starts a share with a fresh key (key id 0).
    pub fn new() -> Result<Self, CryptoError> {
        let key = ShareKey::generate(0)?;
        Ok(Self {
            current: FrameKey::derive(&key),
            pending: None,
            current_share: key,
            next_counter: Some(0),
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

    /// Writes one relay track message to `out` (cleared first): the flags byte, the SFrame
    /// header, then the encrypted timestamp and frame. Returns the header that was written.
    /// A video keyframe switches to a staged key first.
    pub fn seal(
        &mut self,
        channel: u16,
        keyframe: bool,
        timestamp_micros: u64,
        frame: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<SFrameHeader, CryptoError> {
        check_track_channel(channel)?;
        let counter = self.next_counter.ok_or(CryptoError::CounterExhausted)?;
        // Only a video keyframe is a point where every viewer can start a new key (FR-7.44).
        let switch = keyframe && is_video_channel(channel) && self.pending.is_some();
        let key_id = if switch {
            self.pending.as_ref().map_or(self.current.id, |key| key.id)
        } else {
            self.current.id
        };
        let header = SFrameHeader {
            key_id: u64::from(key_id),
            counter,
        };
        let mut header_bytes = [0u8; SFrameHeader::MAX_LEN];
        let header_len = header.encode(&mut header_bytes);
        let flags = flags_of(keyframe);
        let total = 1usize
            .checked_add(header_len)
            .and_then(|len| len.checked_add(TIMESTAMP_LEN))
            .and_then(|len| len.checked_add(frame.len()))
            .and_then(|len| len.checked_add(TAG_LEN))
            .ok_or(CryptoError::TooLarge)?;
        check_track_size(channel, total)?;
        let mut aad_buf = [0u8; TRACK_AAD_MAX];
        let aad = track_aad(&mut aad_buf, channel, flags, &header_bytes[..header_len]);
        if switch && let Some(key) = self.pending.take() {
            self.current = FrameKey::derive(&key);
            self.current_share = key;
        }
        out.clear();
        out.reserve(total);
        out.push(flags);
        out.extend_from_slice(&header_bytes[..header_len]);
        out.extend_from_slice(&timestamp_micros.to_be_bytes());
        out.extend_from_slice(frame);
        let tag = self
            .current
            .aead
            .seal_in_place_separate_tag(
                self.current.nonce(counter),
                Aad::from(aad),
                &mut out[1 + header_len..],
            )
            .map_err(|_| CryptoError::Authentication)?;
        out.extend_from_slice(tag.as_ref());
        self.next_counter = counter.checked_add(1);
        Ok(header)
    }

    /// Seals one channel-0 control payload (the stream description, FR-7.95) with the current
    /// share key and a counter from the same sequence as the track messages. The SFrame header
    /// is clear so the relay can cache the ciphertext; the AAD binds channel 0 and the header.
    pub fn seal_control(
        &mut self,
        plain: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<SFrameHeader, CryptoError> {
        if plain.len() > media::MAX_DESCRIPTION {
            return Err(CryptoError::TooLarge);
        }
        let counter = self.next_counter.ok_or(CryptoError::CounterExhausted)?;
        let header = SFrameHeader {
            key_id: u64::from(self.current.id),
            counter,
        };
        let mut header_bytes = [0u8; SFrameHeader::MAX_LEN];
        let header_len = header.encode(&mut header_bytes);
        let total = header_len
            .checked_add(plain.len())
            .and_then(|len| len.checked_add(TAG_LEN))
            .ok_or(CryptoError::TooLarge)?;
        if total > media::MAX_DESCRIPTION + SFrameHeader::MAX_LEN + TAG_LEN {
            return Err(CryptoError::TooLarge);
        }
        let mut aad_buf = [0u8; TRACK_AAD_MAX];
        let aad = track_aad(
            &mut aad_buf,
            media::CHANNEL_CONTROL,
            0,
            &header_bytes[..header_len],
        );
        out.clear();
        out.reserve(total);
        out.extend_from_slice(&header_bytes[..header_len]);
        out.extend_from_slice(plain);
        let tag = self
            .current
            .aead
            .seal_in_place_separate_tag(
                self.current.nonce(counter),
                Aad::from(aad),
                &mut out[header_len..],
            )
            .map_err(|_| CryptoError::Authentication)?;
        out.extend_from_slice(tag.as_ref());
        self.next_counter = counter.checked_add(1);
        Ok(header)
    }
}

/// One decrypted relay track message (FR-7.92).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OpenedTrack<'a> {
    /// The clear flags byte's keyframe bit.
    pub keyframe: bool,
    /// The share key that decrypted the message.
    pub key_id: u8,
    /// The SFrame counter the message carried.
    pub counter: u64,
    /// The capture timestamp in microseconds, from inside the ciphertext.
    pub timestamp_micros: u64,
    /// The encoded frame.
    pub frame: &'a [u8],
}

impl std::fmt::Debug for OpenedTrack<'_> {
    // Media never ends up in a log; the metadata is enough to debug a stream.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedTrack")
            .field("keyframe", &self.keyframe)
            .field("key_id", &self.key_id)
            .field("counter", &self.counter)
            .field("timestamp_micros", &self.timestamp_micros)
            .field("frame_len", &self.frame.len())
            .finish()
    }
}

struct TrackKey {
    key: FrameKey,
    /// Highest counter opened with this key; a message at or below it is a replay.
    highest: Option<u64>,
}

/// Decrypts track messages on the viewer side of a new-format relay link (FR-7.92).
#[derive(Default)]
pub struct TrackOpener {
    /// Oldest first.
    keys: Vec<TrackKey>,
}

impl TrackOpener {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a key from the key exchange. A key with the same id replaces the old one; beyond
    /// the limit the oldest key is dropped.
    pub fn add_key(&mut self, key: &ShareKey) {
        let highest = self
            .keys
            .iter()
            .find(|known| known.key.id == key.id)
            .and_then(|known| known.highest);
        self.keys.retain(|known| known.key.id != key.id);
        if self.keys.len() == MAX_OPENER_KEYS {
            self.keys.remove(0);
        }
        self.keys.push(TrackKey {
            key: FrameKey::derive(key),
            highest,
        });
    }

    /// Ids of the keys held, oldest first.
    pub fn key_ids(&self) -> Vec<u8> {
        self.keys.iter().map(|known| known.key.id).collect()
    }

    /// Authenticates and decrypts one track message (flags, SFrame header, ciphertext and tag)
    /// in place. `channel` is the channel of the mux message that carried it; it is part of the
    /// AAD, so opening a message under another channel fails authentication. A message that
    /// decrypts with a newer key retires all older ones.
    pub fn open<'a>(
        &mut self,
        channel: u16,
        message: &'a mut [u8],
    ) -> Result<OpenedTrack<'a>, CryptoError> {
        check_track_channel(channel)?;
        check_track_size(channel, message.len())?;
        let (flags, rest) = message.split_first_mut().ok_or(CryptoError::BadTrack)?;
        let flags = *flags;
        let keyframe = media::check_flags(flags).map_err(|_: MediaError| CryptoError::BadFlags)?;
        let (header, header_len) = SFrameHeader::decode(rest)?;
        let (clear, payload) = rest.split_at_mut(header_len);
        if payload.len() < TIMESTAMP_LEN + TAG_LEN {
            return Err(CryptoError::BadTrack);
        }
        let index = self
            .keys
            .iter()
            .position(|known| u64::from(known.key.id) == header.key_id)
            .ok_or(CryptoError::UnknownKey(header.key_id))?;
        // Checked before authenticating, but only advanced after it succeeds, so a forged
        // message cannot make the opener skip a real one.
        if self.keys[index]
            .highest
            .is_some_and(|highest| header.counter <= highest)
        {
            return Err(CryptoError::Replay);
        }
        let mut aad_buf = [0u8; TRACK_AAD_MAX];
        let aad = track_aad(&mut aad_buf, channel, flags, clear);
        let key = &self.keys[index].key;
        let plain = key
            .aead
            .open_in_place(key.nonce(header.counter), Aad::from(aad), payload)
            .map_err(|_| CryptoError::Authentication)?;
        let Some((timestamp, frame)) = plain.split_first_chunk::<TIMESTAMP_LEN>() else {
            return Err(CryptoError::BadTrack);
        };
        let key_id = key.id;
        self.keys[index].highest = Some(header.counter);
        self.keys.drain(..index);
        Ok(OpenedTrack {
            keyframe,
            key_id,
            counter: header.counter,
            timestamp_micros: u64::from_be_bytes(*timestamp),
            frame,
        })
    }

    /// Opens one channel-0 control payload (the stream description, FR-7.95): SFrame header,
    /// ciphertext and tag, with the same key and counter sequence as the track messages.
    pub fn open_control<'a>(&mut self, payload: &'a mut [u8]) -> Result<&'a [u8], CryptoError> {
        if payload.len() > media::MAX_DESCRIPTION + SFrameHeader::MAX_LEN + TAG_LEN {
            return Err(CryptoError::TooLarge);
        }
        let (header, header_len) = SFrameHeader::decode(payload)?;
        let (clear, rest) = payload.split_at_mut(header_len);
        if rest.len() < TAG_LEN {
            return Err(CryptoError::BadTrack);
        }
        let index = self
            .keys
            .iter()
            .position(|known| u64::from(known.key.id) == header.key_id)
            .ok_or(CryptoError::UnknownKey(header.key_id))?;
        if self.keys[index]
            .highest
            .is_some_and(|highest| header.counter <= highest)
        {
            return Err(CryptoError::Replay);
        }
        let mut aad_buf = [0u8; TRACK_AAD_MAX];
        let aad = track_aad(&mut aad_buf, media::CHANNEL_CONTROL, 0, clear);
        let key = &self.keys[index].key;
        let plain = key
            .aead
            .open_in_place(key.nonce(header.counter), Aad::from(aad), rest)
            .map_err(|_| CryptoError::Authentication)?;
        self.keys[index].highest = Some(header.counter);
        self.keys.drain(..index);
        Ok(plain)
    }
}

/// One direct-stream track message (FR-7.92): clear flags, timestamp and frame, no SFrame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DirectTrack<'a> {
    /// The clear flags byte's keyframe bit.
    pub keyframe: bool,
    /// The capture timestamp in microseconds.
    pub timestamp_micros: u64,
    /// The encoded frame.
    pub frame: &'a [u8],
}

impl std::fmt::Debug for DirectTrack<'_> {
    // Media never ends up in a log; the metadata is enough to debug a stream.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectTrack")
            .field("keyframe", &self.keyframe)
            .field("timestamp_micros", &self.timestamp_micros)
            .field("frame_len", &self.frame.len())
            .finish()
    }
}

/// Writes one direct-stream track message to `out` (cleared first): flags, the 8-byte
/// big-endian timestamp and the frame. The QUIC TLS connection protects it, so there is no
/// SFrame layer (FR-7.92).
pub fn encode_direct(
    channel: u16,
    keyframe: bool,
    timestamp_micros: u64,
    frame: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), CryptoError> {
    check_track_channel(channel)?;
    let total = 1usize
        .checked_add(TIMESTAMP_LEN)
        .and_then(|len| len.checked_add(frame.len()))
        .ok_or(CryptoError::TooLarge)?;
    check_track_size(channel, total)?;
    out.clear();
    out.reserve(total);
    out.push(flags_of(keyframe));
    out.extend_from_slice(&timestamp_micros.to_be_bytes());
    out.extend_from_slice(frame);
    Ok(())
}

/// Reads one direct-stream track message (FR-7.92); no decryption is involved.
pub fn decode_direct(channel: u16, message: &[u8]) -> Result<DirectTrack<'_>, CryptoError> {
    check_track_channel(channel)?;
    check_track_size(channel, message.len())?;
    let (&flags, rest) = message.split_first().ok_or(CryptoError::BadTrack)?;
    let keyframe = media::check_flags(flags).map_err(|_: MediaError| CryptoError::BadFlags)?;
    let (timestamp, frame) = rest
        .split_first_chunk::<TIMESTAMP_LEN>()
        .ok_or(CryptoError::BadTrack)?;
    Ok(DirectTrack {
        keyframe,
        timestamp_micros: u64::from_be_bytes(*timestamp),
        frame,
    })
}

// ---------------------------------------------------------------------------------------------
// Legacy clear header (FR-7.43, old-format links)
// ---------------------------------------------------------------------------------------------

/// Encrypts the sharer's legacy media units.
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
    /// A video keyframe or a plugin-data unit switches to a staged key first. Returns the header
    /// that was written.
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
        // Every plugin-data unit stands alone, so it can start a new key like a keyframe.
        let keyframe = matches!(
            content,
            Content::Video { keyframe: true, .. } | Content::Data
        );
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
                self.current.nonce(u64::from(sequence)),
                Aad::from(clear),
                &mut out[HEADER_LEN..],
            )
            .map_err(|_| CryptoError::Authentication)?;
        out.extend_from_slice(tag.as_ref());
        self.next_sequence = sequence.checked_add(1);
        Ok(header)
    }
}

/// Decrypts media units on the viewer side of a legacy link.
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
    ///
    /// Limitation: there is no replay window. The relay could resend or reorder old frames of
    /// the current key; that only causes picture or sound glitches, it can't reveal or forge
    /// media.
    pub fn open<'a>(&mut self, unit: &'a mut [u8]) -> Result<(Header, &'a [u8]), CryptoError> {
        let header = Header::decode(unit)?;
        let (clear, payload) = unit.split_at_mut(HEADER_LEN);
        if matches!(header.content, Content::Control(_)) {
            return Err(CryptoError::NotMedia);
        }
        if usize::try_from(header.payload_len).ok() != Some(payload.len())
            || payload.len() < TAG_LEN
        {
            return Err(CryptoError::SizeMismatch);
        }
        let index = self
            .keys
            .iter()
            .position(|key| key.id == header.key_id)
            .ok_or(CryptoError::UnknownKey(u64::from(header.key_id)))?;
        let key = &self.keys[index];
        let plain_len = key
            .aead
            .open_in_place(
                key.nonce(u64::from(header.sequence)),
                Aad::from(&*clear),
                payload,
            )
            .map_err(|_| CryptoError::Authentication)?
            .len();
        self.keys.drain(..index);
        Ok((header, &payload[..plain_len]))
    }
}

#[cfg(test)]
mod tests {
    use mumcord_p2p::broadcast::VideoCodec;
    use mumcord_p2p::media::MediaWriter;

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

    fn timestamp() -> u64 {
        1_234_567_890
    }

    fn track_opener_for(sealer: &TrackSealer) -> TrackOpener {
        let mut opener = TrackOpener::new();
        for key in sealer.keys() {
            opener.add_key(&key);
        }
        opener
    }

    fn write_header(header: SFrameHeader) -> ([u8; SFrameHeader::MAX_LEN], usize) {
        let mut bytes = [0u8; SFrameHeader::MAX_LEN];
        let len = header.encode(&mut bytes);
        (bytes, len)
    }

    // -- RFC 9605 conformance -----------------------------------------------------------------

    /// RFC 9605 appendix C.3, cipher suite 0x0004, with the header now written by the compact
    /// encoder; the ciphertext is checked in both directions.
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

        let header = SFrameHeader {
            key_id: 0x123,
            counter: 0x4567,
        };
        let (encoded, len) = write_header(header);
        assert_eq!(&encoded[..len], &unhex("9901234567")[..]);
        assert_eq!(
            SFrameHeader::decode(&unhex("9901234567")),
            Ok((header, len))
        );

        let metadata = unhex("4945544620534672616d65205747");
        let plaintext = unhex("64726166742d696574662d736672616d652d656e63");
        let expected =
            unhex("b7412c2513a1b66dbb48841bbaf17f598751176ad847681a69c6d0b091c07018ce4adb34eb");
        let mut aad = encoded[..len].to_vec();
        aad.extend_from_slice(&metadata);

        let (aead, salt) = derive_key_salt(0x123, &base);
        let sealed_nonce = Nonce::assume_unique_for_key(nonce(&salt, 0x4567));
        let opened_nonce = Nonce::assume_unique_for_key(nonce(&salt, 0x4567));
        let mut sealed = plaintext.clone();
        aead.seal_in_place_append_tag(sealed_nonce, Aad::from(aad.as_slice()), &mut sealed)
            .expect("seal");
        assert_eq!(sealed, expected);

        let mut received = expected.clone();
        let opened = aead
            .open_in_place(opened_nonce, Aad::from(aad.as_slice()), &mut received)
            .expect("open");
        assert_eq!(opened, &plaintext[..]);
    }

    #[test]
    fn the_compact_header_covers_its_boundaries() {
        let cases: &[(u64, u64, &[u8])] = &[
            (0, 0, &[0x00]),
            (0, 7, &[0x07]),
            (7, 0, &[0x70]),
            (7, 7, &[0x77]),
            (8, 8, &[0x88, 0x08, 0x08]),
            (255, 255, &[0x88, 0xff, 0xff]),
            (256, 256, &[0x99, 0x01, 0x00, 0x01, 0x00]),
            (0x123, 0x4567, &[0x99, 0x01, 0x23, 0x45, 0x67]),
            (
                u64::MAX,
                u64::MAX,
                &[
                    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    0xff, 0xff, 0xff, 0xff,
                ],
            ),
        ];
        for &(key_id, counter, expected) in cases {
            let header = SFrameHeader { key_id, counter };
            let (encoded, len) = write_header(header);
            assert_eq!(&encoded[..len], expected, "{key_id}/{counter}");
            assert_eq!(
                SFrameHeader::decode(expected),
                Ok((header, expected.len())),
                "{key_id}/{counter}"
            );
        }
    }

    #[test]
    fn strict_header_validation_rejects_non_canonical_encodings() {
        let bad: &[&[u8]] = &[
            &[],
            &[0x80],                   // extended key id, no bytes
            &[0x88, 0x07, 0x08],       // key id 7 fits the config bits
            &[0x88, 0x00, 0x08],       // leading zero in a key id
            &[0x08, 0x00],             // extended one-byte counter that fits inline
            &[0x09, 0x00, 0x08],       // leading zero in a counter
            &[0x99, 0x01],             // key id cut off
            &[0x99, 0x01, 0x00, 0x01], // counter cut off
            &[0xf0; 8],                // 8-byte key id cut off
            &[0x8f, 0x08, 0x01],       // 8-byte counter cut off
        ];
        for bytes in bad {
            assert_eq!(
                SFrameHeader::decode(bytes),
                Err(CryptoError::BadSFrameHeader),
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn malformed_headers_never_panic() {
        let header = SFrameHeader {
            key_id: 0x123,
            counter: 0x4567,
        };
        let (encoded, len) = write_header(header);
        for end in 0..=len {
            let _ = SFrameHeader::decode(&encoded[..end]);
        }
        for config in 0..=u8::MAX {
            for len in 0..=SFrameHeader::MAX_LEN {
                let mut bytes = [0xffu8; SFrameHeader::MAX_LEN];
                bytes[0] = config;
                let _ = SFrameHeader::decode(&bytes[..len]);
            }
        }
    }

    // -- Media profile ------------------------------------------------------------------------

    #[test]
    fn a_control_description_round_trips_and_is_bound_to_channel_zero() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let description = b".v = 1; .timebase = 1; .kind = \"screen\";";
        let mut sealed = Vec::new();
        let header = sealer.seal_control(description, &mut sealed).expect("seal");
        assert_eq!(header.counter, 0);
        assert!(
            !sealed
                .windows(description.len())
                .any(|window| window == description),
            "the description is encrypted"
        );

        let mut opener = track_opener_for(&sealer);
        let mut replay = sealed.clone();
        assert_eq!(opener.open_control(&mut sealed).expect("open"), description);
        assert_eq!(opener.open_control(&mut replay), Err(CryptoError::Replay));

        // Control and track messages share one counter sequence.
        let mut track = Vec::new();
        let track_header = sealer
            .seal(
                media::CHANNEL_AUDIO,
                false,
                timestamp(),
                b"opus",
                &mut track,
            )
            .expect("track");
        assert_eq!(track_header.counter, 1);
    }

    #[test]
    fn a_track_message_round_trips_and_hides_the_timestamp_and_frame() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let frame = b"encoded frame payload";
        let mut message = Vec::new();
        let header = sealer
            .seal(
                media::CHANNEL_VIDEO_BASE,
                true,
                timestamp(),
                frame,
                &mut message,
            )
            .expect("seal");
        assert_eq!(
            header,
            SFrameHeader {
                key_id: 0,
                counter: 0
            }
        );
        // The clear part is the flags byte and the SFrame header only.
        assert_eq!(message[0], media::FLAG_KEYFRAME);
        assert_eq!(
            SFrameHeader::decode(&message[1..]),
            Ok((header, 1)),
            "a key id and counter below 8 stay in the config byte"
        );
        let mut plain = timestamp().to_be_bytes().to_vec();
        plain.extend_from_slice(frame);
        assert!(
            !message.windows(plain.len()).any(|window| window == plain),
            "the timestamp and frame are encrypted"
        );

        let mut opener = track_opener_for(&sealer);
        let opened = opener
            .open(media::CHANNEL_VIDEO_BASE, &mut message)
            .expect("open");
        assert_eq!(
            (opened.keyframe, opened.key_id, opened.counter),
            (true, 0, 0)
        );
        assert_eq!(opened.timestamp_micros, timestamp());
        assert_eq!(opened.frame, frame);

        // Video, audio and plugin data all use the same track layout.
        for (channel, keyframe) in [
            (2u16, false),
            (media::CHANNEL_AUDIO, false),
            (media::CHANNEL_PLUGIN_DATA, false),
        ] {
            let mut message = Vec::new();
            let header = sealer
                .seal(channel, keyframe, timestamp() + 1, b"payload", &mut message)
                .expect("seal");
            assert_eq!(header.key_id, 0);
            let opened = opener.open(channel, &mut message).expect("open");
            assert_eq!(opened.timestamp_micros, timestamp() + 1);
            assert_eq!(opened.frame, b"payload");
        }
    }

    #[test]
    fn aad_binds_the_channel_flags_and_sframe_header() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let mut message = Vec::new();
        sealer
            .seal(
                media::CHANNEL_VIDEO_BASE,
                true,
                timestamp(),
                b"frame",
                &mut message,
            )
            .expect("seal");

        // The same message under another track channel fails authentication.
        let mut wrong_channel = message.clone();
        assert_eq!(
            track_opener_for(&sealer).open(2, &mut wrong_channel),
            Err(CryptoError::Authentication)
        );

        // Clearing the keyframe flag breaks the AAD.
        let mut no_flag = message.clone();
        no_flag[0] = 0;
        assert_eq!(
            track_opener_for(&sealer).open(1, &mut no_flag),
            Err(CryptoError::Authentication)
        );

        // Unknown flag bits are outside the profile.
        let mut bad_flags = message.clone();
        bad_flags[0] |= 0x02;
        assert_eq!(
            track_opener_for(&sealer).open(1, &mut bad_flags),
            Err(CryptoError::BadFlags)
        );

        // The header is inline (key id 0, counter 0); changing the counter in it breaks the AAD.
        assert_eq!(message[1], 0x00);
        let mut other_counter = message.clone();
        other_counter[1] = 0x01;
        assert_eq!(
            track_opener_for(&sealer).open(1, &mut other_counter),
            Err(CryptoError::Authentication)
        );

        // Changing the header to a key id that was never handed out is refused before auth.
        let mut other_key = message.clone();
        other_key[1] = 0x10;
        assert_eq!(
            track_opener_for(&sealer).open(1, &mut other_key),
            Err(CryptoError::UnknownKey(1))
        );

        // A changed ciphertext or tag fails authentication.
        let mut tampered = message.clone();
        *tampered.last_mut().expect("tag") ^= 1;
        assert_eq!(
            track_opener_for(&sealer).open(1, &mut tampered),
            Err(CryptoError::Authentication)
        );
    }

    #[test]
    fn replayed_and_stale_counters_are_rejected() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let mut opener = track_opener_for(&sealer);
        let mut message = Vec::new();
        sealer
            .seal(1, false, timestamp(), b"one", &mut message)
            .expect("seal");
        let mut first = message.clone();
        assert_eq!(opener.open(1, &mut message).expect("first").counter, 0);
        assert_eq!(opener.open(1, &mut first), Err(CryptoError::Replay));

        sealer
            .seal(1, false, timestamp() + 1, b"two", &mut message)
            .expect("seal");
        let mut second = message.clone();
        sealer
            .seal(1, false, timestamp() + 2, b"three", &mut message)
            .expect("seal");
        assert_eq!(opener.open(1, &mut message).expect("third").counter, 2);
        assert_eq!(opener.open(1, &mut second), Err(CryptoError::Replay));

        // A forged message that fails authentication must not advance the counter.
        let mut opener = track_opener_for(&sealer);
        let mut genuine = Vec::new();
        sealer
            .seal(1, false, timestamp(), b"genuine", &mut message)
            .expect("seal");
        genuine.extend_from_slice(&message);
        let mut forged = message.clone();
        forged[2] ^= 0xff;
        assert_eq!(
            opener.open(1, &mut forged),
            Err(CryptoError::Authentication)
        );
        assert_eq!(opener.open(1, &mut genuine).expect("genuine").counter, 3);
    }

    #[test]
    fn a_rekey_switches_at_the_next_video_keyframe_and_retires_the_old_key() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let mut stays = track_opener_for(&sealer);
        let mut leaves = track_opener_for(&sealer);
        let old_key = sealer.keys()[0].clone();
        let mut message = Vec::new();

        let new_key = sealer.rekey().expect("rekey");
        assert_eq!(new_key.id, 1);
        stays.add_key(&new_key);

        // Audio and plugin data never switch the key, even if a caller sets the keyframe bit.
        for channel in [media::CHANNEL_AUDIO, media::CHANNEL_PLUGIN_DATA] {
            let header = sealer
                .seal(channel, true, timestamp(), b"old", &mut message)
                .expect("seal");
            assert_eq!(header.key_id, 0, "channel {channel} must not switch");
            let mut copy = message.clone();
            assert_eq!(
                leaves.open(channel, &mut copy).expect("old key").frame,
                b"old"
            );
        }

        // A video delta frame stays on the old key too.
        let header = sealer
            .seal(1, false, timestamp(), b"delta", &mut message)
            .expect("seal");
        assert_eq!(header.key_id, 0);
        assert_eq!(
            leaves.open(1, &mut message.clone()).expect("old key").frame,
            b"delta"
        );

        // The video keyframe switches.
        let header = sealer
            .seal(1, true, timestamp(), b"keyframe", &mut message)
            .expect("seal");
        assert_eq!(header.key_id, 1);
        assert_eq!(sealer.current_key_id(), 1);
        assert_eq!(sealer.keys(), vec![new_key.clone()]);
        assert_eq!(
            leaves.open(1, &mut message.clone()),
            Err(CryptoError::UnknownKey(1))
        );
        assert_eq!(
            stays.open(1, &mut message).expect("new key").frame,
            b"keyframe"
        );
        assert_eq!(stays.key_ids(), vec![1], "the old key is retired");

        // Even a former viewer who relabels the old key with the new id can't open new frames.
        sealer
            .seal(
                media::CHANNEL_AUDIO,
                false,
                timestamp(),
                b"after",
                &mut message,
            )
            .expect("seal");
        let mut former = TrackOpener::new();
        former.add_key(&ShareKey {
            id: 1,
            base: old_key.base,
        });
        assert_eq!(
            former.open(media::CHANNEL_AUDIO, &mut message),
            Err(CryptoError::Authentication)
        );
    }

    #[test]
    fn a_late_joiner_gets_the_current_and_staged_keys() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let current = sealer.keys()[0].clone();
        let mut message = Vec::new();
        sealer
            .seal(1, false, timestamp(), b"early", &mut message)
            .expect("seal");
        let new_key = sealer.rekey().expect("rekey");
        assert_eq!(sealer.keys(), vec![current, new_key.clone()]);

        // A viewer admitted during the rekey gets both keys through the key exchange.
        let mut late = TrackOpener::new();
        for key in sealer.keys() {
            late.add_key(&key);
        }
        sealer
            .seal(1, false, timestamp(), b"still old", &mut message)
            .expect("seal");
        assert_eq!(
            late.open(1, &mut message).expect("old key").frame,
            b"still old"
        );
        sealer
            .seal(1, true, timestamp(), b"new", &mut message)
            .expect("seal");
        assert_eq!(late.open(1, &mut message).expect("new key").frame, b"new");

        // After the switch a viewer admitted later gets the current key only.
        assert_eq!(sealer.keys(), vec![new_key]);
    }

    #[test]
    fn key_ids_outside_the_profile_are_unknown() {
        // A key id above 255 can never belong to a ShareKey, so it is reported as unknown.
        let header = SFrameHeader {
            key_id: 256,
            counter: 0,
        };
        let (encoded, len) = write_header(header);
        let mut message = vec![0u8];
        message.extend_from_slice(&encoded[..len]);
        message.extend_from_slice(&[0u8; TIMESTAMP_LEN + TAG_LEN]);
        assert_eq!(
            TrackOpener::new().open(1, &mut message),
            Err(CryptoError::UnknownKey(256))
        );
    }

    #[test]
    fn stops_before_the_track_counter_wraps() {
        let mut sealer = TrackSealer::new().expect("sealer");
        sealer.next_counter = Some(u64::MAX);
        let mut message = Vec::new();
        assert_eq!(
            sealer
                .seal(1, false, timestamp(), b"a", &mut message)
                .expect("last")
                .counter,
            u64::MAX
        );
        assert_eq!(
            sealer
                .seal(1, false, timestamp(), b"a", &mut message)
                .expect_err("must fail"),
            CryptoError::CounterExhausted
        );
    }

    #[test]
    fn track_messages_over_the_profile_caps_are_refused() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let mut out = Vec::new();
        let audio = vec![0u8; media::MAX_AUDIO_MESSAGE];
        assert_eq!(
            sealer.seal(media::CHANNEL_AUDIO, false, 0, &audio, &mut out),
            Err(CryptoError::TooLarge)
        );
        // The whole message counts: flags + header + timestamp + tag + frame.
        let audio = vec![0u8; media::MAX_AUDIO_MESSAGE - (1 + 1 + TIMESTAMP_LEN + TAG_LEN)];
        sealer
            .seal(media::CHANNEL_AUDIO, false, 0, &audio, &mut out)
            .expect("the cap itself is inclusive");
        assert_eq!(out.len(), media::MAX_AUDIO_MESSAGE);
    }

    #[test]
    fn control_and_reserved_channels_are_not_tracks() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let mut out = Vec::new();
        assert_eq!(
            sealer.seal(media::CHANNEL_CONTROL, false, 0, b"x", &mut out),
            Err(CryptoError::NotMedia)
        );
        assert_eq!(
            sealer.seal(7, false, 0, b"x", &mut out),
            Err(CryptoError::NotATrackChannel(7))
        );
        assert_eq!(
            sealer.seal(media::MAX_CHANNEL + 1, false, 0, b"x", &mut out),
            Err(CryptoError::NotATrackChannel(media::MAX_CHANNEL + 1))
        );
    }

    #[test]
    fn opening_malformed_track_messages_never_panics() {
        let mut sealer = TrackSealer::new().expect("sealer");
        let mut message = Vec::new();
        sealer
            .seal(1, true, timestamp(), b"frame", &mut message)
            .expect("seal");
        for end in 0..message.len() {
            let mut cut = message[..end].to_vec();
            let _ = track_opener_for(&sealer).open(1, &mut cut);
        }
        for index in 0..message.len() {
            for value in [0x00u8, 0x01, 0x07, 0x08, 0x7f, 0x80, 0xff] {
                let mut bad = message.clone();
                bad[index] = value;
                let _ = track_opener_for(&sealer).open(1, &mut bad);
            }
        }
    }

    #[test]
    fn errors_never_contain_media_or_key_material() {
        const MARKER: &[u8] = b"SECRET-MEDIA-MARKER";
        let mut sealer = TrackSealer::new().expect("sealer");
        let mut out = Vec::new();
        let huge = MARKER.repeat(media::MAX_AUDIO_MESSAGE);
        let error = sealer
            .seal(media::CHANNEL_AUDIO, false, 0, &huge, &mut out)
            .expect_err("too large");
        assert_eq!(error, CryptoError::TooLarge);
        let text = format!("{error} {error:?}");
        assert!(!text.contains("SECRET"));

        sealer
            .seal(1, true, timestamp(), MARKER, &mut out)
            .expect("seal");
        *out.last_mut().expect("tag") ^= 1;
        let error = track_opener_for(&sealer)
            .open(1, &mut out)
            .expect_err("authentication");
        let text = format!("{error} {error:?}").to_lowercase();
        assert!(!text.contains("secret"));
        assert!(!text.contains(&format!("{:02x}", sealer.keys()[0].base[0])));

        // A successfully opened track keeps its frame out of logs too.
        sealer
            .seal(1, true, timestamp(), MARKER, &mut out)
            .expect("seal");
        let mut opener = track_opener_for(&sealer);
        let opened = opener.open(1, &mut out).expect("open");
        assert_eq!(opened.frame, MARKER);
        assert!(!format!("{opened:?}").contains("SECRET"));
    }

    // -- Channel varint and the AAD prefix ----------------------------------------------------

    #[test]
    fn the_aad_channel_prefix_is_byte_identical_to_the_media_writer() {
        let writer = MediaWriter::new();
        for channel in [
            media::CHANNEL_CONTROL,
            media::CHANNEL_VIDEO_BASE,
            media::CHANNEL_VIDEO_BASE + 1,
            media::CHANNEL_VIDEO_BASE + 2,
            media::CHANNEL_VIDEO_LAST,
            media::CHANNEL_AUDIO,
            media::CHANNEL_PLUGIN_DATA,
        ] {
            let mut doc = Vec::new();
            writer.begin(&mut doc);
            writer
                .write_message(&mut doc, channel, b"payload")
                .expect("valid message");
            // The first chunk is a 3-byte header, then the channel varint.
            let body = &doc[writer.opening().len() + 3..];
            let (varint, len) = channel_varint(channel);
            assert_eq!(&body[..len], &varint[..len], "channel {channel}");

            let (encoded, header_len) = write_header(SFrameHeader {
                key_id: 0,
                counter: 0,
            });
            let mut aad_buf = [0u8; TRACK_AAD_MAX];
            let aad = track_aad(
                &mut aad_buf,
                channel,
                media::FLAG_KEYFRAME,
                &encoded[..header_len],
            );
            assert_eq!(&aad[..len], &body[..len], "AAD prefix of channel {channel}");
            assert_eq!(aad[len], media::FLAG_KEYFRAME);
        }

        assert_eq!(channel_varint(0), ([0x00, 0, 0], 1));
        assert_eq!(channel_varint(127), ([0x7f, 0, 0], 1));
        assert_eq!(channel_varint(128), ([0x80, 0x01, 0], 2));
        assert_eq!(channel_varint(4095), ([0xff, 0x1f, 0], 2));
        assert_eq!(channel_varint(u16::MAX), ([0xff, 0xff, 0x03], 3));
    }

    // -- Direct stream ------------------------------------------------------------------------

    #[test]
    fn the_direct_stream_is_clear_and_has_no_sframe_layer() {
        let frame = b"encoded frame";
        let mut out = Vec::new();
        encode_direct(
            media::CHANNEL_VIDEO_BASE,
            true,
            timestamp(),
            frame,
            &mut out,
        )
        .expect("encode");
        let mut expected = vec![media::FLAG_KEYFRAME];
        expected.extend_from_slice(&timestamp().to_be_bytes());
        expected.extend_from_slice(frame);
        assert_eq!(out, expected, "no SFrame header and no tag");

        let track = decode_direct(media::CHANNEL_VIDEO_BASE, &out).expect("decode");
        assert_eq!(
            (track.keyframe, track.timestamp_micros, track.frame),
            (true, timestamp(), &frame[..])
        );
        assert!(
            !format!("{track:?}").contains("encoded"),
            "the frame stays out of logs"
        );

        for channel in [media::CHANNEL_AUDIO, media::CHANNEL_PLUGIN_DATA] {
            encode_direct(channel, false, 7, b"x", &mut out).expect("encode");
            assert_eq!(
                decode_direct(channel, &out)
                    .expect("decode")
                    .timestamp_micros,
                7
            );
        }

        let mut bad = vec![0x02];
        bad.extend_from_slice(&timestamp().to_be_bytes());
        assert_eq!(
            decode_direct(media::CHANNEL_VIDEO_BASE, &bad),
            Err(CryptoError::BadFlags)
        );
        assert_eq!(
            decode_direct(media::CHANNEL_VIDEO_BASE, &[0x00; TIMESTAMP_LEN - 1]),
            Err(CryptoError::BadTrack)
        );
        assert_eq!(
            decode_direct(media::CHANNEL_CONTROL, b"x"),
            Err(CryptoError::NotMedia)
        );
        assert_eq!(
            decode_direct(7, b"x"),
            Err(CryptoError::NotATrackChannel(7))
        );
        let huge = vec![0u8; media::MAX_VIDEO_MESSAGE];
        assert_eq!(
            encode_direct(media::CHANNEL_VIDEO_BASE, false, 0, &huge, &mut out),
            Err(CryptoError::TooLarge)
        );
    }

    // -- Legacy clear header ------------------------------------------------------------------

    fn opener_for(sealer: &FrameSealer) -> FrameOpener {
        let mut opener = FrameOpener::new();
        for key in sealer.keys() {
            opener.add_key(&key);
        }
        opener
    }

    #[test]
    fn legacy_units_round_trip_video_and_audio_share_one_counter() {
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
    fn legacy_tampering_is_detected() {
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
    fn legacy_rejects_control_units_and_unknown_keys() {
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
    fn a_legacy_plugin_data_unit_switches_to_the_staged_key_at_once() {
        let mut sealer = FrameSealer::new().expect("sealer");
        let mut stays = opener_for(&sealer);
        let mut leaves = opener_for(&sealer);
        let new_key = sealer.rekey().expect("rekey");
        stays.add_key(&new_key);
        let mut unit = Vec::new();
        let header = sealer
            .seal(Content::Data, b"data", &mut unit)
            .expect("seal");
        assert_eq!(header.key_id, 1);
        assert_eq!(
            leaves.open(&mut unit.clone()).expect_err("must fail"),
            CryptoError::UnknownKey(1)
        );
        assert_eq!(stays.open(&mut unit).expect("new key").1, b"data");
    }

    #[test]
    fn legacy_rekey_switches_at_the_next_keyframe_and_locks_out_the_old_key() {
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
    fn a_second_legacy_rekey_before_the_switch_replaces_the_staged_key() {
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
    fn the_legacy_sealer_stops_before_the_counter_wraps() {
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
    fn openers_keep_a_bounded_number_of_keys() {
        let mut legacy = FrameOpener::new();
        for id in 0..6 {
            legacy.add_key(&ShareKey::generate(id).expect("key"));
        }
        assert_eq!(legacy.key_ids(), vec![2, 3, 4, 5]);

        let mut track = TrackOpener::new();
        for id in 0..6 {
            track.add_key(&ShareKey::generate(id).expect("key"));
        }
        assert_eq!(track.key_ids(), vec![2, 3, 4, 5]);
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
