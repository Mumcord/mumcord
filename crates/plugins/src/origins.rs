//! The websites a plugin declares in `net.origins` (FR-9.25, FR-9.26). Origins are exact
//! (`https://host[:port]` or `wss://host[:port]`) and shown in ASCII form. A package breaking a
//! rule is a hard stop (FR-9.78): it cannot be installed and gets no score.

use std::net::{Ipv4Addr, Ipv6Addr};

/// At most this many origins per plugin (FR-9.25).
pub const MAX_ORIGINS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Https,
    Wss,
    /// Only `http://localhost:<port>` in Developer mode (FR-9.26).
    Http,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Https => "https",
            Scheme::Wss => "wss",
            Scheme::Http => "http",
        }
    }
}

/// A declared origin in its canonical form: lower-case ASCII host, default port dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub scheme: Scheme,
    pub host: String,
    pub port: Option<u16>,
    /// The host contains punycode, so the prompt adds the line "Unusual characters".
    pub unusual: bool,
}

impl Origin {
    /// The text stored and shown, e.g. `https://api.example:8443`.
    pub fn ascii(&self) -> String {
        match self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme.as_str(), self.host),
            None => format!("{}://{}", self.scheme.as_str(), self.host),
        }
    }

    /// The port a connection uses.
    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(match self.scheme {
            Scheme::Https | Scheme::Wss => 443,
            Scheme::Http => 80,
        })
    }
}

/// Why an origin is refused. The text completes "Cannot be installed: it wants to use a website
/// that ..." style prompts (FR-9.78).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HardStop {
    #[error("it wants to use a website that is not encrypted")]
    NotEncrypted,
    #[error("it wants to use a website address with a user name or password")]
    UserInfo,
    #[error("it wants to use a website address with a path, query or fragment")]
    PathQueryFragment,
    #[error("it wants to use a whole group of websites (wildcard)")]
    Wildcard,
    #[error("it wants to use a website given as a number (IP address)")]
    IpAddress,
    #[error("it wants to use a website on your own computer or network")]
    LocalName,
    #[error("it wants to use a website address that is not valid")]
    Malformed,
    #[error("it wants to use more than {MAX_ORIGINS} websites")]
    TooMany,
}

/// Checks one declared origin. `developer` allows the `http://localhost:<port>` exception.
pub fn validate(input: &str, developer: bool) -> Result<Origin, HardStop> {
    if input.is_empty() || input.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(HardStop::Malformed);
    }
    let (scheme, rest) = if let Some(rest) = input.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else if let Some(rest) = input.strip_prefix("wss://") {
        (Scheme::Wss, rest)
    } else if let Some(rest) = input.strip_prefix("http://") {
        (Scheme::Http, rest)
    } else if input.contains("://") {
        return Err(HardStop::NotEncrypted);
    } else {
        return Err(HardStop::Malformed);
    };

    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    if authority.contains('@') {
        return Err(HardStop::UserInfo);
    }
    if !tail.is_empty() {
        return Err(HardStop::PathQueryFragment);
    }
    if authority.contains('*') {
        return Err(HardStop::Wildcard);
    }
    if authority.starts_with('[') {
        return Err(HardStop::IpAddress);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(parse_port(port)?)),
        None => (authority, None),
    };
    if !host.is_ascii() {
        return Err(HardStop::Malformed);
    }
    let host = host.to_ascii_lowercase();

    if scheme == Scheme::Http {
        // Only the Developer-mode localhost exception is plain text.
        if !(developer && host == "localhost" && port.is_some()) {
            return Err(HardStop::NotEncrypted);
        }
        return Ok(Origin {
            scheme,
            host,
            port,
            unusual: false,
        });
    }

    check_host(&host)?;
    let unusual = host.split('.').any(|label| label.starts_with("xn--"));
    let port = port.filter(|&p| p != 443);
    Ok(Origin {
        scheme,
        host,
        port,
        unusual,
    })
}

fn parse_port(port: &str) -> Result<u16, HardStop> {
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return Err(HardStop::Malformed);
    }
    match port.parse::<u16>() {
        Ok(p) if p > 0 => Ok(p),
        _ => Err(HardStop::Malformed),
    }
}

/// Name suffixes that never resolve on the public internet.
const LOCAL_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "internal",
    "lan",
    "localdomain",
    "home.arpa",
    "intranet",
    "corp",
    "home",
];

fn check_host(host: &str) -> Result<(), HardStop> {
    if host.is_empty() || host.len() > 253 {
        return Err(HardStop::Malformed);
    }
    if host.parse::<Ipv4Addr>().is_ok() || host.parse::<Ipv6Addr>().is_ok() {
        return Err(HardStop::IpAddress);
    }
    let labels: Vec<&str> = host.split('.').collect();
    for label in &labels {
        let valid = !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid || label.strip_prefix("xn--").is_some_and(str::is_empty) {
            return Err(HardStop::Malformed);
        }
    }
    // A public name never ends in a number, so this catches short forms such as `127.1` or
    // `0x7f.1` that some resolvers read as IPv4.
    let last = labels.last().copied().unwrap_or_default();
    if last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x") {
        return Err(HardStop::IpAddress);
    }
    let local = labels.len() < 2
        || LOCAL_SUFFIXES
            .iter()
            .any(|s| host == *s || host.ends_with(&format!(".{s}")));
    if local {
        return Err(HardStop::LocalName);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str) -> Origin {
        validate(input, false).unwrap_or_else(|e| panic!("{input}: {e}"))
    }

    #[test]
    fn accepts_exact_https_and_wss_origins() {
        assert_eq!(ok("https://api.example").ascii(), "https://api.example");
        assert_eq!(
            ok("wss://Live.Example:8443").ascii(),
            "wss://live.example:8443"
        );
        assert_eq!(ok("https://api.example:443").port, None);
        assert!(!ok("https://api.example").unusual);
    }

    #[test]
    fn punycode_is_flagged_as_unusual() {
        let origin = ok("https://xn--bcher-kva.example");
        assert!(origin.unusual);
        assert_eq!(origin.ascii(), "https://xn--bcher-kva.example");
        assert_eq!(
            validate("https://xn--.example", false),
            Err(HardStop::Malformed)
        );
        assert_eq!(
            validate("https://bücher.example", false),
            Err(HardStop::Malformed)
        );
    }

    #[test]
    fn rules_table() {
        let cases = [
            ("http://api.example", HardStop::NotEncrypted),
            ("ws://api.example", HardStop::NotEncrypted),
            ("ftp://api.example", HardStop::NotEncrypted),
            ("api.example", HardStop::Malformed),
            ("", HardStop::Malformed),
            ("https://", HardStop::Malformed),
            ("https://api.example/", HardStop::PathQueryFragment),
            ("https://api.example/v1", HardStop::PathQueryFragment),
            ("https://api.example?x=1", HardStop::PathQueryFragment),
            ("https://api.example#top", HardStop::PathQueryFragment),
            ("https://user@api.example", HardStop::UserInfo),
            ("https://user:pw@api.example", HardStop::UserInfo),
            ("https://*.example.com", HardStop::Wildcard),
            ("https://api.*", HardStop::Wildcard),
            ("https://1.2.3.4", HardStop::IpAddress),
            ("https://8.8.8.8:8443", HardStop::IpAddress),
            ("https://[::1]", HardStop::IpAddress),
            ("https://[2001:db8::1]:443", HardStop::IpAddress),
            ("https://127.1", HardStop::IpAddress),
            ("https://0x7f.1", HardStop::IpAddress),
            ("https://2130706433", HardStop::IpAddress),
            ("https://10.0.0.1", HardStop::IpAddress),
            ("https://169.254.169.254", HardStop::IpAddress),
            ("https://localhost", HardStop::LocalName),
            ("https://localhost:8443", HardStop::LocalName),
            ("https://app.localhost", HardStop::LocalName),
            ("https://printer.local", HardStop::LocalName),
            ("https://nas", HardStop::LocalName),
            ("https://db.internal", HardStop::LocalName),
            ("https://router.home.arpa", HardStop::LocalName),
            ("https://api.example:0", HardStop::Malformed),
            ("https://api.example:99999", HardStop::Malformed),
            ("https://api.example:", HardStop::Malformed),
            ("https://api..example", HardStop::Malformed),
            ("https://-api.example", HardStop::Malformed),
            ("https://api example", HardStop::Malformed),
        ];
        for (input, want) in cases {
            assert_eq!(validate(input, false), Err(want), "{input}");
            assert!(validate(input, true).is_err(), "{input} (developer)");
        }
    }

    #[test]
    fn developer_mode_allows_only_plain_localhost_with_a_port() {
        let origin = validate("http://localhost:5173", true).expect("developer exception");
        assert_eq!(origin.ascii(), "http://localhost:5173");
        assert_eq!(origin.effective_port(), 5173);
        assert_eq!(
            validate("http://localhost:5173", false),
            Err(HardStop::NotEncrypted)
        );
        assert_eq!(
            validate("http://localhost", true),
            Err(HardStop::NotEncrypted)
        );
        assert_eq!(
            validate("http://api.example:80", true),
            Err(HardStop::NotEncrypted)
        );
        assert_eq!(
            validate("http://127.0.0.1:80", true),
            Err(HardStop::NotEncrypted)
        );
    }
}
