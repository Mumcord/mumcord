//! TLS setup: client certificate authentication and a server-certificate check that hands the
//! decision to the caller. Mumble servers are usually self-signed, so instead of CA validation the
//! caller pins the fingerprint (trust on first use). Handshake signatures are still verified, so
//! the peer must really own the presented certificate.

use std::fmt;
use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use sha1::{Digest, Sha1};
use x509_parser::prelude::FromDer;
use x509_parser::prelude::X509Certificate;

/// The server's certificate as presented in the TLS handshake.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerCert {
    /// DER of the leaf certificate followed by any intermediates.
    pub chain: Vec<CertificateDer<'static>>,
    /// Lower-case hex SHA-1 of the leaf DER (the fingerprint Mumble shows).
    pub sha1_hex: String,
    /// Lower-case hex SHA-256 of the leaf DER.
    pub sha256_hex: String,
    /// Issuer distinguished name as text; `None` if the DER could not be parsed.
    pub issuer: Option<String>,
    /// Start of the validity period, RFC 2822; `None` if unknown.
    pub valid_from: Option<String>,
    /// End of the validity period, RFC 2822; `None` if unknown.
    pub valid_until: Option<String>,
    /// PEM of the leaf certificate, for the "Show certificate" dialog.
    pub pem: String,
}

impl fmt::Debug for ServerCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerCert")
            .field("sha1_hex", &self.sha1_hex)
            .field("sha256_hex", &self.sha256_hex)
            .field("issuer", &self.issuer)
            .field("valid_from", &self.valid_from)
            .field("valid_until", &self.valid_until)
            .finish_non_exhaustive()
    }
}

impl ServerCert {
    fn new(end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>]) -> Self {
        let der = end_entity.as_ref();
        let details = parse_cert_details(der);
        let mut chain = vec![end_entity.clone().into_owned()];
        chain.extend(intermediates.iter().map(|c| c.clone().into_owned()));
        Self {
            chain,
            sha1_hex: hex(&Sha1::digest(der)),
            sha256_hex: hex(&sha256(der)),
            issuer: details.issuer,
            valid_from: details.valid_from,
            valid_until: details.valid_until,
            pem: pem(der),
        }
    }
}

/// Human-readable parts of an X.509 certificate for the server info panel.
struct CertDetails {
    issuer: Option<String>,
    valid_from: Option<String>,
    valid_until: Option<String>,
}

fn parse_cert_details(der: &[u8]) -> CertDetails {
    let Ok((_, cert)) = X509Certificate::from_der(der) else {
        return CertDetails {
            issuer: None,
            valid_from: None,
            valid_until: None,
        };
    };
    let validity = cert.validity();
    CertDetails {
        issuer: Some(cert.issuer().to_string()),
        valid_from: validity.not_before.to_rfc2822().ok(),
        valid_until: validity.not_after.to_rfc2822().ok(),
    }
}

/// PEM encoding of a DER certificate, wrapped at 64 columns.
fn pem(der: &[u8]) -> String {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = String::with_capacity(encoded.len() + 64);
    out.push_str("-----BEGIN CERTIFICATE-----\n");
    for chunk in encoded.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

/// Caller-supplied trust decision: return `true` to accept the server certificate.
pub type CertVerifier = Arc<dyn Fn(&ServerCert) -> bool + Send + Sync>;

/// The user's identity certificate and key (DER).
pub struct ClientIdentity {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}

impl fmt::Debug for ClientIdentity {
    // Never print the private key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientIdentity")
            .field("chain_len", &self.chain.len())
            .finish_non_exhaustive()
    }
}

/// Lower-case hex SHA-1 of a certificate DER; this is the user hash Mumble servers report in
/// `UserState.hash` for a client certificate.
pub fn cert_hash(der: &[u8]) -> String {
    hex(&Sha1::digest(der))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(data: &[u8]) -> Vec<u8> {
    // SHA-256 via the ring provider rustls already ships with, to avoid another hash crate.
    let suite = rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256;
    match suite.tls13() {
        Some(tls13) => tls13.common.hash_provider.hash(data).as_ref().to_vec(),
        None => unreachable!("TLS13_AES_128_GCM_SHA256 is a TLS 1.3 suite"),
    }
}

#[derive(Debug)]
struct Verifier {
    decide: CertVerifierDebug,
    provider: Arc<CryptoProvider>,
    /// Last certificate seen, so a rejection can be reported with its details.
    seen: Mutex<Option<ServerCert>>,
}

struct CertVerifierDebug(CertVerifier);

impl fmt::Debug for CertVerifierDebug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CertVerifier")
    }
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let cert = ServerCert::new(end_entity, intermediates);
        let accepted = (self.decide.0)(&cert);
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some(cert);
        }
        if accepted {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Handle to read back the certificate the verifier saw.
pub(crate) struct SeenCert(Arc<Verifier>);

impl SeenCert {
    pub(crate) fn take(&self) -> Option<ServerCert> {
        self.0.seen.lock().ok().and_then(|mut s| s.take())
    }
}

pub(crate) fn client_config(
    identity: Option<ClientIdentity>,
    decide: CertVerifier,
) -> Result<(Arc<ClientConfig>, SeenCert), rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(Verifier {
        decide: CertVerifierDebug(decide),
        provider: provider.clone(),
        seen: Mutex::new(None),
    });
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone());
    let config = match identity {
        Some(id) => builder.with_client_auth_cert(id.chain, id.key)?,
        None => builder.with_no_client_auth(),
    };
    Ok((Arc::new(config), SeenCert(verifier)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_hex() {
        assert_eq!(
            cert_hash(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn parses_certificate_details() {
        let generated =
            rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
        let der = generated.cert.der();
        let details = parse_cert_details(der.as_ref());
        assert!(details.issuer.is_some());
        assert!(details.valid_from.is_some());
        assert!(details.valid_until.is_some());
        let pem = pem(der.as_ref());
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert!(pem.ends_with("-----END CERTIFICATE-----\n"));
    }
}
