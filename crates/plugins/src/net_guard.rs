//! Origin enforcement for `net.http` (FR-9.27, FR-9.28): which declared origin a request URL
//! belongs to, whether the addresses a name resolves to are public, and the per-day request
//! counters behind "Requests today" in the plugin details (FR-9.30).
//!
//! The address check runs after DNS, and the request then connects only to the addresses that
//! were checked, so a name that resolves to a public address for the check and to a local one
//! for the connection (DNS rebinding) cannot reach the local network.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use mumcord_store::{Store, UnixMillis};
use reqwest::Url;

use crate::origins::{Origin, Scheme};
use crate::test_hooks;

/// Counter rows are kept for this many days, today included.
const KEPT_DAYS: i64 = 7;

/// The E2E hooks of the origin checks (spec §10.2). They come from [`test_hooks`], which keeps
/// them `None` in release builds; unit tests set them directly.
#[derive(Debug, Clone)]
pub(crate) struct NetHooks {
    /// An extra trusted root certificate (PEM file) for the fixture server.
    pub test_ca: Option<PathBuf>,
    /// The fixture server's address, which the address check treats as public.
    pub public_addr: Option<IpAddr>,
    /// Answers a name instead of DNS.
    pub resolve: fn(&str) -> Option<IpAddr>,
}

impl NetHooks {
    pub(crate) fn from_env() -> Self {
        Self {
            test_ca: test_hooks::test_ca(),
            public_addr: test_hooks::public_addr(),
            resolve: test_hooks::resolve,
        }
    }
}

/// Why a request to a declared origin was not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// The name resolved to an address that is not public (or not loopback for a developer
    /// `http://localhost` origin).
    Address,
    /// The name did not resolve at all.
    NotFound,
}

/// The declared origin `url` belongs to: same scheme, same host and same port. `wss` origins
/// never match, since `net.http` only speaks `http` and `https`.
pub(crate) fn match_origin<'a>(origins: &'a [Origin], url: &Url) -> Option<&'a Origin> {
    let scheme = match url.scheme() {
        "https" => Scheme::Https,
        "http" => Scheme::Http,
        _ => return None,
    };
    // `Url` already lower-cases and punycode-encodes the host, like the declared origins.
    let host = url.host_str()?;
    let port = url.port_or_known_default()?;
    origins
        .iter()
        .find(|o| o.scheme == scheme && o.host == host && o.effective_port() == port)
}

/// A unicast address on the public internet: not loopback, private, link-local, CGNAT,
/// multicast, broadcast, unspecified or reserved for documentation (FR-9.28).
pub(crate) fn is_public(ip: IpAddr, hooks: &NetHooks) -> bool {
    if hooks.public_addr == Some(ip) {
        return true;
    }
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    let cgnat = a == 100 && (64..128).contains(&b);
    // 0.0.0.0/8 ("this network") and 240.0.0.0/4 (reserved) are not reachable hosts either.
    let reserved = a == 0 || a >= 240;
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        || cgnat
        || reserved)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    let documentation = first == 0x2001 && ip.segments()[1] == 0x0db8;
    // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) embed an IPv4 address that may be local.
    let embeds_v4 = (first == 0x0064 && ip.segments()[1] == 0xff9b) || first == 0x2002;
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || documentation
        || embeds_v4)
}

/// Resolves the origin's host and checks every answer. One local answer blocks the whole
/// request, so a name cannot mix a public and a local address. A developer `http://localhost`
/// origin may only reach loopback, and `localhost` is never asked of DNS.
pub(crate) async fn resolve_checked(
    origin: &Origin,
    hooks: &NetHooks,
) -> Result<Vec<SocketAddr>, Blocked> {
    let port = origin.effective_port();
    let local_only = origin.scheme == Scheme::Http;
    let ips: Vec<IpAddr> = if let Some(ip) = (hooks.resolve)(&origin.host) {
        vec![ip]
    } else if local_only {
        vec![Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()]
    } else {
        tokio::net::lookup_host((origin.host.as_str(), port))
            .await
            .map_err(|_| Blocked::NotFound)?
            .map(|a| a.ip())
            .collect()
    };
    if ips.is_empty() {
        return Err(Blocked::NotFound);
    }
    let allowed = |ip: IpAddr| {
        if local_only {
            ip.is_loopback()
        } else {
            is_public(ip, hooks)
        }
    };
    if !ips.iter().all(|ip| allowed(*ip)) {
        return Err(Blocked::Address);
    }
    Ok(ips
        .into_iter()
        .map(|ip| SocketAddr::new(ip, port))
        .collect())
}

/// Writes the per-plugin, per-day request counters to the database. Called on the HTTP worker
/// (through `spawn_blocking`), never on a plugin's thread.
#[derive(Default)]
pub(crate) struct Counters {
    store: Option<Arc<Mutex<Store>>>,
    /// The day old rows were last pruned, so pruning runs once per day.
    pruned: Mutex<Option<String>>,
}

impl std::fmt::Debug for Counters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Counters")
            .field("store", &self.store.is_some())
            .finish_non_exhaustive()
    }
}

impl Counters {
    pub(crate) fn new(store: Option<Arc<Mutex<Store>>>) -> Self {
        Self {
            store,
            pruned: Mutex::new(None),
        }
    }

    /// Counts one request of `plugin` to `host` today (local time; the details say "today").
    /// Failures are logged without the host.
    pub(crate) fn record(&self, plugin: &str, host: &str, blocked: bool) {
        let Some(store) = &self.store else {
            return;
        };
        let now = chrono::Local::now();
        let day = now.format("%Y-%m-%d").to_string();
        let at: UnixMillis = now.timestamp_millis();
        let store = store.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(error) = store.record_plugin_request(plugin, &day, host, blocked, at) {
            tracing::warn!(plugin, %error, "can't count a plugin request");
        }
        let mut pruned = self.pruned.lock().unwrap_or_else(PoisonError::into_inner);
        if pruned.as_deref() != Some(day.as_str()) {
            let oldest = (now.date_naive() - chrono::Days::new(KEPT_DAYS as u64 - 1))
                .format("%Y-%m-%d")
                .to_string();
            match store.prune_plugin_origin_counters(&oldest) {
                Ok(_) => *pruned = Some(day),
                Err(error) => tracing::warn!(%error, "can't prune the plugin request counters"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origins;

    fn no_hooks() -> NetHooks {
        NetHooks {
            test_ca: None,
            public_addr: None,
            resolve: |_| None,
        }
    }

    fn origin(text: &str, developer: bool) -> Origin {
        origins::validate(text, developer).expect("valid origin")
    }

    fn url(text: &str) -> Url {
        Url::parse(text).expect("valid url")
    }

    #[test]
    fn only_the_exact_scheme_host_and_port_match() {
        let declared = [
            origin("https://api.example.com", false),
            origin("https://media.example.com:8443", false),
            origin("wss://live.example.com", false),
        ];
        let found = |u: &str| match_origin(&declared, &url(u)).map(Origin::ascii);
        assert_eq!(
            found("https://API.example.com/v1?q=1"),
            Some("https://api.example.com".into())
        );
        assert_eq!(
            found("https://api.example.com:443/"),
            Some("https://api.example.com".into())
        );
        assert_eq!(
            found("https://media.example.com:8443/x"),
            Some("https://media.example.com:8443".into())
        );
        for refused in [
            "https://api.example.com:8443/",
            "http://api.example.com/",
            "https://media.example.com/",
            "https://example.com/",
            "https://evil.api.example.com/",
            "https://api.example.com.evil.net/",
            "https://live.example.com/",
        ] {
            assert_eq!(found(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_developer_localhost_origin_matches_only_its_port() {
        let declared = [origin("http://localhost:5173", true)];
        assert!(match_origin(&declared, &url("http://localhost:5173/api")).is_some());
        assert!(match_origin(&declared, &url("http://localhost:5174/")).is_none());
        assert!(match_origin(&declared, &url("https://localhost:5173/")).is_none());
        assert!(match_origin(&declared, &url("http://127.0.0.1:5173/")).is_none());
    }

    #[test]
    fn only_public_unicast_addresses_are_public() {
        let hooks = no_hooks();
        for public in [
            "93.184.216.34",
            "1.1.1.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            let ip: IpAddr = public.parse().expect("ip");
            assert!(is_public(ip, &hooks), "{public}");
        }
        for local in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.254",
            "224.0.0.1",
            "255.255.255.255",
            "0.0.0.0",
            "240.0.0.1",
            "192.0.2.1",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.0.1",
            "64:ff9b::7f00:1",
            "2002:7f00:1::1",
        ] {
            let ip: IpAddr = local.parse().expect("ip");
            assert!(!is_public(ip, &hooks), "{local}");
        }
    }

    #[test]
    fn the_fixture_address_hook_counts_as_public() {
        let hooks = NetHooks {
            public_addr: Some(Ipv4Addr::LOCALHOST.into()),
            ..no_hooks()
        };
        assert!(is_public(Ipv4Addr::LOCALHOST.into(), &hooks));
        assert!(!is_public(Ipv4Addr::new(10, 0, 0, 1).into(), &hooks));
    }

    #[tokio::test]
    async fn a_declared_name_resolving_to_a_local_address_is_blocked() {
        let hooks = NetHooks {
            resolve: |name| (name == "api.example.com").then_some(Ipv4Addr::LOCALHOST.into()),
            ..no_hooks()
        };
        let declared = origin("https://api.example.com", false);
        assert_eq!(
            resolve_checked(&declared, &hooks).await,
            Err(Blocked::Address)
        );
        let public = NetHooks {
            public_addr: Some(Ipv4Addr::LOCALHOST.into()),
            ..hooks
        };
        assert_eq!(
            resolve_checked(&declared, &public).await,
            Ok(vec![SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 443)])
        );
    }

    #[tokio::test]
    async fn developer_localhost_reaches_only_loopback() {
        let declared = origin("http://localhost:5173", true);
        let addrs = resolve_checked(&declared, &no_hooks())
            .await
            .expect("loopback");
        assert!(
            addrs
                .iter()
                .all(|a| a.ip().is_loopback() && a.port() == 5173)
        );
        let rebound = NetHooks {
            resolve: |_| Some(Ipv4Addr::new(93, 184, 216, 34).into()),
            ..no_hooks()
        };
        assert_eq!(
            resolve_checked(&declared, &rebound).await,
            Err(Blocked::Address)
        );
    }

    #[test]
    fn requests_and_blocked_ones_are_counted_per_day() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Mutex::new(Store::open(dir.path()).expect("store")));
        let counters = Counters::new(Some(Arc::clone(&store)));
        counters.record("weather", "api.example.com", false);
        counters.record("weather", "evil.example.net", true);
        counters.record("other", "api.example.com", false);
        let rows = store
            .lock()
            .expect("lock")
            .plugin_origin_counters("weather")
            .expect("counters");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].day,
            chrono::Local::now().format("%Y-%m-%d").to_string()
        );
        assert_eq!((rows[0].requests, rows[0].blocked), (2, 1));
        assert_eq!(rows[0].last_host.as_deref(), Some("evil.example.net"));
        Counters::default().record("weather", "api.example.com", true);
    }
}
