//! The user's identity: a client certificate with its private key, created here or
//! imported from a PKCS#12 file (e.g. one exported by the official Mumble client).

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use p12_keystore::{
    Certificate, KeyStore, KeyStoreEntry, Pkcs12Archive, PrivateKey, PrivateKeyChain,
};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    PublicKeyData, SerialNumber, date_time_ymd,
};
use rsa::pkcs8::EncodePrivateKey;
use rsa::rand_core::{OsRng, RngCore};
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};

/// Same key size and validity as certificates created by the official Mumble client.
const RSA_BITS: usize = 2048;
const VALIDITY_YEARS: i32 = 20;

/// Certificate hash as Mumble uses it to identify users: SHA-1 over the DER-encoded
/// certificate, as 40 lowercase hex characters (matches `UserState.hash`).
pub fn cert_hash(cert_der: &[u8]) -> String {
    Sha1::digest(cert_der)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A client certificate with its private key. `Debug` never prints the key.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    name: String,
    cert_der: Vec<u8>,
    key_pkcs8_der: Vec<u8>,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("name", &self.name)
            .field("cert_hash", &self.cert_hash())
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Creates a new self-signed identity like the official client does: RSA-2048,
    /// subject `CN=<name>`, not a CA, client-auth usage, valid for 20 years.
    pub fn generate(name: &str) -> Result<Self> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::EmptyIdentityName);
        }
        // ring (our crypto backend, chosen to keep the build cmake-free) cannot generate
        // RSA keys, so the `rsa` crate is used for key generation only. All private-key
        // operations (certificate signing here, TLS signatures at runtime) run in ring;
        // RUSTSEC-2023-0071 (timing side channel) concerns the `rsa` crate's private-key
        // operations, which are never used.
        let key = rsa::RsaPrivateKey::new(&mut OsRng, RSA_BITS)
            .map_err(|e| Error::KeyGeneration(e.to_string()))?;
        let key_pkcs8 = key
            .to_pkcs8_der()
            .map_err(|e| Error::KeyGeneration(e.to_string()))?;
        let key_pair = KeyPair::try_from(key_pkcs8.as_bytes())?;

        let mut params = CertificateParams::default();
        let mut subject = DistinguishedName::new();
        subject.push(DnType::CommonName, name);
        params.distinguished_name = subject;
        params.is_ca = IsCa::ExplicitNoCa;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params.serial_number = Some(random_serial());
        let (year, month, day) = today_utc();
        params.not_before = date_time_ymd(year, month, day);
        // Feb 29 does not exist in every target year.
        let end_day = if month == 2 && day == 29 { 28 } else { day };
        params.not_after = date_time_ymd(year + VALIDITY_YEARS, month, end_day);

        let cert = params.self_signed(&key_pair)?;
        Ok(Self {
            name: name.to_owned(),
            cert_der: cert.der().to_vec(),
            key_pkcs8_der: key_pkcs8.as_bytes().to_vec(),
        })
    }

    /// Imports a PKCS#12 file. Supports the modern (AES/SHA-256) and legacy
    /// (3DES/RC2/SHA-1) encodings and empty passwords, which covers files exported
    /// by the official Mumble client with any OpenSSL version.
    pub fn from_pkcs12(data: &[u8], password: &str) -> Result<Self> {
        let archive = Pkcs12Archive::from_pkcs12(data, password)?;
        // Pair key and certificate by public key rather than by bag attributes:
        // local key ids and friendly names are optional and set inconsistently by tools.
        for key_bag in &archive.keys {
            let key_der = key_bag.key.as_der();
            let Ok(key_pair) = KeyPair::try_from(key_der) else {
                continue;
            };
            // The raw public key (not the full SPKI) avoids depending on how the
            // algorithm identifier is encoded.
            let public_key = key_pair.der_bytes();
            let Some(cert_bag) = archive
                .certs
                .iter()
                .find(|bag| contains(bag.cert.as_der(), public_key))
            else {
                continue;
            };
            let name = common_name(cert_bag.cert.subject())
                .or_else(|| key_bag.friendly_name.clone())
                .unwrap_or_default();
            return Ok(Self {
                name,
                cert_der: cert_bag.cert.as_der().to_vec(),
                key_pkcs8_der: key_der.to_vec(),
            });
        }
        Err(Error::Pkcs12NoIdentity)
    }

    /// Exports as a PKCS#12 file protected with `password` (AES-256, HMAC-SHA256).
    pub fn to_pkcs12(&self, password: &str) -> Result<Vec<u8>> {
        let chain = PrivateKeyChain::new(
            Sha1::digest(&self.cert_der).to_vec(),
            PrivateKey::from_der(&self.key_pkcs8_der)?,
            [Certificate::from_der(&self.cert_der)?],
        );
        let mut keystore = KeyStore::new();
        keystore.add_entry(&self.name, KeyStoreEntry::PrivateKeyChain(chain));
        Ok(keystore.writer(password).write()?)
    }

    /// Default username (from the certificate's common name).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// DER-encoded client certificate, for the TLS client config.
    pub fn cert_der(&self) -> &[u8] {
        &self.cert_der
    }

    /// PKCS#8 DER private key, for the TLS client config. Never log or persist it.
    pub fn key_pkcs8_der(&self) -> &[u8] {
        &self.key_pkcs8_der
    }

    pub fn cert_hash(&self) -> String {
        cert_hash(&self.cert_der)
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Extracts `CN=` from an RFC 4514-style subject string ("CN=Tester, emailAddress=…").
fn common_name(subject: &str) -> Option<String> {
    subject
        .split(", ")
        .find_map(|part| part.strip_prefix("CN="))
        .filter(|cn| !cn.is_empty())
        .map(str::to_owned)
}

fn random_serial() -> SerialNumber {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    // Serial numbers must be positive DER integers.
    bytes[0] &= 0x7f;
    SerialNumber::from_slice(&bytes)
}

/// Today's UTC date as (year, month, day).
fn today_utc() -> (i32, u8, u8) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    civil_from_days((secs / 86_400) as i64)
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::LazyLock;

    use super::*;

    // RSA key generation is slow in debug builds; share one identity across tests.
    static TESTER: LazyLock<Identity> =
        LazyLock::new(|| Identity::generate("Tester").expect("generate"));

    #[test]
    fn cert_hash_is_lowercase_sha1_hex() {
        // SHA-1("abc"), well-known test vector.
        assert_eq!(
            cert_hash(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_750), (2026, 10, 24));
    }

    #[test]
    fn generated_identity_is_rsa_with_common_name() {
        let identity = &*TESTER;
        assert_eq!(identity.name(), "Tester");
        assert_eq!(identity.cert_hash().len(), 40);
        let key_pair = KeyPair::try_from(identity.key_pkcs8_der()).expect("ring loads key");
        assert_eq!(key_pair.algorithm(), &rcgen::PKCS_RSA_SHA256);
        assert!(contains(identity.cert_der(), key_pair.der_bytes()));
        assert!(matches!(
            Identity::generate("  "),
            Err(Error::EmptyIdentityName)
        ));
    }

    #[test]
    fn export_import_round_trip_keeps_cert_hash() {
        let identity = &*TESTER;
        let p12 = identity.to_pkcs12("s3cret").expect("export");
        let imported = Identity::from_pkcs12(&p12, "s3cret").expect("import");
        assert_eq!(&imported, identity);
        assert_eq!(imported.cert_hash(), identity.cert_hash());
        assert!(Identity::from_pkcs12(&p12, "wrong").is_err());
    }

    #[test]
    fn empty_password_round_trip() {
        let p12 = TESTER.to_pkcs12("").expect("export");
        assert_eq!(Identity::from_pkcs12(&p12, "").expect("import"), *TESTER);
    }

    #[test]
    fn debug_does_not_leak_key() {
        let printed = format!("{:?}", *TESTER);
        assert!(printed.contains("Tester"));
        assert!(!printed.contains("key_pkcs8"));
    }

    /// Re-encodes our identity with the OpenSSL CLI the way the official client's
    /// exports look (legacy 3DES/RC2 + SHA-1 MAC from OpenSSL 1.x, AES from 3.x) and
    /// checks we import it with the same hash. Skipped if `openssl` is not installed.
    #[test]
    fn imports_openssl_encoded_files() {
        if Command::new("openssl").arg("version").output().is_err() {
            eprintln!("openssl not found, skipping");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let cert = dir.path().join("cert.der");
        let key = dir.path().join("key.der");
        std::fs::write(&cert, TESTER.cert_der()).expect("write cert");
        std::fs::write(&key, TESTER.key_pkcs8_der()).expect("write key");
        let pem = |input: &std::path::Path, kind: &str| -> Vec<u8> {
            let output = Command::new("openssl")
                .args([kind, "-inform", "DER", "-in"])
                .arg(input)
                .output()
                .expect("openssl convert");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        let mut combined = pem(&cert, "x509");
        combined.extend(pem(&key, "pkey"));
        let pem_path = dir.path().join("identity.pem");
        std::fs::write(&pem_path, combined).expect("write pem");

        for (extra, password) in [
            (&["-legacy"][..], "pw"),
            (
                &[
                    "-certpbe",
                    "PBE-SHA1-3DES",
                    "-keypbe",
                    "PBE-SHA1-3DES",
                    "-macalg",
                    "sha1",
                ][..],
                "pw",
            ),
            (&[][..], "pw"),
            (&[][..], ""),
        ] {
            let out = dir.path().join("identity.p12");
            let status = Command::new("openssl")
                .args(["pkcs12", "-export", "-name", "Tester", "-in"])
                .arg(&pem_path)
                .arg("-out")
                .arg(&out)
                .arg("-passout")
                .arg(format!("pass:{password}"))
                .args(extra)
                .output()
                .expect("openssl pkcs12");
            assert!(
                status.status.success(),
                "{extra:?}: {}",
                String::from_utf8_lossy(&status.stderr)
            );
            let data = std::fs::read(&out).expect("read p12");
            let imported = Identity::from_pkcs12(&data, password)
                .unwrap_or_else(|e| panic!("import {extra:?} failed: {e}"));
            assert_eq!(imported.cert_hash(), TESTER.cert_hash(), "{extra:?}");
            assert_eq!(imported.name(), "Tester");
        }

        // The other direction: OpenSSL (and therefore the official client) reads our export.
        let ours = dir.path().join("ours.p12");
        std::fs::write(&ours, TESTER.to_pkcs12("pw").expect("export")).expect("write");
        let check = Command::new("openssl")
            .args(["pkcs12", "-noout", "-passin", "pass:pw", "-in"])
            .arg(&ours)
            .output()
            .expect("openssl read");
        assert!(
            check.status.success(),
            "{}",
            String::from_utf8_lossy(&check.stderr)
        );
    }
}
