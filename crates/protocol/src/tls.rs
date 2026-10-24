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

/// The server's certificate as presented in the TLS handshake.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerCert {
    /// DER of the leaf certificate followed by any intermediates.
    pub chain: Vec<CertificateDer<'static>>,
    /// Lower-case hex SHA-1 of the leaf DER (the fingerprint Mumble shows).
    pub sha1_hex: String,
    /// Lower-case hex SHA-256 of the leaf DER.
    pub sha256_hex: String,
}

impl fmt::Debug for ServerCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerCert")
            .field("sha1_hex", &self.sha1_hex)
            .field("sha256_hex", &self.sha256_hex)
            .finish_non_exhaustive()
    }
}

impl ServerCert {
    fn new(end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>]) -> Self {
        let mut chain = vec![end_entity.clone().into_owned()];
        chain.extend(intermediates.iter().map(|c| c.clone().into_owned()));
        Self {
            chain,
            sha1_hex: hex(&Sha1::digest(end_entity.as_ref())),
            sha256_hex: hex(&sha256(end_entity.as_ref())),
        }
    }
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
}
