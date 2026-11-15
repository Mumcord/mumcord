//! What a transfer carries, and its binary encoding (the plaintext before compression).
//!
//! ```text
//! payload  = version(1) | identity count(1) | identity* | has servers(1) | [server count(2) | server*]
//! identity = label(str) | flags(1, bit 0 = default) | cert len(2) | cert | key len(2) | key
//! server   = name(str) | address(str) | port(2) | user name(str) | has identity(1) | [SHA-1(20)]
//! str      = len(1) | UTF-8 bytes
//! ```
//!
//! Integers are little endian. Decoding reads exactly this and nothing else: unknown versions,
//! flags, lengths above the limits and trailing bytes are all refused.

use std::fmt;

use zeroize::Zeroizing;

use super::TransferError;

/// Most identities in one transfer.
pub const MAX_IDENTITIES: usize = 32;
/// Most servers in one transfer.
pub const MAX_SERVERS: usize = 500;
/// Longest label, server name, address or user name, in UTF-8 bytes.
pub const MAX_STRING_BYTES: usize = 255;
/// Largest DER certificate (an RSA-4096 certificate has about 1.5 KiB).
pub const MAX_CERT_BYTES: usize = 8 * 1024;
/// Largest PKCS#8 private key (RSA-4096 has about 2.4 KiB).
pub const MAX_KEY_BYTES: usize = 8 * 1024;
/// Largest payload before compression; also the cap for decompression on the phone.
pub const MAX_PLAINTEXT_BYTES: usize = 256 * 1024;

const PAYLOAD_VERSION: u8 = 1;
const FLAG_DEFAULT: u8 = 1;
const FINGERPRINT_BYTES: usize = 20;

/// One identity in a transfer. The key is wiped when the value is dropped and never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct TransferIdentity {
    /// The local label on the sending device.
    pub label: String,
    /// The DER-encoded certificate.
    pub cert_der: Vec<u8>,
    key_pkcs8_der: Zeroizing<Vec<u8>>,
    /// Whether it is the default identity of the sending device.
    pub is_default: bool,
}

impl TransferIdentity {
    pub fn new(label: &str, cert_der: &[u8], key_pkcs8_der: &[u8], is_default: bool) -> Self {
        Self {
            label: label.to_owned(),
            cert_der: cert_der.to_vec(),
            key_pkcs8_der: Zeroizing::new(key_pkcs8_der.to_vec()),
            is_default,
        }
    }

    /// The PKCS#8 DER private key. Never log or persist it outside secure storage.
    pub fn key_pkcs8_der(&self) -> &[u8] {
        &self.key_pkcs8_der
    }
}

impl fmt::Debug for TransferIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransferIdentity")
            .field("label", &self.label)
            .field("cert_hash", &crate::cert_hash(&self.cert_der))
            .field("is_default", &self.is_default)
            .finish_non_exhaustive()
    }
}

/// One saved server in a transfer. It has no password and no access tokens (FR-5.66).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferServer {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    /// Certificate hash (40 hex digits, see [`cert_hash`](crate::cert_hash)) of the identity the
    /// server uses on the sending device; `None` when it uses the default identity.
    pub identity_fingerprint: Option<String>,
}

/// The content of a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransferPayload {
    pub identities: Vec<TransferIdentity>,
    /// `None` when the server list is not part of the transfer.
    pub servers: Option<Vec<TransferServer>>,
}

impl TransferPayload {
    pub(crate) fn encode(&self) -> Result<Zeroizing<Vec<u8>>, TransferError> {
        if self.identities.is_empty() && self.servers.is_none() {
            return Err(TransferError::InvalidPayload("nothing is selected"));
        }
        if self.identities.len() > MAX_IDENTITIES {
            return Err(TransferError::PayloadTooLarge);
        }
        if self.identities.iter().filter(|i| i.is_default).count() > 1 {
            return Err(TransferError::InvalidPayload(
                "more than one default identity",
            ));
        }
        let mut out = Zeroizing::new(Vec::new());
        out.push(PAYLOAD_VERSION);
        out.push(self.identities.len() as u8);
        for identity in &self.identities {
            put_str(&mut out, &identity.label)?;
            out.push(if identity.is_default { FLAG_DEFAULT } else { 0 });
            put_blob(&mut out, &identity.cert_der, MAX_CERT_BYTES)?;
            put_blob(&mut out, &identity.key_pkcs8_der, MAX_KEY_BYTES)?;
        }
        match &self.servers {
            None => out.push(0),
            Some(servers) => {
                let count =
                    u16::try_from(servers.len()).map_err(|_| TransferError::PayloadTooLarge)?;
                if servers.len() > MAX_SERVERS {
                    return Err(TransferError::PayloadTooLarge);
                }
                out.push(1);
                out.extend_from_slice(&count.to_le_bytes());
                for server in servers {
                    put_str(&mut out, &server.name)?;
                    put_str(&mut out, &server.address)?;
                    out.extend_from_slice(&server.port.to_le_bytes());
                    put_str(&mut out, &server.username)?;
                    match &server.identity_fingerprint {
                        None => out.push(0),
                        Some(hex) => {
                            out.push(1);
                            out.extend_from_slice(&fingerprint_bytes(hex)?);
                        }
                    }
                }
            }
        }
        if out.len() > MAX_PLAINTEXT_BYTES {
            return Err(TransferError::PayloadTooLarge);
        }
        Ok(out)
    }

    /// Decodes untrusted bytes. Every failure is [`TransferError::Malformed`].
    pub(crate) fn decode(data: &[u8]) -> Result<Self, TransferError> {
        if data.len() > MAX_PLAINTEXT_BYTES {
            return Err(TransferError::Malformed);
        }
        let mut input = Input { rest: data };
        if input.byte()? != PAYLOAD_VERSION {
            return Err(TransferError::Malformed);
        }
        let identity_count = usize::from(input.byte()?);
        if identity_count > MAX_IDENTITIES {
            return Err(TransferError::Malformed);
        }
        let mut identities = Vec::with_capacity(identity_count);
        for _ in 0..identity_count {
            let label = input.string()?;
            let is_default = match input.byte()? {
                0 => false,
                FLAG_DEFAULT => true,
                _ => return Err(TransferError::Malformed),
            };
            let cert_der = input.blob(MAX_CERT_BYTES)?.to_vec();
            let key_pkcs8_der = Zeroizing::new(input.blob(MAX_KEY_BYTES)?.to_vec());
            identities.push(TransferIdentity {
                label,
                cert_der,
                key_pkcs8_der,
                is_default,
            });
        }
        if identities.iter().filter(|i| i.is_default).count() > 1 {
            return Err(TransferError::Malformed);
        }
        let servers = match input.byte()? {
            0 => None,
            1 => {
                let count = usize::from(input.u16()?);
                if count > MAX_SERVERS {
                    return Err(TransferError::Malformed);
                }
                let mut servers = Vec::with_capacity(count);
                for _ in 0..count {
                    servers.push(TransferServer {
                        name: input.string()?,
                        address: input.string()?,
                        port: input.u16()?,
                        username: input.string()?,
                        identity_fingerprint: match input.byte()? {
                            0 => None,
                            1 => Some(hex(input.take(FINGERPRINT_BYTES)?)),
                            _ => return Err(TransferError::Malformed),
                        },
                    });
                }
                Some(servers)
            }
            _ => return Err(TransferError::Malformed),
        };
        if !input.rest.is_empty() || (identities.is_empty() && servers.is_none()) {
            return Err(TransferError::Malformed);
        }
        Ok(Self {
            identities,
            servers,
        })
    }
}

fn put_str(out: &mut Vec<u8>, text: &str) -> Result<(), TransferError> {
    let len = u8::try_from(text.len())
        .map_err(|_| TransferError::InvalidPayload("a name is longer than 255 bytes"))?;
    out.push(len);
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn put_blob(out: &mut Vec<u8>, blob: &[u8], max: usize) -> Result<(), TransferError> {
    if blob.is_empty() {
        return Err(TransferError::InvalidPayload("an identity is incomplete"));
    }
    if blob.len() > max {
        return Err(TransferError::PayloadTooLarge);
    }
    let len = u16::try_from(blob.len()).map_err(|_| TransferError::PayloadTooLarge)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(blob);
    Ok(())
}

fn fingerprint_bytes(hex: &str) -> Result<[u8; FINGERPRINT_BYTES], TransferError> {
    let invalid = TransferError::InvalidPayload("an identity fingerprint is not 40 hex digits");
    let digits = hex.as_bytes();
    if digits.len() != FINGERPRINT_BYTES * 2 {
        return Err(invalid);
    }
    let mut out = [0u8; FINGERPRINT_BYTES];
    for (byte, pair) in out.iter_mut().zip(digits.chunks(2)) {
        let high = char::from(pair[0]).to_digit(16).ok_or(invalid)?;
        let low = char::from(pair[1]).to_digit(16).ok_or(invalid)?;
        *byte = (high * 16 + low) as u8;
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Cursor over untrusted bytes; running out of input is an error, never a panic.
struct Input<'a> {
    rest: &'a [u8],
}

impl<'a> Input<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], TransferError> {
        let (head, tail) = self
            .rest
            .split_at_checked(len)
            .ok_or(TransferError::Malformed)?;
        self.rest = tail;
        Ok(head)
    }

    fn byte(&mut self) -> Result<u8, TransferError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, TransferError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn string(&mut self) -> Result<String, TransferError> {
        let len = usize::from(self.byte()?);
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| TransferError::Malformed)
    }

    fn blob(&mut self, max: usize) -> Result<&'a [u8], TransferError> {
        let len = usize::from(self.u16()?);
        if len == 0 || len > max {
            return Err(TransferError::Malformed);
        }
        self.take(len)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use proptest::prelude::*;

    use super::*;

    /// An identity with stand-in bytes of realistic size; the format treats them as opaque.
    /// The bytes look random, like real key material, so they compress as badly.
    pub(crate) fn identity(label: &str, seed: u8, is_default: bool) -> TransferIdentity {
        let bytes = |len: usize, seed: u32| -> Vec<u8> {
            let mut state = seed;
            (0..len)
                .map(|_| {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    (state >> 16) as u8
                })
                .collect()
        };
        let seed = u32::from(seed) * 2;
        TransferIdentity::new(
            label,
            &bytes(780, seed + 1),
            &bytes(1218, seed + 2),
            is_default,
        )
    }

    pub(crate) fn server(index: u16) -> TransferServer {
        TransferServer {
            name: format!("Server {index}"),
            address: format!("voice{index}.example.org"),
            port: 64738 - index,
            username: "Zoë".to_owned(),
            identity_fingerprint: index
                .is_multiple_of(2)
                .then(|| "00112233445566778899aabbccddeeff00112233".to_owned()),
        }
    }

    pub(crate) fn sample(identities: usize, servers: Option<u16>) -> TransferPayload {
        TransferPayload {
            identities: (0..identities)
                .map(|i| identity(&format!("Identity {i}"), i as u8, i == 0))
                .collect(),
            servers: servers.map(|count| (0..count).map(server).collect()),
        }
    }

    #[test]
    fn round_trips() {
        for payload in [
            sample(1, None),
            sample(3, Some(0)),
            sample(0, Some(20)),
            sample(MAX_IDENTITIES, Some(MAX_SERVERS as u16)),
        ] {
            let encoded = payload.encode().expect("encode");
            assert_eq!(TransferPayload::decode(&encoded), Ok(payload));
        }
    }

    #[test]
    fn fingerprints_come_back_in_lowercase() {
        let mut payload = sample(0, Some(1));
        let servers = payload.servers.as_mut().expect("servers");
        servers[0].identity_fingerprint =
            Some("00112233445566778899AABBCCDDEEFF00112233".to_owned());
        let decoded = TransferPayload::decode(&payload.encode().expect("encode")).expect("decode");
        assert_eq!(
            decoded.servers.expect("servers")[0]
                .identity_fingerprint
                .as_deref(),
            Some("00112233445566778899aabbccddeeff00112233")
        );
    }

    #[test]
    fn the_writer_refuses_what_breaks_a_limit() {
        use TransferError::{InvalidPayload, PayloadTooLarge};

        assert!(matches!(
            TransferPayload::default().encode(),
            Err(InvalidPayload(_))
        ));
        assert_eq!(
            sample(MAX_IDENTITIES + 1, None).encode().err(),
            Some(PayloadTooLarge)
        );
        let mut two_defaults = sample(2, None);
        two_defaults.identities[1].is_default = true;
        assert!(matches!(two_defaults.encode(), Err(InvalidPayload(_))));
        let mut long_label = sample(1, None);
        long_label.identities[0].label = "x".repeat(MAX_STRING_BYTES + 1);
        assert!(matches!(long_label.encode(), Err(InvalidPayload(_))));
        let big_key = TransferPayload {
            identities: vec![TransferIdentity::new(
                "a",
                &[1],
                &[0; MAX_KEY_BYTES + 1],
                false,
            )],
            servers: None,
        };
        assert_eq!(big_key.encode().err(), Some(PayloadTooLarge));
        let no_cert = TransferPayload {
            identities: vec![TransferIdentity::new("a", &[], &[1], false)],
            servers: None,
        };
        assert!(matches!(no_cert.encode(), Err(InvalidPayload(_))));
        let mut bad_fingerprint = sample(0, Some(1));
        bad_fingerprint.servers.as_mut().expect("servers")[0].identity_fingerprint =
            Some("xyz".to_owned());
        assert!(matches!(bad_fingerprint.encode(), Err(InvalidPayload(_))));
        let too_many_servers = TransferPayload {
            identities: Vec::new(),
            servers: Some((0..=MAX_SERVERS as u16).map(server).collect()),
        };
        assert_eq!(too_many_servers.encode().err(), Some(PayloadTooLarge));
    }

    #[test]
    fn the_reader_refuses_malformed_input() {
        let good = sample(1, Some(2)).encode().expect("encode");
        // Every truncation, and trailing bytes.
        for len in 0..good.len() {
            assert_eq!(
                TransferPayload::decode(&good[..len]),
                Err(TransferError::Malformed),
                "truncated to {len}"
            );
        }
        let mut longer = good.to_vec();
        longer.push(0);
        assert_eq!(
            TransferPayload::decode(&longer),
            Err(TransferError::Malformed)
        );

        let corrupt = |index: usize, value: u8| {
            let mut bytes = good.to_vec();
            bytes[index] = value;
            TransferPayload::decode(&bytes)
        };
        assert_eq!(corrupt(0, 2), Err(TransferError::Malformed), "version");
        assert_eq!(
            corrupt(1, 33),
            Err(TransferError::Malformed),
            "identity count"
        );
        // The flags byte follows the label "Identity 0".
        assert_eq!(
            corrupt(2 + 1 + 10, 2),
            Err(TransferError::Malformed),
            "flags"
        );

        // Counts and lengths that promise more than the limits allow.
        assert_eq!(
            TransferPayload::decode(&[1, 0, 1, 0xff, 0xff]),
            Err(TransferError::Malformed)
        );
        assert_eq!(
            TransferPayload::decode(&[1, 1, 0, 0, 0xff, 0xff]),
            Err(TransferError::Malformed)
        );
        // Not UTF-8.
        assert_eq!(
            TransferPayload::decode(&[1, 1, 2, 0xff, 0xfe]),
            Err(TransferError::Malformed)
        );
        // Nothing inside.
        assert_eq!(
            TransferPayload::decode(&[1, 0, 0]),
            Err(TransferError::Malformed)
        );
        assert_eq!(
            TransferPayload::decode(&vec![1; MAX_PLAINTEXT_BYTES + 1]),
            Err(TransferError::Malformed)
        );
    }

    #[test]
    fn debug_output_has_no_key_material() {
        let payload = TransferPayload {
            identities: vec![TransferIdentity::new(
                "Main",
                b"certificate",
                b"SECRETKEY",
                true,
            )],
            servers: None,
        };
        let text = format!("{payload:?}");
        assert!(text.contains("Main"));
        assert!(!text.contains("SECRETKEY"));
        // The key as a byte list, as a derived `Debug` would print it.
        assert!(!text.contains("83, 69, 67"));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn decoding_never_panics(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
            let _ = TransferPayload::decode(&data);
        }

        #[test]
        fn decoding_survives_any_single_byte_change(index in 0usize..4096, value: u8) {
            let mut bytes = sample(1, Some(3)).encode().expect("encode").to_vec();
            let index = index % bytes.len();
            bytes[index] = value;
            // Either still a well-formed payload or a clean error.
            if let Ok(decoded) = TransferPayload::decode(&bytes) {
                prop_assert_eq!(decoded.encode().expect("re-encode").to_vec(), bytes);
            }
        }
    }
}
