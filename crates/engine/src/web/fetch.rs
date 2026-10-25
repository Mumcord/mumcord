//! Guarded HTTP fetching of links posted in chat (FR-1.49; reused for external images, FR-1.55).
//!
//! Everything a message links to is attacker-chosen, so every request is limited: http(s)
//! only, no cookies, no referrer, no proxy, a neutral User-Agent, short timeouts, a size cap
//! and at most three redirects. Private, loopback, link-local and other non-public addresses
//! are never contacted (SSRF guard): host names go through a resolver that drops such
//! addresses at connect time, which also covers DNS rebinding and every redirect hop, and IP
//! literals are checked before each request and each redirect.

use std::error::Error as StdError;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue, USER_AGENT};
use reqwest::{Client, Url, redirect};

/// HTML is read up to this size; metadata sits in `<head>`, so the rest is dropped.
pub const PAGE_LIMIT: usize = 512 * 1024;
/// Larger images are refused.
pub const IMAGE_LIMIT: usize = 2 * 1024 * 1024;
/// Image formats shown in chat, for the `Accept` header.
pub const IMAGE_ACCEPT: &str = "image/png,image/jpeg,image/gif,image/webp";
const MAX_REDIRECTS: usize = 3;
/// Generic on purpose: says what is asking without fingerprinting the user's system.
const AGENT: &str = "Mozilla/5.0 (compatible; Mumcord link preview)";

/// Which addresses may be contacted.
#[derive(Debug, Clone)]
pub enum AddressPolicy {
    /// Public internet addresses only (the app).
    PublicOnly,
    /// Exactly these addresses and nothing else (tests and E2E, which serve pages locally and
    /// must never reach the internet).
    Only(Arc<Vec<IpAddr>>),
}

impl AddressPolicy {
    pub fn permits(&self, ip: IpAddr) -> bool {
        match self {
            Self::PublicOnly => is_public(ip),
            Self::Only(list) => list.contains(&canonical(ip)),
        }
    }
}

/// Timeouts of one request; tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub connect: Duration,
    /// From connecting until the whole body was read, redirects included.
    pub total: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(4),
            total: Duration::from_secs(8),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Not an http(s) URL.
    InvalidUrl,
    /// The URL or a redirect points to an address that must not be contacted.
    Blocked,
    TooManyRedirects,
    TooLarge,
    Timeout,
    Status(u16),
    /// The response is not of the expected kind (e.g. not HTML, not a supported image).
    UnsupportedType,
    /// Connection, TLS or protocol failure.
    Network(String),
    /// The HTTP client could not be created (e.g. no OS certificate store).
    Setup(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => f.write_str("not an http(s) link"),
            Self::Blocked => f.write_str("address not allowed"),
            Self::TooManyRedirects => f.write_str("too many redirects"),
            Self::TooLarge => f.write_str("response too large"),
            Self::Timeout => f.write_str("timed out"),
            Self::Status(code) => write!(f, "HTTP status {code}"),
            Self::UnsupportedType => f.write_str("unsupported content type"),
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::Setup(e) => write!(f, "client setup failed: {e}"),
        }
    }
}

impl StdError for FetchError {}

/// Marker error raised inside the resolver and the redirect policy.
#[derive(Debug)]
struct BlockedAddress;

impl fmt::Display for BlockedAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("address not allowed")
    }
}

impl StdError for BlockedAddress {}

#[derive(Debug)]
struct RedirectLimit;

impl fmt::Display for RedirectLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("too many redirects")
    }
}

impl StdError for RedirectLimit {}

/// A fetched response body.
#[derive(Debug)]
pub struct Fetched {
    /// Final URL after redirects (base for relative links in the page).
    pub url: Url,
    /// Lower-case media type without parameters, e.g. `text/html`.
    pub media_type: String,
    pub body: Vec<u8>,
}

/// Image formats shown in chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageKind {
    /// Detects the format from the bytes, never from what the server claims.
    pub fn sniff(data: &[u8]) -> Option<Self> {
        if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            Some(Self::Gif)
        } else if data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
            Some(Self::Webp)
        } else {
            None
        }
    }
}

/// A fetched image of a supported format.
#[derive(Debug)]
pub struct FetchedImage {
    pub kind: ImageKind,
    pub bytes: Vec<u8>,
}

/// The guarded HTTP client. Cheap to clone.
#[derive(Clone)]
pub struct Fetcher {
    client: Client,
    policy: AddressPolicy,
}

impl Fetcher {
    pub fn new(policy: AddressPolicy, timeouts: Timeouts) -> Result<Self, FetchError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut tls = {
            use rustls_platform_verifier::BuilderVerifierExt;
            rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .and_then(|b| b.with_platform_verifier())
                .map_err(|e| FetchError::Setup(e.to_string()))?
                .with_no_client_auth()
        };
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let redirect_policy = policy.clone();
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .dns_resolver(Arc::new(GuardedResolver {
                policy: policy.clone(),
            }))
            .no_proxy()
            .referer(false)
            .connect_timeout(timeouts.connect)
            .timeout(timeouts.total)
            .redirect(redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() > MAX_REDIRECTS {
                    attempt.error(RedirectLimit)
                } else if check_url(attempt.url(), &redirect_policy).is_err() {
                    attempt.error(BlockedAddress)
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|e| FetchError::Setup(e.to_string()))?;
        Ok(Self { client, policy })
    }

    /// GETs `url` and reads at most `limit` bytes of the body. With `truncate` a longer body is
    /// cut off; otherwise it fails with [`FetchError::TooLarge`].
    pub async fn get(
        &self,
        url: &str,
        accept: &'static str,
        limit: usize,
        truncate: bool,
    ) -> Result<Fetched, FetchError> {
        let url = Url::parse(url).map_err(|_| FetchError::InvalidUrl)?;
        check_url(&url, &self.policy)?;
        let mut response = self
            .client
            .get(url)
            .header(USER_AGENT, HeaderValue::from_static(AGENT))
            .header(ACCEPT, HeaderValue::from_static(accept))
            .send()
            .await
            .map_err(map_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        let media_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase()
            })
            .unwrap_or_default();
        let declared = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        if !truncate && declared.is_some_and(|n| n > limit as u64) {
            return Err(FetchError::TooLarge);
        }
        let final_url = response.url().clone();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_error)? {
            let room = limit - body.len();
            if chunk.len() > room {
                if !truncate {
                    return Err(FetchError::TooLarge);
                }
                body.extend_from_slice(&chunk[..room]);
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Fetched {
            url: final_url,
            media_type,
            body,
        })
    }

    /// Fetches an HTML page (first [`PAGE_LIMIT`] bytes).
    pub async fn page(&self, url: &str) -> Result<Fetched, FetchError> {
        let page = self
            .get(
                url,
                "text/html,application/xhtml+xml;q=0.9,*/*;q=0.1",
                PAGE_LIMIT,
                true,
            )
            .await?;
        if is_html(&page.media_type) {
            Ok(page)
        } else {
            Err(FetchError::UnsupportedType)
        }
    }

    /// Fetches an image of at most [`IMAGE_LIMIT`] bytes in a supported format.
    pub async fn image(&self, url: &str) -> Result<FetchedImage, FetchError> {
        self.image_up_to(url, IMAGE_LIMIT).await
    }

    /// Fetches an image of at most `limit` bytes in a supported format. The format is sniffed
    /// from the bytes; the server's content type is ignored.
    pub async fn image_up_to(&self, url: &str, limit: usize) -> Result<FetchedImage, FetchError> {
        let fetched = self.get(url, IMAGE_ACCEPT, limit, false).await?;
        let kind = ImageKind::sniff(&fetched.body).ok_or(FetchError::UnsupportedType)?;
        Ok(FetchedImage {
            kind,
            bytes: fetched.body,
        })
    }
}

/// Whether a media type (lower case, without parameters) is an HTML page.
pub fn is_html(media_type: &str) -> bool {
    media_type == "text/html" || media_type == "application/xhtml+xml"
}

/// Only http(s) URLs with a host; IP literals must be allowed by the policy. Host names are
/// checked by the resolver when connecting.
pub fn check_url(url: &Url, policy: &AddressPolicy) -> Result<(), FetchError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::InvalidUrl);
    }
    let host = url.host_str().ok_or(FetchError::InvalidUrl)?;
    // The URL parser normalizes every IPv4 spelling (e.g. `2130706433`) to dotted form and
    // brackets IPv6 literals.
    match host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        Ok(ip) if !policy.permits(ip) => Err(FetchError::Blocked),
        _ => Ok(()),
    }
}

/// Resolves with the system resolver and keeps only permitted addresses.
struct GuardedResolver {
    policy: AddressPolicy,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.policy.clone();
        Box::pin(async move {
            let found = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let allowed: Vec<SocketAddr> = found.filter(|a| policy.permits(a.ip())).collect();
            if allowed.is_empty() {
                return Err(Box::new(BlockedAddress) as Box<dyn StdError + Send + Sync>);
            }
            let addrs: Addrs = Box::new(allowed.into_iter());
            Ok(addrs)
        })
    }
}

fn map_error(err: reqwest::Error) -> FetchError {
    let mut source: Option<&(dyn StdError + 'static)> = Some(&err);
    while let Some(e) = source {
        if e.is::<BlockedAddress>() {
            return FetchError::Blocked;
        }
        if e.is::<RedirectLimit>() {
            return FetchError::TooManyRedirects;
        }
        source = e.source();
    }
    if err.is_timeout() {
        FetchError::Timeout
    } else {
        // Without the URL: links are chat content and errors may end up in logs.
        FetchError::Network(err.without_url().to_string())
    }
}

/// IPv4-mapped IPv6 addresses are treated as the IPv4 address they carry.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

/// Globally routable unicast addresses only.
pub fn is_public(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0 // "this network"
        || a == 10
        || a == 127
        || (a == 100 && (64..128).contains(&b)) // carrier-grade NAT
        || (a == 169 && b == 254) // link-local
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2) // documentation
        || (a == 192 && b == 88 && c == 99) // 6to4 relay
        || (a == 192 && b == 168)
        || (a == 198 && (18..20).contains(&b)) // benchmarking
        || (a == 198 && b == 51 && c == 100) // documentation
        || (a == 203 && b == 0 && c == 113) // documentation
        || a >= 224) // multicast, reserved, broadcast
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    // Global unicast is 2000::/3; within it, Teredo/ORCHID (2001:0::/23 holds the special
    // ranges), documentation and 6to4 (which embeds arbitrary IPv4) are refused.
    (s[0] & 0xE000) == 0x2000
        && !(s[0] == 0x2001 && s[1] < 0x0200)
        && s[0..2] != [0x2001, 0x0DB8]
        && s[0] != 0x2002
        && s[0] != 0x3FFF // documentation (RFC 9637)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("ip")
    }

    #[test]
    fn public_addresses_only() {
        for private in [
            "127.0.0.1",
            "127.8.9.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.10",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "2001:db8::1",
            "2002:7f00:1::1",
            "2001::1",
        ] {
            assert!(!is_public(ip(private)), "{private} must be refused");
        }
        for public in [
            "1.1.1.1",
            "93.184.216.34",
            "172.32.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4001::1",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_public(ip(public)), "{public} must be allowed");
        }
    }

    #[test]
    fn checks_scheme_and_ip_literals() {
        let policy = AddressPolicy::PublicOnly;
        let check = |u: &str| check_url(&Url::parse(u).expect("url"), &policy);
        assert_eq!(check("ftp://example.com/"), Err(FetchError::InvalidUrl));
        assert_eq!(check("file:///etc/passwd"), Err(FetchError::InvalidUrl));
        assert_eq!(check("http://127.0.0.1/"), Err(FetchError::Blocked));
        assert_eq!(check("http://[::1]:8080/"), Err(FetchError::Blocked));
        assert_eq!(
            check("http://[::ffff:192.168.0.1]/"),
            Err(FetchError::Blocked)
        );
        assert_eq!(check("http://2130706433/"), Err(FetchError::Blocked));
        assert_eq!(check("https://example.com/a"), Ok(()));
        assert_eq!(check("http://1.1.1.1/"), Ok(()));
    }

    #[test]
    fn sniffs_image_formats() {
        assert_eq!(
            ImageKind::sniff(b"\x89PNG\r\n\x1a\nrest"),
            Some(ImageKind::Png)
        );
        assert_eq!(ImageKind::sniff(b"GIF89a.."), Some(ImageKind::Gif));
        assert_eq!(ImageKind::sniff(b"<svg onload=x>"), None);
    }
}
