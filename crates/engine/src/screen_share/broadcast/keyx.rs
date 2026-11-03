//! Authenticated exchange of the share keys between sharer and viewer through the relay
//! (spec 07, FR-7.43 and FR-7.44, section 6.6). Pure code: the caller carries the messages as
//! `keyreq` / `key` control units and supplies the Mumble certificate hashes.
//!
//! The Mumble server is the trust anchor, as for direct connections: each side proves it holds
//! the private key of a certificate whose SHA-1 hash equals the hash the Mumble server reports
//! for that user (`UserState.hash`). The relay and the bot only pass messages on; they can't
//! read the share key and can't pose as either side.
//!
//! # Flow
//!
//! 1. The viewer calls [`request`]: a fresh X25519 key pair, and a key request signed with its
//!    Mumble certificate key. It keeps the [`PendingRequest`] and sends the message.
//! 2. The sharer calls [`answer_request`] with the request, a check that the certificate hash
//!    belongs to an admitted viewer, and the keys from `FrameSealer::keys`. It verifies the
//!    signature, makes its own X25519 key pair and derives a per-viewer wrap key (X25519 →
//!    HKDF-SHA256 → AES-128-GCM). The answer carries the share keys sealed under the wrap key
//!    and is signed with the sharer's certificate key. The sharer keeps the [`ViewerChannel`].
//! 3. The viewer calls [`PendingRequest::accept`] with the answer and the sharer's hash and
//!    gets the share keys plus a [`SharerChannel`].
//! 4. On a rekey the sharer calls [`ViewerChannel::update`] for every remaining viewer; each
//!    viewer opens it with [`SharerChannel::open_update`]. A viewer who left gets no update,
//!    so it can't decrypt anything after the switch.
//!
//! # Messages
//!
//! All numbers big-endian. Signatures use the certificate's key: RSA PKCS#1 v1.5 with SHA-256
//! (2048 to 8192 bits), ECDSA P-256/SHA-256 or P-384/SHA-384 (ASN.1), or Ed25519.
//!
//! - Request: `0x01 | broadcast id u64 | viewer X25519 public key (32) | cert length u16 |
//!   viewer cert DER | signature`, signed over `"mumcord broadcast key request v1" |
//!   broadcast id | viewer public key`.
//! - Answer: `0x02 | sharer X25519 public key (32) | cert length u16 | sharer cert DER |
//!   sealed length u16 | sealed keys | signature`, signed over
//!   `"mumcord broadcast key answer v1" | broadcast id | viewer public key | sharer public key |
//!   sealed keys`. The sealed keys use wrap counter 0.
//! - Update: `0x03 | wrap counter u32 | sealed keys`; the counter must grow with every update.
//!
//! Sealed keys are AES-128-GCM under the wrap key with the nonce `0…0 | wrap counter u32`, AAD
//! `"mumcord broadcast key answer v1"` or `"… update v1"` + broadcast id + wrap counter, and
//! the plaintext `count u8 (1–4) | count × (key id u8 | base key 16)`. The wrap key is
//! HKDF-SHA256 with salt `"mumcord broadcast key wrap v1" | broadcast id`, the X25519 shared
//! secret as input and `viewer public key | sharer public key` as info.
//!
//! # Chunks
//!
//! A certificate alone can be close to the 1 KiB limit of a `keyreq` / `key` body, so every
//! message is sent as 1 to 8 chunks made by [`split`], each at most `MAX_KEY_BODY` bytes:
//! `set id u8 | index u8 | count u8 | data`. The set id tells retries apart. The receiver puts
//! them together with a [`Reassembler`], one per sender (the slot on the sharer side); chunks may
//! arrive in any order, a chunk of a new set replaces an unfinished one, and unfinished sets are
//! dropped after [`REASSEMBLY_TIMEOUT`].

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use mumcord_p2p::BroadcastId;
use mumcord_p2p::broadcast::MAX_KEY_BODY;
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::agreement::{self, EphemeralPrivateKey, UnparsedPublicKey, X25519};
use ring::hkdf::{HKDF_SHA256, Salt};
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, Ed25519KeyPair, RsaKeyPair};
use x509_parser::oid_registry::{
    OID_EC_P256, OID_KEY_TYPE_EC_PUBLIC_KEY, OID_NIST_EC_P384, OID_PKCS1_RSAENCRYPTION,
    OID_SIG_ED25519,
};

use super::crypto::{KEY_LEN, ShareKey, TAG_LEN};

/// Bytes in front of every chunk: set id, index, count.
pub const CHUNK_HEADER_LEN: usize = 3;
/// Most chunks one message may be split into.
pub const MAX_CHUNKS: usize = 8;
/// How long an unfinished set of chunks is kept.
pub const REASSEMBLY_TIMEOUT: Duration = Duration::from_secs(10);
/// Most unfinished sets one [`Reassembler`] keeps, so a misbehaving relay can't fill memory.
const MAX_OPEN_SETS: usize = 64;
/// Most share keys in one sealed message (as many as a viewer's frame opener keeps).
const MAX_KEYS: usize = 4;
const CHUNK_DATA_LEN: usize = MAX_KEY_BODY as usize - CHUNK_HEADER_LEN;
const MAX_MESSAGE_LEN: usize = MAX_CHUNKS * CHUNK_DATA_LEN;
const X25519_LEN: usize = 32;

const TYPE_REQUEST: u8 = 1;
const TYPE_ANSWER: u8 = 2;
const TYPE_UPDATE: u8 = 3;
const LABEL_REQUEST: &[u8] = b"mumcord broadcast key request v1";
const LABEL_ANSWER: &[u8] = b"mumcord broadcast key answer v1";
const LABEL_UPDATE: &[u8] = b"mumcord broadcast key update v1";
const LABEL_WRAP: &[u8] = b"mumcord broadcast key wrap v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyxError {
    #[error("no secure random numbers available")]
    Random,
    #[error("the certificate key type is not supported")]
    UnsupportedKey,
    #[error("the private key does not belong to the certificate")]
    KeyMismatch,
    #[error("invalid certificate")]
    BadCertificate,
    #[error("invalid signature")]
    BadSignature,
    #[error("the certificate does not match the user's Mumble certificate")]
    WrongIdentity,
    #[error("the viewer is not admitted to this broadcast")]
    NotAdmitted,
    #[error("the key request is for another broadcast")]
    WrongBroadcast,
    #[error("malformed key exchange message")]
    Malformed,
    #[error("the key agreement failed")]
    KeyAgreement,
    #[error("the sealed keys failed authentication")]
    Decrypt,
    #[error("a key update was repeated or came out of order")]
    Replay,
    #[error("the wrap counter is used up")]
    CounterExhausted,
    #[error("the message is too large for {MAX_CHUNKS} chunks")]
    TooLarge,
    #[error("malformed chunk")]
    BadChunk,
    #[error("too many unfinished chunk sets")]
    TooManySets,
}

/// The user's Mumble client certificate with its private key, for signing.
pub struct CertKey {
    cert_der: Vec<u8>,
    signer: Signer,
}

enum Signer {
    Rsa(RsaKeyPair),
    Ecdsa(EcdsaKeyPair),
    Ed25519(Ed25519KeyPair),
}

impl CertKey {
    /// Loads the PKCS#8 private key of `cert_der` and checks that the two belong together.
    pub fn new(cert_der: Vec<u8>, key_pkcs8_der: &[u8]) -> Result<Self, KeyxError> {
        let rng = SystemRandom::new();
        let signer = if let Ok(key) = RsaKeyPair::from_pkcs8(key_pkcs8_der) {
            Signer::Rsa(key)
        } else if let Ok(key) = EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            key_pkcs8_der,
            &rng,
        ) {
            Signer::Ecdsa(key)
        } else if let Ok(key) = EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P384_SHA384_ASN1_SIGNING,
            key_pkcs8_der,
            &rng,
        ) {
            Signer::Ecdsa(key)
        } else if let Ok(key) = Ed25519KeyPair::from_pkcs8_maybe_unchecked(key_pkcs8_der) {
            Signer::Ed25519(key)
        } else {
            return Err(KeyxError::UnsupportedKey);
        };
        let cert_key = Self { cert_der, signer };
        let probe = b"mumcord broadcast key check";
        verify(&cert_key.cert_der, probe, &cert_key.sign(probe)?).map_err(|err| match err {
            KeyxError::BadSignature => KeyxError::KeyMismatch,
            other => other,
        })?;
        Ok(cert_key)
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, KeyxError> {
        let rng = SystemRandom::new();
        match &self.signer {
            Signer::Rsa(key) => {
                let mut sig = vec![0u8; key.public().modulus_len()];
                key.sign(&signature::RSA_PKCS1_SHA256, &rng, message, &mut sig)
                    .map_err(|_| KeyxError::UnsupportedKey)?;
                Ok(sig)
            }
            Signer::Ecdsa(key) => Ok(key
                .sign(&rng, message)
                .map_err(|_| KeyxError::Random)?
                .as_ref()
                .to_vec()),
            Signer::Ed25519(key) => Ok(key.sign(message).as_ref().to_vec()),
        }
    }
}

/// Checks `sig` over `message` with the public key of `cert_der`.
fn verify(cert_der: &[u8], message: &[u8], sig: &[u8]) -> Result<(), KeyxError> {
    let (rest, cert) =
        x509_parser::parse_x509_certificate(cert_der).map_err(|_| KeyxError::BadCertificate)?;
    if !rest.is_empty() {
        return Err(KeyxError::BadCertificate);
    }
    let spki = cert.public_key();
    let key_type = &spki.algorithm.algorithm;
    let algorithm: &dyn signature::VerificationAlgorithm = if *key_type == OID_PKCS1_RSAENCRYPTION {
        &signature::RSA_PKCS1_2048_8192_SHA256
    } else if *key_type == OID_SIG_ED25519 {
        &signature::ED25519
    } else if *key_type == OID_KEY_TYPE_EC_PUBLIC_KEY {
        let curve = spki
            .algorithm
            .parameters
            .as_ref()
            .and_then(|params| params.as_oid().ok())
            .ok_or(KeyxError::UnsupportedKey)?;
        if curve == OID_EC_P256 {
            &signature::ECDSA_P256_SHA256_ASN1
        } else if curve == OID_NIST_EC_P384 {
            &signature::ECDSA_P384_SHA384_ASN1
        } else {
            return Err(KeyxError::UnsupportedKey);
        }
    } else {
        return Err(KeyxError::UnsupportedKey);
    };
    signature::UnparsedPublicKey::new(algorithm, spki.subject_public_key.data.as_ref())
        .verify(message, sig)
        .map_err(|_| KeyxError::BadSignature)
}

fn matches_hash(cert_der: &[u8], expected: &str) -> bool {
    mumcord_store::cert_hash(cert_der).eq_ignore_ascii_case(expected)
}

/// A viewer's key request waiting for the sharer's answer.
pub struct PendingRequest {
    broadcast: BroadcastId,
    private: EphemeralPrivateKey,
    public: [u8; X25519_LEN],
}

/// Starts the key exchange as a viewer of `broadcast`. Returns the state to keep and the
/// request to send (through [`split`]).
pub fn request(
    broadcast: BroadcastId,
    me: &CertKey,
) -> Result<(PendingRequest, Vec<u8>), KeyxError> {
    let private = EphemeralPrivateKey::generate(&X25519, &SystemRandom::new())
        .map_err(|_| KeyxError::Random)?;
    let public: [u8; X25519_LEN] = private
        .compute_public_key()
        .map_err(|_| KeyxError::Random)?
        .as_ref()
        .try_into()
        .map_err(|_| KeyxError::KeyAgreement)?;
    let bid = broadcast.0.to_be_bytes();
    let sig = me.sign(&[LABEL_REQUEST, &bid, &public].concat())?;
    let mut message = Writer::new(TYPE_REQUEST);
    message.bytes(&bid);
    message.bytes(&public);
    message.with_len(&me.cert_der)?;
    message.bytes(&sig);
    Ok((
        PendingRequest {
            broadcast,
            private,
            public,
        },
        message.0,
    ))
}

/// Answers a viewer's key request as the sharer of `broadcast`. `is_admitted` gets the
/// requesting certificate's hash (lowercase hex SHA-1) and must say whether it belongs to a
/// viewer this sharer admitted. `keys` are the share keys to hand over.
pub fn answer_request(
    broadcast: BroadcastId,
    me: &CertKey,
    request: &[u8],
    is_admitted: impl FnOnce(&str) -> bool,
    keys: &[ShareKey],
) -> Result<(ViewerChannel, Vec<u8>), KeyxError> {
    let bid = broadcast.0.to_be_bytes();
    let mut reader = Reader::new(request, TYPE_REQUEST)?;
    if reader.array::<8>()? != bid {
        return Err(KeyxError::WrongBroadcast);
    }
    let viewer_public = reader.array::<X25519_LEN>()?;
    let cert = reader.with_len()?;
    let sig = reader.rest();
    verify(cert, &[LABEL_REQUEST, &bid, &viewer_public].concat(), sig)?;
    let cert_hash = mumcord_store::cert_hash(cert);
    if !is_admitted(&cert_hash) {
        return Err(KeyxError::NotAdmitted);
    }

    let private = EphemeralPrivateKey::generate(&X25519, &SystemRandom::new())
        .map_err(|_| KeyxError::Random)?;
    let sharer_public: [u8; X25519_LEN] = private
        .compute_public_key()
        .map_err(|_| KeyxError::Random)?
        .as_ref()
        .try_into()
        .map_err(|_| KeyxError::KeyAgreement)?;
    let wrap = wrap_key(
        private,
        &viewer_public,
        &viewer_public,
        &sharer_public,
        &bid,
    )?;
    let sealed = seal_keys(&wrap, LABEL_ANSWER, &bid, 0, keys)?;
    let sig = me.sign(&[LABEL_ANSWER, &bid, &viewer_public, &sharer_public, &sealed].concat())?;

    let mut message = Writer::new(TYPE_ANSWER);
    message.bytes(&sharer_public);
    message.with_len(&me.cert_der)?;
    message.with_len(&sealed)?;
    message.bytes(&sig);
    Ok((
        ViewerChannel {
            broadcast,
            wrap,
            cert_hash,
            next_counter: Some(1),
        },
        message.0,
    ))
}

impl PendingRequest {
    /// Checks the sharer's answer against the sharer's Mumble certificate hash and returns the
    /// channel for later key updates and the share keys.
    pub fn accept(
        self,
        answer: &[u8],
        sharer_hash: &str,
    ) -> Result<(SharerChannel, Vec<ShareKey>), KeyxError> {
        let bid = self.broadcast.0.to_be_bytes();
        let mut reader = Reader::new(answer, TYPE_ANSWER)?;
        let sharer_public = reader.array::<X25519_LEN>()?;
        let cert = reader.with_len()?;
        let sealed = reader.with_len()?;
        let sig = reader.rest();
        verify(
            cert,
            &[LABEL_ANSWER, &bid, &self.public, &sharer_public, sealed].concat(),
            sig,
        )?;
        if !matches_hash(cert, sharer_hash) {
            return Err(KeyxError::WrongIdentity);
        }
        let wrap = wrap_key(
            self.private,
            &sharer_public,
            &self.public,
            &sharer_public,
            &bid,
        )?;
        let keys = open_keys(&wrap, LABEL_ANSWER, &bid, 0, sealed)?;
        Ok((
            SharerChannel {
                broadcast: self.broadcast,
                wrap,
                last_counter: 0,
            },
            keys,
        ))
    }
}

/// The sharer's side of an established exchange with one viewer.
pub struct ViewerChannel {
    broadcast: BroadcastId,
    wrap: LessSafeKey,
    cert_hash: String,
    next_counter: Option<u32>,
}

impl ViewerChannel {
    /// The viewer's verified certificate hash.
    pub fn cert_hash(&self) -> &str {
        &self.cert_hash
    }

    /// Seals new share keys for this viewer (after a rekey).
    pub fn update(&mut self, keys: &[ShareKey]) -> Result<Vec<u8>, KeyxError> {
        let counter = self.next_counter.ok_or(KeyxError::CounterExhausted)?;
        let bid = self.broadcast.0.to_be_bytes();
        let sealed = seal_keys(&self.wrap, LABEL_UPDATE, &bid, counter, keys)?;
        self.next_counter = counter.checked_add(1);
        let mut message = Writer::new(TYPE_UPDATE);
        message.bytes(&counter.to_be_bytes());
        message.bytes(&sealed);
        Ok(message.0)
    }
}

/// The viewer's side of an established exchange with the sharer.
pub struct SharerChannel {
    broadcast: BroadcastId,
    wrap: LessSafeKey,
    last_counter: u32,
}

impl SharerChannel {
    /// Opens a key update from the sharer.
    pub fn open_update(&mut self, update: &[u8]) -> Result<Vec<ShareKey>, KeyxError> {
        let mut reader = Reader::new(update, TYPE_UPDATE)?;
        let counter = u32::from_be_bytes(reader.array::<4>()?);
        if counter <= self.last_counter {
            return Err(KeyxError::Replay);
        }
        let bid = self.broadcast.0.to_be_bytes();
        let keys = open_keys(&self.wrap, LABEL_UPDATE, &bid, counter, reader.rest())?;
        self.last_counter = counter;
        Ok(keys)
    }
}

fn wrap_key(
    private: EphemeralPrivateKey,
    peer_public: &[u8; X25519_LEN],
    viewer_public: &[u8; X25519_LEN],
    sharer_public: &[u8; X25519_LEN],
    bid: &[u8; 8],
) -> Result<LessSafeKey, KeyxError> {
    let mut key = [0u8; KEY_LEN];
    agreement::agree_ephemeral(
        private,
        &UnparsedPublicKey::new(&X25519, peer_public),
        |secret| {
            Salt::new(HKDF_SHA256, &[LABEL_WRAP, bid].concat())
                .extract(secret)
                .expand(&[viewer_public, sharer_public], &AES_128_GCM)
                .and_then(|okm| okm.fill(&mut key))
        },
    )
    .map_err(|_| KeyxError::KeyAgreement)?
    .map_err(|_| KeyxError::KeyAgreement)?;
    let unbound = UnboundKey::new(&AES_128_GCM, &key).map_err(|_| KeyxError::KeyAgreement)?;
    Ok(LessSafeKey::new(unbound))
}

fn wrap_nonce(counter: u32) -> Nonce {
    let mut nonce = [0u8; NONCE_LEN];
    nonce[NONCE_LEN - 4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::assume_unique_for_key(nonce)
}

fn seal_keys(
    wrap: &LessSafeKey,
    label: &[u8],
    bid: &[u8; 8],
    counter: u32,
    keys: &[ShareKey],
) -> Result<Vec<u8>, KeyxError> {
    if keys.is_empty() || keys.len() > MAX_KEYS {
        return Err(KeyxError::Malformed);
    }
    let mut data = Vec::with_capacity(1 + keys.len() * (1 + KEY_LEN) + TAG_LEN);
    data.push(keys.len() as u8);
    for key in keys {
        data.push(key.id);
        data.extend_from_slice(&key.base);
    }
    let aad = [label, bid, &counter.to_be_bytes()].concat();
    wrap.seal_in_place_append_tag(wrap_nonce(counter), Aad::from(aad), &mut data)
        .map_err(|_| KeyxError::Decrypt)?;
    Ok(data)
}

fn open_keys(
    wrap: &LessSafeKey,
    label: &[u8],
    bid: &[u8; 8],
    counter: u32,
    sealed: &[u8],
) -> Result<Vec<ShareKey>, KeyxError> {
    let aad = [label, bid, &counter.to_be_bytes()].concat();
    let mut data = sealed.to_vec();
    let plain = wrap
        .open_in_place(wrap_nonce(counter), Aad::from(aad), &mut data)
        .map_err(|_| KeyxError::Decrypt)?;
    let (&count, entries) = plain.split_first().ok_or(KeyxError::Malformed)?;
    let count = usize::from(count);
    if count == 0 || count > MAX_KEYS || entries.len() != count * (1 + KEY_LEN) {
        return Err(KeyxError::Malformed);
    }
    Ok(entries
        .as_chunks::<{ 1 + KEY_LEN }>()
        .0
        .iter()
        .map(|[id, base @ ..]| ShareKey {
            id: *id,
            base: *base,
        })
        .collect())
}

struct Writer(Vec<u8>);

impl Writer {
    fn new(kind: u8) -> Self {
        Self(vec![kind])
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn with_len(&mut self, bytes: &[u8]) -> Result<(), KeyxError> {
        let len = u16::try_from(bytes.len()).map_err(|_| KeyxError::TooLarge)?;
        self.bytes(&len.to_be_bytes());
        self.bytes(bytes);
        Ok(())
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn new(message: &'a [u8], kind: u8) -> Result<Self, KeyxError> {
        match message.split_first() {
            Some((&first, rest)) if first == kind => Ok(Self(rest)),
            _ => Err(KeyxError::Malformed),
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], KeyxError> {
        if self.0.len() < len {
            return Err(KeyxError::Malformed);
        }
        let (head, rest) = self.0.split_at(len);
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], KeyxError> {
        Ok(self.take(N)?.try_into().expect("took N bytes"))
    }

    fn with_len(&mut self) -> Result<&'a [u8], KeyxError> {
        let len = u16::from_be_bytes(self.array::<2>()?);
        self.take(usize::from(len))
    }

    fn rest(self) -> &'a [u8] {
        self.0
    }
}

/// Splits a key exchange message into chunks of at most `MAX_KEY_BODY` bytes, tagged with
/// `set_id` (pick a new one for every message to the same receiver).
pub fn split(message: &[u8], set_id: u8) -> Result<Vec<Vec<u8>>, KeyxError> {
    if message.is_empty() || message.len() > MAX_MESSAGE_LEN {
        return Err(KeyxError::TooLarge);
    }
    let parts: Vec<&[u8]> = message.chunks(CHUNK_DATA_LEN).collect();
    let count = parts.len() as u8;
    Ok(parts
        .iter()
        .enumerate()
        .map(|(index, data)| [&[set_id, index as u8, count][..], data].concat())
        .collect())
}

struct PartialSet {
    set_id: u8,
    parts: Vec<Option<Vec<u8>>>,
    started: Instant,
}

/// Puts chunks from several senders back together; see the module docs.
pub struct Reassembler<K> {
    sets: HashMap<K, PartialSet>,
}

impl<K: Eq + Hash> Default for Reassembler<K> {
    fn default() -> Self {
        Self {
            sets: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Clone> Reassembler<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one chunk from `from`; returns the whole message once its last chunk arrived.
    pub fn push(
        &mut self,
        from: K,
        chunk: &[u8],
        now: Instant,
    ) -> Result<Option<Vec<u8>>, KeyxError> {
        self.sets
            .retain(|_, set| now.saturating_duration_since(set.started) < REASSEMBLY_TIMEOUT);
        let Some((&[set_id, index, count], data)) = chunk.split_first_chunk::<CHUNK_HEADER_LEN>()
        else {
            return Err(KeyxError::BadChunk);
        };
        let (index, count) = (usize::from(index), usize::from(count));
        if count == 0 || count > MAX_CHUNKS || index >= count || data.is_empty() {
            return Err(KeyxError::BadChunk);
        }
        if count == 1 {
            self.sets.remove(&from);
            return Ok(Some(data.to_vec()));
        }
        let fresh = match self.sets.get(&from) {
            Some(set) => set.set_id != set_id || set.parts.len() != count,
            None => {
                if self.sets.len() >= MAX_OPEN_SETS {
                    return Err(KeyxError::TooManySets);
                }
                true
            }
        };
        if fresh {
            self.sets.insert(
                from.clone(),
                PartialSet {
                    set_id,
                    parts: vec![None; count],
                    started: now,
                },
            );
        }
        let Some(set) = self.sets.get_mut(&from) else {
            return Ok(None);
        };
        set.parts[index] = Some(data.to_vec());
        if set.parts.iter().all(Option::is_some)
            && let Some(set) = self.sets.remove(&from)
        {
            return Ok(Some(set.parts.into_iter().flatten().flatten().collect()));
        }
        Ok(None)
    }

    /// Drops the unfinished set of a sender that is gone.
    pub fn forget(&mut self, from: &K) {
        self.sets.remove(from);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use mumcord_store::Identity;

    use super::super::crypto::{FrameOpener, FrameSealer};
    use super::*;

    const BID: BroadcastId = BroadcastId(0x0123_4567_89ab_cdef);

    /// The Mumble default: a self-signed RSA-2048 certificate.
    static RSA: LazyLock<(Vec<u8>, Vec<u8>)> = LazyLock::new(|| {
        let identity = Identity::generate("viewer").expect("identity");
        (
            identity.cert_der().to_vec(),
            identity.key_pkcs8_der().to_vec(),
        )
    });

    fn rcgen_cert(alg: &'static rcgen::SignatureAlgorithm) -> (Vec<u8>, Vec<u8>) {
        let key = rcgen::KeyPair::generate_for(alg).expect("key");
        let cert = rcgen::CertificateParams::new(vec!["user".to_owned()])
            .expect("params")
            .self_signed(&key)
            .expect("cert");
        (cert.der().to_vec(), key.serialize_der())
    }

    fn cert_key((cert, key): &(Vec<u8>, Vec<u8>)) -> CertKey {
        CertKey::new(cert.clone(), key).expect("cert key")
    }

    fn hash((cert, _): &(Vec<u8>, Vec<u8>)) -> String {
        mumcord_store::cert_hash(cert)
    }

    fn keys() -> Vec<ShareKey> {
        vec![ShareKey::generate(4).expect("key")]
    }

    fn exchange(
        viewer: &(Vec<u8>, Vec<u8>),
        sharer: &(Vec<u8>, Vec<u8>),
    ) -> (ViewerChannel, SharerChannel, Vec<ShareKey>, Vec<ShareKey>) {
        let (pending, req) = request(BID, &cert_key(viewer)).expect("request");
        let sent = keys();
        let viewer_hash = hash(viewer);
        let (to_viewer, ans) =
            answer_request(BID, &cert_key(sharer), &req, |h| h == viewer_hash, &sent)
                .expect("answer");
        assert_eq!(to_viewer.cert_hash(), viewer_hash);
        let (to_sharer, got) = pending.accept(&ans, &hash(sharer)).expect("accept");
        (to_viewer, to_sharer, sent, got)
    }

    #[test]
    fn exchanges_keys_with_every_supported_key_type() {
        let ecdsa256 = rcgen_cert(&rcgen::PKCS_ECDSA_P256_SHA256);
        let ecdsa384 = rcgen_cert(&rcgen::PKCS_ECDSA_P384_SHA384);
        let ed25519 = rcgen_cert(&rcgen::PKCS_ED25519);
        for (viewer, sharer) in [(&*RSA, &ecdsa256), (&ecdsa384, &ed25519), (&ed25519, &*RSA)] {
            let (_, _, sent, got) = exchange(viewer, sharer);
            assert_eq!(got, sent);
        }
    }

    #[test]
    fn rejects_a_key_that_is_not_the_certificates() {
        let other = rcgen_cert(&rcgen::PKCS_ED25519);
        let mine = rcgen_cert(&rcgen::PKCS_ED25519);
        assert_eq!(
            CertKey::new(mine.0, &other.1).err(),
            Some(KeyxError::KeyMismatch)
        );
        assert_eq!(
            CertKey::new(RSA.0.clone(), b"not a key").err(),
            Some(KeyxError::UnsupportedKey)
        );
    }

    #[test]
    fn sharer_rejects_bad_requests() {
        let viewer = rcgen_cert(&rcgen::PKCS_ECDSA_P256_SHA256);
        let impostor = rcgen_cert(&rcgen::PKCS_ECDSA_P256_SHA256);
        let sharer = cert_key(&rcgen_cert(&rcgen::PKCS_ED25519));
        let (_, req) = request(BID, &cert_key(&viewer)).expect("request");
        let answer = |req: &[u8], bid| {
            answer_request(bid, &sharer, req, |h| h == hash(&viewer), &keys()).map(|_| ())
        };
        assert_eq!(answer(&req, BID), Ok(()));
        assert_eq!(answer(&req, BroadcastId(1)), Err(KeyxError::WrongBroadcast));

        // Signed by someone else, e.g. the bot or relay posing as an admitted viewer.
        let (_, other) = request(BID, &cert_key(&impostor)).expect("request");
        assert_eq!(answer(&other, BID), Err(KeyxError::NotAdmitted));

        // The relay swaps in its own X25519 key.
        let mut swapped = req.clone();
        swapped[9] ^= 1;
        assert_eq!(answer(&swapped, BID), Err(KeyxError::BadSignature));

        // The relay swaps in the viewer's certificate on the impostor's request.
        let mut forged = other[..41].to_vec();
        forged.extend_from_slice(&(viewer.0.len() as u16).to_be_bytes());
        forged.extend_from_slice(&viewer.0);
        forged.extend_from_slice(&other[43 + impostor.0.len()..]);
        assert_eq!(answer(&forged, BID), Err(KeyxError::BadSignature));

        assert_eq!(answer(&req[..30], BID), Err(KeyxError::Malformed));
        assert_eq!(answer(b"", BID), Err(KeyxError::Malformed));
    }

    #[test]
    fn viewer_rejects_bad_answers() {
        let viewer = cert_key(&rcgen_cert(&rcgen::PKCS_ED25519));
        let sharer = rcgen_cert(&rcgen::PKCS_ECDSA_P256_SHA256);
        let make = || {
            let (pending, req) = request(BID, &viewer).expect("request");
            let (_, ans) =
                answer_request(BID, &cert_key(&sharer), &req, |_| true, &keys()).expect("answer");
            (pending, ans)
        };

        let (pending, ans) = make();
        assert_eq!(
            pending.accept(&ans, &"0".repeat(40)).err(),
            Some(KeyxError::WrongIdentity)
        );

        let (pending, mut ans) = make();
        let last = ans.len() - 80;
        ans[last] ^= 1;
        assert_eq!(
            pending.accept(&ans, &hash(&sharer)).err(),
            Some(KeyxError::BadSignature)
        );

        // An answer meant for another request (another ephemeral key) doesn't verify.
        let (pending, _) = make();
        let (_, other_ans) = make();
        assert_eq!(
            pending.accept(&other_ans, &hash(&sharer)).err(),
            Some(KeyxError::BadSignature)
        );

        let (pending, ans) = make();
        assert!(pending.accept(&ans, &hash(&sharer).to_uppercase()).is_ok());
    }

    #[test]
    fn rekey_reaches_remaining_viewers_only() {
        let sharer = rcgen_cert(&rcgen::PKCS_ED25519);
        let stays_cert = rcgen_cert(&rcgen::PKCS_ECDSA_P256_SHA256);
        let leaves_cert = rcgen_cert(&rcgen::PKCS_ECDSA_P256_SHA256);
        let mut sealer = FrameSealer::new().expect("sealer");
        let sharer_key = cert_key(&sharer);

        let join = |cert: &(Vec<u8>, Vec<u8>)| {
            let (pending, req) = request(BID, &cert_key(cert)).expect("request");
            let (to_viewer, ans) =
                answer_request(BID, &sharer_key, &req, |_| true, &sealer.keys()).expect("answer");
            let (to_sharer, got) = pending.accept(&ans, &hash(&sharer)).expect("accept");
            let mut opener = FrameOpener::new();
            got.iter().for_each(|key| opener.add_key(key));
            (to_viewer, to_sharer, opener)
        };
        let (mut to_stays, mut stays_channel, mut stays) = join(&stays_cert);
        let (_, _, mut leaves) = join(&leaves_cert);

        // The second viewer leaves: rekey and update only the first.
        let new_key = sealer.rekey().expect("rekey");
        let update = to_stays.update(&[new_key]).expect("update");
        for key in stays_channel.open_update(&update).expect("open update") {
            stays.add_key(&key);
        }
        assert_eq!(
            stays_channel.open_update(&update).err(),
            Some(KeyxError::Replay)
        );

        let mut unit = Vec::new();
        let video = mumcord_p2p::broadcast::Content::Video {
            codec: mumcord_p2p::broadcast::VideoCodec::Vp9,
            keyframe: true,
            temporal_layer: 0,
        };
        sealer.seal(video, b"after", &mut unit).expect("seal");
        assert_eq!(stays.open(&mut unit.clone()).expect("open").1, b"after");
        assert!(leaves.open(&mut unit).is_err());

        let mut tampered = to_stays.update(&sealer.keys()).expect("update");
        *tampered.last_mut().expect("tag") ^= 1;
        assert_eq!(
            stays_channel.open_update(&tampered).err(),
            Some(KeyxError::Decrypt)
        );
    }

    #[test]
    fn an_update_for_one_viewer_does_not_open_for_another() {
        let sharer = rcgen_cert(&rcgen::PKCS_ED25519);
        let viewer = rcgen_cert(&rcgen::PKCS_ED25519);
        let (mut to_a, _, _, _) = exchange(&viewer, &sharer);
        let (_, mut b, _, _) = exchange(&viewer, &sharer);
        let update = to_a.update(&keys()).expect("update");
        assert_eq!(b.open_update(&update).err(), Some(KeyxError::Decrypt));
    }

    #[test]
    fn rsa_messages_need_several_chunks_and_reassemble() {
        let (_, req) = request(BID, &cert_key(&RSA)).expect("request");
        let chunks = split(&req, 9).expect("split");
        assert!(chunks.len() > 1, "an RSA request exceeds one key body");
        assert!(chunks.iter().all(|c| c.len() <= MAX_KEY_BODY as usize));

        let now = Instant::now();
        let mut reassembler = Reassembler::new();
        for chunk in chunks[1..].iter().rev() {
            assert_eq!(reassembler.push(3u16, chunk, now), Ok(None));
        }
        let whole = reassembler.push(3u16, &chunks[0], now).expect("push");
        assert_eq!(whole.as_deref(), Some(&req[..]));
    }

    #[test]
    fn reassembler_limits() {
        let now = Instant::now();
        let message = vec![7u8; CHUNK_DATA_LEN * 2 + 1];
        let chunks = split(&message, 1).expect("split");
        assert_eq!(chunks.len(), 3);
        assert_eq!(
            split(&vec![0; MAX_MESSAGE_LEN + 1], 1),
            Err(KeyxError::TooLarge)
        );
        assert_eq!(split(&[], 1), Err(KeyxError::TooLarge));

        let mut r = Reassembler::new();
        for bad in [
            &[1u8, 0][..],
            &[1, 0, 0, 5],
            &[1, 3, 3, 5],
            &[1, 0, 9, 5],
            &[1, 0, 2],
        ] {
            assert_eq!(r.push(0u16, bad, now), Err(KeyxError::BadChunk));
        }

        // An unfinished set times out.
        assert_eq!(r.push(0u16, &chunks[0], now), Ok(None));
        assert_eq!(r.push(0u16, &chunks[1], now), Ok(None));
        let later = now + REASSEMBLY_TIMEOUT;
        assert_eq!(r.push(0u16, &chunks[2], later), Ok(None));

        // A new set id replaces the unfinished one.
        let retry = split(&message, 2).expect("split");
        assert_eq!(r.push(0u16, &retry[0], later), Ok(None));
        assert_eq!(r.push(0u16, &retry[1], later), Ok(None));
        assert_eq!(r.push(0u16, &retry[2], later), Ok(Some(message.clone())));

        // Unfinished sets per sender are capped, and forgetting a sender frees its set.
        for slot in 0..MAX_OPEN_SETS as u16 {
            assert_eq!(r.push(slot, &chunks[0], later), Ok(None));
        }
        assert_eq!(r.push(999, &chunks[0], later), Err(KeyxError::TooManySets));
        r.forget(&0);
        assert_eq!(r.push(999, &chunks[0], later), Ok(None));
    }
}
