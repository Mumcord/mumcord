//! OCB2-AES128 as used by Mumble's UDP voice channel, ported from Mumble's
//! `CryptStateOCB2.cpp` (1.5.735) so both sides agree bit for bit.
//!
//! The protocol's OCB2 has known weaknesses (spec §9). Like upstream we include the
//! counter-measure against the XEX* attack (eprint 2019/311, section 9): when encrypting, a
//! critical second-to-last block gets one bit flipped (inaudible), and when decrypting such a
//! forged block is rejected.

use aes::Aes128;
use aes::cipher::array::Array;
use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};

pub(crate) const BLOCK: usize = 16;
/// Bytes the crypto adds in front of the plaintext: nonce low byte + 3 tag bytes.
pub(crate) const OVERHEAD: usize = 4;

type Block = [u8; BLOCK];

/// Packet statistics of the decrypting side, as Mumble counts them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CryptStats {
    /// Packets that decrypted fine.
    pub good: u32,
    /// Packets that arrived out of order (late).
    pub late: u32,
    /// Packets that never arrived (estimated from nonce gaps).
    pub lost: u32,
    /// Times the decrypt nonce was resynchronised by the server.
    pub resync: u32,
}

/// The UDP crypto state of one connection. `encrypt_iv`/`decrypt_iv` are the nonces; the
/// server's `client_nonce` is our encrypt nonce and its `server_nonce` our decrypt nonce.
pub(crate) struct CryptState {
    cipher: Option<Aes128>,
    encrypt_iv: Block,
    decrypt_iv: Block,
    decrypt_history: [u8; 256],
    pub(crate) stats: CryptStats,
}

impl std::fmt::Debug for CryptState {
    // Keys and nonces stay out of logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptState")
            .field("valid", &self.is_valid())
            .field("stats", &self.stats)
            .finish()
    }
}

impl CryptState {
    pub(crate) fn new() -> Self {
        Self {
            cipher: None,
            encrypt_iv: [0; BLOCK],
            decrypt_iv: [0; BLOCK],
            decrypt_history: [0; 256],
            stats: CryptStats::default(),
        }
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.cipher.is_some()
    }

    /// Installs a full key; returns `false` (state unchanged) if a length is wrong.
    pub(crate) fn set_key(&mut self, key: &[u8], encrypt_iv: &[u8], decrypt_iv: &[u8]) -> bool {
        let (Ok(key), Ok(eiv), Ok(div)) = (
            <Block>::try_from(key),
            <Block>::try_from(encrypt_iv),
            <Block>::try_from(decrypt_iv),
        ) else {
            return false;
        };
        self.cipher = Some(Aes128::new(&Array::from(key)));
        self.encrypt_iv = eiv;
        self.decrypt_iv = div;
        self.decrypt_history = [0; 256];
        true
    }

    pub(crate) fn set_decrypt_iv(&mut self, iv: &[u8]) -> bool {
        match <Block>::try_from(iv) {
            Ok(iv) => {
                self.decrypt_iv = iv;
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn encrypt_iv(&self) -> Vec<u8> {
        self.encrypt_iv.to_vec()
    }

    /// Encrypts `plain` into a packet of `plain.len() + OVERHEAD` bytes.
    pub(crate) fn encrypt(&mut self, plain: &[u8]) -> Option<Vec<u8>> {
        let cipher = self.cipher.as_ref()?;
        increment(&mut self.encrypt_iv);
        let mut out = vec![0; plain.len() + OVERHEAD];
        let tag = ocb_encrypt(cipher, plain, &mut out[OVERHEAD..], &self.encrypt_iv, true).0;
        out[0] = self.encrypt_iv[0];
        out[1..OVERHEAD].copy_from_slice(&tag[..3]);
        Some(out)
    }

    /// Decrypts a packet, updating nonce, replay history and statistics. `None` on any failure
    /// (bad tag, replay, nonce too far off); the state is then unchanged.
    pub(crate) fn decrypt(&mut self, packet: &[u8]) -> Option<Vec<u8>> {
        let cipher = self.cipher.as_ref()?;
        if packet.len() < OVERHEAD {
            return None;
        }
        let save_iv = self.decrypt_iv;
        let iv_byte = packet[0];
        let mut restore = false;
        let mut late = 0i64;
        let mut lost = 0i64;
        let div = &mut self.decrypt_iv;

        if div[0].wrapping_add(1) == iv_byte {
            // In order as expected.
            if iv_byte > div[0] {
                div[0] = iv_byte;
            } else if iv_byte < div[0] {
                div[0] = iv_byte;
                increment(&mut div[1..]);
            } else {
                return None;
            }
        } else {
            // Out of order or a repeat.
            let mut diff = i32::from(iv_byte) - i32::from(div[0]);
            if diff > 128 {
                diff -= 256;
            } else if diff < -128 {
                diff += 256;
            }
            if iv_byte < div[0] && diff > -30 && diff < 0 {
                // Late packet, no wraparound.
                late = 1;
                lost = -1;
                div[0] = iv_byte;
                restore = true;
            } else if iv_byte > div[0] && diff > -30 && diff < 0 {
                // Late packet from before the last wraparound.
                late = 1;
                lost = -1;
                div[0] = iv_byte;
                decrement(&mut div[1..]);
                restore = true;
            } else if iv_byte > div[0] && diff > 0 {
                // Lost a few packets.
                lost = i64::from(iv_byte) - i64::from(div[0]) - 1;
                div[0] = iv_byte;
            } else if iv_byte < div[0] && diff > 0 {
                // Lost a few packets and wrapped around.
                lost = 256 - i64::from(div[0]) + i64::from(iv_byte) - 1;
                div[0] = iv_byte;
                increment(&mut div[1..]);
            } else {
                return None;
            }
            if self.decrypt_history[usize::from(div[0])] == div[1] {
                self.decrypt_iv = save_iv;
                return None;
            }
        }

        let mut plain = vec![0; packet.len() - OVERHEAD];
        let (tag, ok) = ocb_decrypt(cipher, &packet[OVERHEAD..], &mut plain, &self.decrypt_iv);
        if !ok || tag[..3] != packet[1..OVERHEAD] {
            self.decrypt_iv = save_iv;
            return None;
        }
        self.decrypt_history[usize::from(self.decrypt_iv[0])] = self.decrypt_iv[1];
        if restore {
            self.decrypt_iv = save_iv;
        }
        let stats = &mut self.stats;
        stats.good = stats.good.wrapping_add(1);
        stats.late = apply_delta(stats.late, late);
        stats.lost = apply_delta(stats.lost, lost);
        Some(plain)
    }
}

/// Adds a signed delta like upstream: never wraps below zero.
fn apply_delta(value: u32, delta: i64) -> u32 {
    if delta > 0 {
        value.saturating_add(u32::try_from(delta).unwrap_or(u32::MAX))
    } else if i64::from(value) > -delta {
        value - u32::try_from(-delta).unwrap_or(0)
    } else {
        value
    }
}

/// Little-endian multi-byte increment (Mumble's nonce counter).
fn increment(bytes: &mut [u8]) {
    for b in bytes {
        *b = b.wrapping_add(1);
        if *b != 0 {
            break;
        }
    }
}

fn decrement(bytes: &mut [u8]) {
    for b in bytes {
        let was = *b;
        *b = was.wrapping_sub(1);
        if was != 0 {
            break;
        }
    }
}

fn aes_encrypt(cipher: &Aes128, block: &Block) -> Block {
    let mut b = Array::from(*block);
    cipher.encrypt_block(&mut b);
    b.into()
}

fn aes_decrypt(cipher: &Aes128, block: &Block) -> Block {
    let mut b = Array::from(*block);
    cipher.decrypt_block(&mut b);
    b.into()
}

fn xor(a: &Block, b: &Block) -> Block {
    std::array::from_fn(|i| a[i] ^ b[i])
}

/// Doubling in GF(2^128) on the big-endian block value.
fn s2(block: &Block) -> Block {
    let v = u128::from_be_bytes(*block);
    let carry = v >> 127;
    ((v << 1) ^ (carry * 0x87)).to_be_bytes()
}

/// Tripling: `x ^ 2x`.
fn s3(block: &Block) -> Block {
    xor(block, &s2(block))
}

fn length_block(len: usize) -> Block {
    // `len < BLOCK` here, so the bit length fits easily.
    ((len as u128) * 8).to_be_bytes()
}

/// Returns `(tag, ok)`; `ok` is `false` only when a critical block is met and
/// `modify_on_attack` is off (kept for the test that checks the decrypt-side detection).
fn ocb_encrypt(
    cipher: &Aes128,
    plain: &[u8],
    encrypted: &mut [u8],
    nonce: &Block,
    modify_on_attack: bool,
) -> (Block, bool) {
    let mut ok = true;
    let mut delta = aes_encrypt(cipher, nonce);
    let mut checksum = [0u8; BLOCK];
    let mut len = plain.len();
    let mut pos = 0;

    while len > BLOCK {
        let block: Block = plain[pos..pos + BLOCK].try_into().unwrap_or([0; BLOCK]);
        let mut flip = false;
        if len - BLOCK <= BLOCK && block[..BLOCK - 1].iter().all(|&b| b == 0) {
            if modify_on_attack {
                flip = true;
            } else {
                ok = false;
            }
        }
        delta = s2(&delta);
        let mut tmp = xor(&delta, &block);
        if flip {
            tmp[0] ^= 1;
        }
        let out = xor(&delta, &aes_encrypt(cipher, &tmp));
        encrypted[pos..pos + BLOCK].copy_from_slice(&out);
        checksum = xor(&checksum, &block);
        if flip {
            checksum[0] ^= 1;
        }
        len -= BLOCK;
        pos += BLOCK;
    }

    delta = s2(&delta);
    let pad = aes_encrypt(cipher, &xor(&length_block(len), &delta));
    let mut tmp = pad;
    tmp[..len].copy_from_slice(&plain[pos..]);
    checksum = xor(&checksum, &tmp);
    let out = xor(&pad, &tmp);
    encrypted[pos..].copy_from_slice(&out[..len]);
    delta = s3(&delta);
    (aes_encrypt(cipher, &xor(&delta, &checksum)), ok)
}

/// Returns `(tag, ok)`; `ok` is `false` when the XEX* forgery pattern is detected.
fn ocb_decrypt(
    cipher: &Aes128,
    encrypted: &[u8],
    plain: &mut [u8],
    nonce: &Block,
) -> (Block, bool) {
    let mut ok = true;
    let mut delta = aes_encrypt(cipher, nonce);
    let mut checksum = [0u8; BLOCK];
    let mut len = encrypted.len();
    let mut pos = 0;

    while len > BLOCK {
        let block: Block = encrypted[pos..pos + BLOCK].try_into().unwrap_or([0; BLOCK]);
        delta = s2(&delta);
        let out = xor(&delta, &aes_decrypt(cipher, &xor(&delta, &block)));
        plain[pos..pos + BLOCK].copy_from_slice(&out);
        checksum = xor(&checksum, &out);
        len -= BLOCK;
        pos += BLOCK;
    }

    delta = s2(&delta);
    let pad = aes_encrypt(cipher, &xor(&length_block(len), &delta));
    let mut tmp = [0u8; BLOCK];
    tmp[..len].copy_from_slice(&encrypted[pos..]);
    tmp = xor(&tmp, &pad);
    checksum = xor(&checksum, &tmp);
    plain[pos..].copy_from_slice(&tmp[..len]);

    // A forged last block would equal `delta ^ len`; `len` only touches the last byte.
    if tmp[..BLOCK - 1] == delta[..BLOCK - 1] {
        ok = false;
    }

    delta = s3(&delta);
    (aes_encrypt(cipher, &xor(&delta, &checksum)), ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW_KEY: Block = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];
    const NONCE: Block = [
        0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11,
        0x00,
    ];

    fn cipher() -> Aes128 {
        Aes128::new(&Array::from(RAW_KEY))
    }

    /// Encrypt/decrypt pair as in Mumble's TestCrypt: random key, encrypt nonce forced to 0x55.
    fn pair() -> (CryptState, CryptState) {
        let key: Block = rand::random();
        let enc_iv = [0x55; BLOCK];
        let dec_iv: Block = rand::random();
        let mut enc = CryptState::new();
        let mut dec = CryptState::new();
        assert!(enc.set_key(&key, &enc_iv, &dec_iv));
        assert!(dec.set_key(&key, &dec_iv, &enc_iv));
        (enc, dec)
    }

    // Vectors from draft-krovetz-ocb-00 as used in Mumble's TestCrypt::testvectors.
    #[test]
    fn known_vectors() {
        let c = cipher();
        let (tag, ok) = ocb_encrypt(&c, &[], &mut [], &RAW_KEY, true);
        assert!(ok);
        assert_eq!(
            tag,
            [
                0xBF, 0x31, 0x08, 0x13, 0x07, 0x73, 0xAD, 0x5E, 0xC7, 0x0E, 0xC6, 0x9E, 0x78, 0x75,
                0xA7, 0xB0
            ]
        );

        let source: Vec<u8> = (0..40).collect();
        let mut crypt = [0u8; 40];
        let (tag, ok) = ocb_encrypt(&c, &source, &mut crypt, &RAW_KEY, true);
        assert!(ok);
        assert_eq!(
            tag,
            [
                0x9D, 0xB0, 0xCD, 0xF8, 0x80, 0xF7, 0x3E, 0x3E, 0x10, 0xD4, 0xEB, 0x32, 0x17, 0x76,
                0x66, 0x88
            ]
        );
        assert_eq!(
            crypt,
            [
                0xF7, 0x5D, 0x6B, 0xC8, 0xB4, 0xDC, 0x8D, 0x66, 0xB8, 0x36, 0xA2, 0xB0, 0x8B, 0x32,
                0xA6, 0x36, 0x9F, 0x1C, 0xD3, 0xC5, 0x22, 0x8D, 0x79, 0xFD, 0x6C, 0x26, 0x7F, 0x5F,
                0x6A, 0xA7, 0xB2, 0x31, 0xC7, 0xDF, 0xB9, 0xD5, 0x99, 0x51, 0xAE, 0x9C
            ]
        );
    }

    #[test]
    fn authcrypt_roundtrip_all_lengths() {
        let c = cipher();
        for len in 0..128u8 {
            let src: Vec<u8> = (1..=len).collect();
            let mut enc = vec![0; src.len()];
            let mut dec = vec![0; src.len()];
            let (etag, eok) = ocb_encrypt(&c, &src, &mut enc, &NONCE, true);
            let (dtag, dok) = ocb_decrypt(&c, &enc, &mut dec, &NONCE);
            assert!(eok && dok, "len {len}");
            assert_eq!(etag, dtag);
            assert_eq!(src, dec);
        }
    }

    // Mumble's TestCrypt::xexstarAttack.
    #[test]
    fn xexstar_attack_is_detected_and_avoided() {
        let c = cipher();
        let mut src = vec![0u8; 2 * BLOCK];
        src[BLOCK - 1] = (BLOCK * 8) as u8;
        src[BLOCK..].fill(42);
        let mut enc = vec![0u8; 2 * BLOCK];
        let (mut etag, eok) = ocb_encrypt(&c, &src, &mut enc, &NONCE, false);
        assert!(!eok);

        enc[BLOCK - 1] ^= (BLOCK * 8) as u8;
        for i in 0..BLOCK {
            etag[i] = src[BLOCK + i] ^ enc[BLOCK + i];
        }
        let mut dec = vec![0u8; BLOCK];
        let (dtag, dok) = ocb_decrypt(&c, &enc[..BLOCK], &mut dec, &NONCE);
        assert_eq!(
            etag, dtag,
            "forged tag must match for the test to be meaningful"
        );
        assert!(!dok);

        let (etag, eok) = ocb_encrypt(&c, &src, &mut enc, &NONCE, true);
        let mut dec = vec![0u8; 2 * BLOCK];
        let (dtag, dok) = ocb_decrypt(&c, &enc, &mut dec, &NONCE);
        assert!(eok && dok);
        assert_eq!(etag, dtag);
        assert_eq!(src[0], 0);
        assert_eq!(dec[0], 1);
    }

    // Mumble's TestCrypt::reverserecovery.
    #[test]
    fn out_of_order_within_30_and_replay_rejected() {
        let (mut enc, mut dec) = pair();
        let secret = b"abcdefghi\0";
        let crypted: Vec<_> = (0..128)
            .map(|_| enc.encrypt(secret).expect("enc"))
            .collect();
        for i in 0..30 {
            assert!(dec.decrypt(&crypted[127 - i]).is_some(), "i={i}");
        }
        for i in 30..128 {
            assert!(dec.decrypt(&crypted[127 - i]).is_none(), "i={i}");
        }
        for i in 0..30 {
            assert!(dec.decrypt(&crypted[127 - i]).is_none(), "replay i={i}");
        }

        let crypted: Vec<_> = (0..512)
            .map(|_| enc.encrypt(secret).expect("enc"))
            .collect();
        for c in &crypted {
            assert!(dec.decrypt(c).is_some());
        }
        for c in &crypted {
            assert!(dec.decrypt(c).is_none());
        }
    }

    // Mumble's TestCrypt::ivrecovery.
    #[test]
    fn nonce_recovery_wraparound_and_resync() {
        let (mut enc, mut dec) = pair();
        let secret = b"abcdefghi\0";
        let c = enc.encrypt(secret).expect("enc");
        assert_eq!(dec.decrypt(&c).as_deref(), Some(&secret[..]));
        assert!(dec.decrypt(&c).is_none(), "same nonce twice");

        let mut c = Vec::new();
        for _ in 0..16 {
            c = enc.encrypt(secret).expect("enc");
        }
        assert!(dec.decrypt(&c).is_some(), "recovers from lost packets");

        for _ in 0..128 {
            dec.stats.lost = 0;
            for _ in 0..15 {
                c = enc.encrypt(secret).expect("enc");
            }
            assert!(dec.decrypt(&c).is_some());
            assert_eq!(dec.stats.lost, 14);
        }
        assert_eq!(enc.encrypt_iv, dec.decrypt_iv);

        for _ in 0..257 {
            c = enc.encrypt(secret).expect("enc");
        }
        assert!(dec.decrypt(&c).is_none(), "wrapped too far");

        assert!(dec.set_decrypt_iv(&enc.encrypt_iv()));
        let c = enc.encrypt(secret).expect("enc");
        assert!(dec.decrypt(&c).is_some(), "works again after resync");
    }

    #[test]
    fn late_packets_are_counted() {
        let (mut enc, mut dec) = pair();
        let a = enc.encrypt(b"a").expect("enc");
        let b = enc.encrypt(b"b").expect("enc");
        assert!(dec.decrypt(&b).is_some());
        assert_eq!(dec.stats.lost, 1);
        assert!(dec.decrypt(&a).is_some());
        // Upstream only decrements while the counter stays above zero, so `lost` stays 1.
        assert_eq!((dec.stats.good, dec.stats.late, dec.stats.lost), (2, 1, 1));
    }

    #[test]
    fn tampered_packet_fails_and_keeps_state() {
        let (mut enc, mut dec) = pair();
        let mut c = enc.encrypt(b"hello").expect("enc");
        c[5] ^= 1;
        assert!(dec.decrypt(&c).is_none());
        let c = enc.encrypt(b"again").expect("enc");
        assert!(dec.decrypt(&c).is_some());
        assert!(
            CryptState::new().encrypt(b"x").is_none(),
            "no key, no packet"
        );
    }
}
