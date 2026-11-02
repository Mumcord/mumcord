//! The HTTP side (FR-6.57): `User-Agent: Mumcord`, no cookies, no referer, no identifiers, size
//! caps and the https rule (FR-6.74). Bodies are never logged.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ETAG, IF_NONE_MATCH, USER_AGENT};
use reqwest::{StatusCode, Url, redirect};

use super::{MAX_SIGNATURE_BYTES, RegistryError, SIGNATURE_SUFFIX};
use crate::verify::VerifyFailure;

const USER_AGENT_VALUE: &str = "Mumcord";
const MAX_REDIRECTS: usize = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(120);
/// Slowest connection a download is still expected to finish on, so a 64 MiB official package
/// gets minutes instead of the two a small index gets.
const MIN_BYTES_PER_SECOND: u64 = 256 * 1024;

/// Which addresses may be fetched: https always, plain http only for localhost and only in
/// Developer mode (FR-6.74), which is also how a local test registry is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UrlPolicy {
    pub allow_localhost_http: bool,
}

impl UrlPolicy {
    pub fn new(developer_mode: bool) -> Self {
        Self {
            allow_localhost_http: developer_mode,
        }
    }

    pub fn check(&self, url: &Url) -> Result<(), RegistryError> {
        // Credentials in the address would be sent to the server: no identifiers (FR-6.57).
        if !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() {
            return Err(RegistryError::InvalidUrl);
        }
        match url.scheme() {
            "https" => Ok(()),
            "http" if self.allow_localhost_http && is_localhost(url) => Ok(()),
            "http" => Err(RegistryError::InsecureUrl),
            _ => Err(RegistryError::InvalidUrl),
        }
    }
}

fn is_localhost(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// A downloaded package plus its author signature (`<url>.minisig`), ready for
/// `verify::verify_package`.
#[derive(Debug)]
pub struct Download {
    pub bytes: Vec<u8>,
    pub signature: String,
}

pub(super) enum Response {
    NotModified,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

/// The registry HTTP client. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    policy: UrlPolicy,
}

impl Client {
    pub fn new(policy: UrlPolicy) -> Result<Self, RegistryError> {
        use rustls_platform_verifier::BuilderVerifierExt;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|builder| builder.with_platform_verifier())
            .map_err(|e| RegistryError::Setup(e.to_string()))?
            .with_no_client_auth();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .referer(false)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            .redirect(redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() > MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if policy.check(attempt.url()).is_err() {
                    attempt.error("redirect to a forbidden address")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|e| RegistryError::Setup(e.to_string()))?;
        Ok(Self { http, policy })
    }

    pub fn policy(&self) -> UrlPolicy {
        self.policy
    }

    /// GET with an optional ETag; bodies over `max` bytes are refused.
    pub(super) async fn get(
        &self,
        url: &Url,
        max: usize,
        etag: Option<&str>,
    ) -> Result<Response, RegistryError> {
        self.get_with_progress(url, max, etag, &mut |_| {}).await
    }

    /// [`Self::get`], calling `on_bytes` with the bytes received so far after every chunk.
    async fn get_with_progress(
        &self,
        url: &Url,
        max: usize,
        etag: Option<&str>,
        on_bytes: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Response, RegistryError> {
        self.policy.check(url)?;
        let mut request = self
            .http
            .get(url.clone())
            .timeout(TOTAL_TIMEOUT.max(Duration::from_secs(max as u64 / MIN_BYTES_PER_SECOND)))
            .header(USER_AGENT, USER_AGENT_VALUE);
        if let Some(etag) = etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let mut response = request.send().await.map_err(|e| {
            tracing::debug!(host = url.host_str(), "plugin registry request failed: {e}");
            RegistryError::Unreachable
        })?;
        let status = response.status();
        if status == StatusCode::NOT_MODIFIED && etag.is_some() {
            return Ok(Response::NotModified);
        }
        if status != StatusCode::OK {
            tracing::debug!(
                host = url.host_str(),
                status = status.as_u16(),
                "plugin registry"
            );
            return Err(RegistryError::Status(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > max as u64)
        {
            return Err(RegistryError::TooLarge(max));
        }
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| RegistryError::Unreachable)?
        {
            if bytes.len() + chunk.len() > max {
                return Err(RegistryError::TooLarge(max));
            }
            bytes.extend_from_slice(&chunk);
            on_bytes(bytes.len() as u64);
        }
        Ok(Response::Body { bytes, etag })
    }

    /// GET without caching; a 304 cannot happen without an ETag.
    pub(super) async fn get_body(&self, url: &Url, max: usize) -> Result<Vec<u8>, RegistryError> {
        match self.get(url, max, None).await? {
            Response::Body { bytes, .. } => Ok(bytes),
            Response::NotModified => Err(RegistryError::Status(304)),
        }
    }

    /// The detached signature next to `url`.
    pub(super) async fn get_signature(&self, url: &Url) -> Result<String, RegistryError> {
        let bytes = self
            .get_body(&signature_url(url), MAX_SIGNATURE_BYTES)
            .await?;
        String::from_utf8(bytes).map_err(|_| VerifyFailure::Signature.into())
    }

    /// Downloads a package of the size the index promised, plus its author signature (B14's
    /// `<id>-<ver>.mcplugin.minisig` next to the package). A larger body is a mismatch.
    pub async fn download(&self, url: &str, size: u64) -> Result<Download, RegistryError> {
        self.download_with_progress(url, size, &mut |_| {}).await
    }

    /// [`Self::download`], calling `on_bytes` with the package bytes received so far.
    pub async fn download_with_progress(
        &self,
        url: &str,
        size: u64,
        on_bytes: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Download, RegistryError> {
        let url = Url::parse(url).map_err(|_| RegistryError::InvalidUrl)?;
        let max = usize::try_from(size).map_err(|_| VerifyFailure::Mismatch)?;
        let bytes = match self.get_with_progress(&url, max, None, on_bytes).await {
            Ok(Response::Body { bytes, .. }) => bytes,
            Ok(Response::NotModified) => return Err(RegistryError::Status(304)),
            Err(RegistryError::TooLarge(_)) => return Err(VerifyFailure::Mismatch.into()),
            Err(error) => return Err(error),
        };
        let signature = self.get_signature(&url).await?;
        Ok(Download { bytes, signature })
    }
}

pub(super) fn signature_url(url: &Url) -> Url {
    let mut signed = url.clone();
    signed.set_path(&format!("{}{SIGNATURE_SUFFIX}", url.path()));
    signed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).expect("test step")
    }

    #[test]
    fn url_policy() {
        let normal = UrlPolicy::new(false);
        let developer = UrlPolicy::new(true);
        assert!(normal.check(&url("https://example.org/index.json")).is_ok());
        assert!(matches!(
            normal.check(&url("http://localhost:8080/index.json")),
            Err(RegistryError::InsecureUrl)
        ));
        assert!(
            developer
                .check(&url("http://localhost:8080/index.json"))
                .is_ok()
        );
        assert!(developer.check(&url("http://127.0.0.1/index.json")).is_ok());
        assert!(developer.check(&url("http://[::1]/index.json")).is_ok());
        assert!(
            developer
                .check(&url("http://example.org/index.json"))
                .is_err()
        );
        assert!(normal.check(&url("https://user:pw@example.org/")).is_err());
        assert!(normal.check(&url("ftp://example.org/")).is_err());
    }

    #[test]
    fn signature_url_appends_suffix_to_the_path() {
        assert_eq!(
            signature_url(&url("https://x.org/a/index.json?x=1")).as_str(),
            "https://x.org/a/index.json.minisig?x=1"
        );
    }
}
