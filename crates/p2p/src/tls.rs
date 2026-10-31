//! Mutual TLS for direct connections: both sides present their Mumble client certificate and
//! accept the other one only if its hash equals the hash the Mumble server reported for that
//! user. There is no CA and no date check (Mumble certificates are self-signed and long-lived);
//! handshake signatures are still verified, so the peer must really own the presented
//! certificate.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mumcord_protocol::cert_hash;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

/// ALPN of the direct connection protocol; a QUIC peer speaking anything else is refused.
const ALPN: &[u8] = b"mumcord-p2p/1";
/// ALPN of the relay handshake (spec 04 section 6.5), on QUIC and on TLS over TCP alike.
const RELAY_ALPN: &[u8] = b"mumpeer/1";

/// Accepts exactly one certificate, identified by its Mumble hash. Remembers a mismatch so the
/// caller can tell "wrong person" apart from "no path", and wakes [`Self::mismatched`] so the
/// attempt ends at once instead of after the connect timeout.
#[derive(Debug)]
pub(crate) struct PinnedPeer {
    hash: String,
    provider: Arc<CryptoProvider>,
    mismatch: AtomicBool,
    mismatch_seen: Notify,
}

impl PinnedPeer {
    pub(crate) fn new(hash: String) -> Arc<Self> {
        Arc::new(Self {
            hash,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
            mismatch: AtomicBool::new(false),
            mismatch_seen: Notify::new(),
        })
    }

    pub(crate) fn saw_mismatch(&self) -> bool {
        self.mismatch.load(Ordering::Relaxed)
    }

    /// Resolves once a presented certificate did not match (also when that happened earlier).
    pub(crate) async fn mismatched(&self) {
        self.mismatch_seen.notified().await;
    }

    /// The other side refused our certificate: it expects someone else, so this attempt cannot
    /// succeed either.
    pub(crate) fn refused_by_peer(&self) {
        self.mismatch.store(true, Ordering::Relaxed);
        self.mismatch_seen.notify_one();
    }

    fn check(&self, end_entity: &CertificateDer<'_>) -> Result<(), rustls::Error> {
        if cert_hash(end_entity.as_ref()) == self.hash {
            Ok(())
        } else {
            self.mismatch.store(true, Ordering::Relaxed);
            // `notify_one` keeps a permit, so a waiter that starts later still wakes.
            self.mismatch_seen.notify_one();
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

impl ServerCertVerifier for PinnedPeer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.check(end_entity)
            .map(|()| ServerCertVerified::assertion())
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
        self.schemes()
    }
}

impl ClientCertVerifier for PinnedPeer {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        self.check(end_entity)
            .map(|()| ClientCertVerified::assertion())
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
        self.schemes()
    }
}

/// Accepts exactly the relay certificate whose SHA-256 the relay bot announced. The relay has no
/// CA-signed certificate and may be reached by IP, so neither WebPKI nor the name is checked.
#[derive(Debug)]
pub(crate) struct RelayPin {
    fingerprint: [u8; 32],
    provider: Arc<CryptoProvider>,
    mismatch: AtomicBool,
}

impl RelayPin {
    pub(crate) fn new(fingerprint: [u8; 32]) -> Arc<Self> {
        Arc::new(Self {
            fingerprint,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
            mismatch: AtomicBool::new(false),
        })
    }

    /// Whether a relay presented a different certificate than the announced one, so a failed
    /// attempt can be told apart from an unreachable relay.
    pub(crate) fn saw_mismatch(&self) -> bool {
        self.mismatch.load(Ordering::Relaxed)
    }
}

impl ServerCertVerifier for RelayPin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if Sha256::digest(end_entity.as_ref()).as_slice() == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            self.mismatch.store(true, Ordering::Relaxed);
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

/// TLS 1.3 client config for the outer relay link (QUIC and TCP): the user's Mumble certificate
/// as client certificate, because the relay admits only the hashes of the token's pair.
pub(crate) fn relay_client_config(
    chain: &[CertificateDer<'static>],
    key: &PrivateKeyDer<'static>,
    pin: &Arc<RelayPin>,
) -> Result<Arc<rustls::ClientConfig>, rustls::Error> {
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])?
    .dangerous()
    .with_custom_certificate_verifier(pin.clone())
    .with_client_auth_cert(chain.to_vec(), key.clone_key())?;
    config.alpn_protocols = vec![RELAY_ALPN.to_vec()];
    Ok(Arc::new(config))
}

/// QUIC client and server configs for one connection attempt, both pinned to `peer`.
pub(crate) fn quic_configs(
    chain: &[CertificateDer<'static>],
    key: &PrivateKeyDer<'static>,
    peer: &Arc<PinnedPeer>,
) -> Result<(quinn::ClientConfig, quinn::ServerConfig), rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    // QUIC requires TLS 1.3.
    let versions = &[&rustls::version::TLS13];

    let mut client = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(versions)?
        .dangerous()
        .with_custom_certificate_verifier(peer.clone())
        .with_client_auth_cert(chain.to_vec(), key.clone_key())?;
    client.alpn_protocols = vec![ALPN.to_vec()];

    let mut server = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(versions)?
        .with_client_cert_verifier(peer.clone())
        .with_single_cert(chain.to_vec(), key.clone_key())?;
    server.alpn_protocols = vec![ALPN.to_vec()];

    let client = quinn::crypto::rustls::QuicClientConfig::try_from(client)
        .map_err(|e| rustls::Error::General(e.to_string()))?;
    let server = quinn::crypto::rustls::QuicServerConfig::try_from(server)
        .map_err(|e| rustls::Error::General(e.to_string()))?;
    Ok((
        quinn::ClientConfig::new(Arc::new(client)),
        quinn::ServerConfig::with_crypto(Arc::new(server)),
    ))
}
