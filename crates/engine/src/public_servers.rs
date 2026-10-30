//! The public server browser (issue #68, FR-3.91, FR-3.92).
//!
//! The list is the one the official Mumble client shows: an XML document from
//! `publist.mumble.info`. It is fetched only when the user opens the browser, never in the
//! background. The request names Mumcord honestly in its `User-Agent` and accepts gzip, which the
//! list server requires.
//!
//! Live figures (ping, users, user limit) come from the standard UDP ping of each server, the same
//! query the `mumble://` link card uses. The UI asks only for the rows on screen, and the engine
//! paces every ping through [`PingPacer`], so the servers are never contacted all at once.
//!
//! Everything in the list is untrusted: names are cleaned of control characters and cut to a
//! sane length, hosts must look like a host name or an IP address, and an entry that points to
//! a private, loopback or link-local address is dropped so the list cannot make the client probe
//! the user's own network.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use tokio::sync::{Mutex, Semaphore, SemaphorePermit};
use tokio::time::Instant;

use crate::server_query::{QueryOutcome, ServerQueries, ServerQuery};
use crate::web::fetch::{AddressPolicy, FetchError, Fetcher, Timeouts};

/// The list the official client uses.
const LIST_URL: &str = "https://publist.mumble.info/v1/list";
/// Tests and E2E serve a fixture list locally; the app never reads another address.
const URL_OVERRIDE_ENV: &str = "MUMCORD_PUBLIC_SERVERS_URL";
/// The list is about 50 KB for some 270 servers; anything far bigger is not that list.
const BODY_LIMIT: usize = 2 * 1024 * 1024;
/// More entries than this are ignored, so a broken list cannot exhaust memory in the UI.
const MAX_SERVERS: usize = 5000;
/// Longest server name kept, in characters.
const NAME_LIMIT: usize = 80;
/// Longest country name kept, in characters.
const COUNTRY_LIMIT: usize = 60;
/// Longest host name (DNS limit).
const HOST_LIMIT: usize = 253;
/// Time between two pings, like the official client's 50 ms.
pub const PING_SPACING: Duration = Duration::from_millis(50);
/// Pings waiting for an answer at the same time.
pub const PING_CONCURRENCY: usize = 6;

/// One entry of the public list, already cleaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicServer {
    pub name: String,
    pub host: String,
    pub port: u16,
    /// Country name as the list gives it; may be empty.
    pub country: String,
    /// Upper-case ISO 3166 alpha-2 code, or empty when the list has none.
    pub country_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PublicListError {
    #[error("fetching the public server list failed: {0}")]
    Fetch(#[from] FetchError),
    #[error("the public server list is not valid XML: {0}")]
    Malformed(String),
}

/// Parses the list document. Entries that cannot be used (no host, bad port, an IP address the
/// policy does not allow) are skipped; only a document that is not XML at all is an error.
pub fn parse_list(
    xml: &[u8],
    policy: &AddressPolicy,
) -> Result<Vec<PublicServer>, PublicListError> {
    let mut reader = Reader::from_reader(xml);
    let mut servers = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.name().as_ref() == "server" => {
                if let Some(server) = entry(&e, policy) {
                    servers.push(server);
                    if servers.len() == MAX_SERVERS {
                        break;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(PublicListError::Malformed(e.to_string())),
        }
    }
    // The same server listed twice would be pinged twice and shown twice.
    let mut seen = std::collections::HashSet::new();
    servers.retain(|s| seen.insert((s.host.to_ascii_lowercase(), s.port)));
    Ok(servers)
}

fn entry(e: &BytesStart<'_>, policy: &AddressPolicy) -> Option<PublicServer> {
    let (mut name, mut host, mut port, mut country, mut code) =
        (None, None, None, String::new(), String::new());
    for attr in e.attributes().flatten() {
        let Ok(value) = attr.normalized_value(XmlVersion::Implicit1_0) else {
            continue;
        };
        match attr.key.as_ref() {
            "name" => name = Some(clean_text(&value, NAME_LIMIT)),
            "ip" => host = clean_host(&value, policy),
            "port" => port = value.trim().parse::<u16>().ok().filter(|p| *p != 0),
            "country" => country = clean_text(&value, COUNTRY_LIMIT),
            "country_code" => code = clean_country_code(&value),
            _ => {}
        }
    }
    let host = host?;
    let name = name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| host.clone());
    Some(PublicServer {
        name,
        host,
        port: port?,
        country,
        country_code: code,
    })
}

/// Plain text without control or formatting characters (no bidi overrides that could disguise
/// a name), whitespace collapsed, cut to `limit` characters.
fn clean_text(value: &str, limit: usize) -> String {
    let mut out = String::new();
    for word in value
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(word.chars().filter(|c| !is_format_char(*c)));
    }
    if out.chars().count() > limit {
        out = out
            .chars()
            .take(limit)
            .collect::<String>()
            .trim_end()
            .to_owned();
        out.push('…');
    }
    out
}

/// Invisible characters that change how text is shown (bidi controls, zero-width marks).
fn is_format_char(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

/// A host name or IP address that may be contacted; `None` for anything else, including an IP
/// literal the policy refuses (for the real list: private, loopback, link-local and the like).
fn clean_host(value: &str, policy: &AddressPolicy) -> Option<String> {
    let host = value.trim().trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return policy.permits(ip).then(|| ip.to_string());
    }
    let valid = !host.is_empty()
        && host.len() <= HOST_LIMIT
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        && !host.eq_ignore_ascii_case("localhost");
    valid.then(|| host.to_ascii_lowercase())
}

fn clean_country_code(value: &str) -> String {
    let code = value.trim();
    if code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()) {
        code.to_ascii_uppercase()
    } else {
        String::new()
    }
}

/// Spaces pings out: one starts at most every `spacing`, and at most `concurrency` wait for an
/// answer at the same time. Holding the returned permit counts as one ping in flight.
#[derive(Debug)]
pub struct PingPacer {
    next: Mutex<Instant>,
    spacing: Duration,
    in_flight: Semaphore,
}

impl PingPacer {
    pub fn new(spacing: Duration, concurrency: usize) -> Self {
        Self {
            next: Mutex::new(Instant::now()),
            spacing,
            in_flight: Semaphore::new(concurrency),
        }
    }

    /// Waits for the next free slot.
    pub async fn slot(&self) -> SemaphorePermit<'_> {
        // The semaphore is never closed, so acquiring cannot fail.
        let permit = self
            .in_flight
            .acquire()
            .await
            .unwrap_or_else(|_| unreachable!("the ping semaphore is never closed"));
        let at = {
            let mut next = self.next.lock().await;
            let at = (*next).max(Instant::now());
            *next = at + self.spacing;
            at
        };
        tokio::time::sleep_until(at).await;
        permit
    }
}

/// The browser's state in the engine: where the list comes from and the ping pacing.
#[derive(Debug)]
pub struct PublicServers {
    url: String,
    policy: AddressPolicy,
    pacer: PingPacer,
    queries: Arc<ServerQueries>,
}

impl PublicServers {
    /// The real list, or the local one named by `MUMCORD_PUBLIC_SERVERS_URL` (tests and E2E,
    /// which then may contact only this machine).
    pub fn new(queries: Arc<ServerQueries>) -> Self {
        let (url, policy) = match std::env::var(URL_OVERRIDE_ENV) {
            Ok(url) if !url.is_empty() => (
                url,
                AddressPolicy::Only(Arc::new(vec![
                    IpAddr::V4(Ipv4Addr::LOCALHOST),
                    IpAddr::V6(Ipv6Addr::LOCALHOST),
                ])),
            ),
            _ => (LIST_URL.to_owned(), AddressPolicy::PublicOnly),
        };
        Self::with_source(url, policy, queries)
    }

    pub fn with_source(url: String, policy: AddressPolicy, queries: Arc<ServerQueries>) -> Self {
        Self {
            url,
            policy,
            pacer: PingPacer::new(PING_SPACING, PING_CONCURRENCY),
            queries,
        }
    }

    /// Downloads and parses the list. `agent` is the honest `User-Agent`, e.g. `Mumcord/0.5.0`.
    pub async fn list(&self, agent: &str) -> Result<Vec<PublicServer>, PublicListError> {
        let fetcher = Fetcher::new(self.policy.clone(), Timeouts::default())?;
        let response = fetcher
            .get_as(
                &self.url,
                agent,
                "application/xml,text/xml",
                BODY_LIMIT,
                false,
            )
            .await?;
        parse_list(&response.body, &self.policy)
    }

    /// Pings one listed server, paced with all other pings. `None` when it did not answer or
    /// its address must not be contacted.
    pub async fn ping(&self, host: &str, port: u16) -> Option<ServerQuery> {
        // A host name from the list could point into the user's own network; only addresses
        // the policy allows are asked.
        let addr = resolve(host, port).await?;
        if !self.policy.permits(addr.ip()) {
            tracing::debug!("a listed server resolves to an address that is not public");
            return None;
        }
        let _slot = self.pacer.slot().await;
        match self
            .queries
            .query(&addr.ip().to_string(), port, false)
            .await
        {
            QueryOutcome::Answered(answer) => Some(answer),
            QueryOutcome::Unreachable | QueryOutcome::NotAllowed => None,
        }
    }
}

async fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    match tokio::net::lookup_host((host, port)).await {
        Ok(mut targets) => targets.next(),
        Err(e) => {
            tracing::debug!(error = %e, "resolving a listed server failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version='1.0' standalone='yes'?>
<servers>
  <server name="Nordic &amp; Friends" ca="0" continent_code="EU" country="Sweden" country_code="se" ip="voice.example.org" port="64738" region="Stockholm" url="https://example.org/" />
  <server name="  Spaced   out&#x9;name " country="Germany" country_code="DE" ip="93.184.216.34" port="64739" />
  <server name="No port" country_code="FR" ip="noport.example.org" />
  <server name="Bad port" country_code="FR" ip="badport.example.org" port="70000" />
  <server name="LAN" country_code="DE" ip="192.168.1.10" port="64738" />
  <server name="Loop" country_code="DE" ip="localhost" port="64738" />
  <server name="Evil" country_code="XYZ" ip="evil host/../x" port="64738" />
  <server name="" country_code="US" ip="[fe80::1]" port="64738" />
  <server name="Dupe" country_code="SE" ip="VOICE.example.org" port="64738" />
</servers>"#;

    #[test]
    fn parses_and_cleans_the_list() {
        let servers = parse_list(FIXTURE.as_bytes(), &AddressPolicy::PublicOnly).expect("valid");
        let hosts: Vec<_> = servers.iter().map(|s| s.host.as_str()).collect();
        assert_eq!(hosts, ["voice.example.org", "93.184.216.34"]);
        assert_eq!(servers[0].name, "Nordic & Friends");
        assert_eq!(servers[0].country_code, "SE");
        assert_eq!(servers[0].country, "Sweden");
        assert_eq!(servers[1].name, "Spaced out name");
        assert_eq!(servers[1].port, 64739);
    }

    #[test]
    fn rejects_a_document_that_is_not_xml() {
        assert!(matches!(
            parse_list(
                b"<servers><server name=\"x\" ip=\"a.b\" port=\"1\"></nope>",
                &AddressPolicy::PublicOnly
            ),
            Err(PublicListError::Malformed(_))
        ));
    }

    #[test]
    fn strips_invisible_characters_and_cuts_long_names() {
        assert_eq!(clean_text("a\u{202E}b\u{200B}c\u{0007}d", 80), "abc d");
        let long = "x".repeat(200);
        let cut = clean_text(&long, NAME_LIMIT);
        assert_eq!(cut.chars().count(), NAME_LIMIT + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn a_nameless_server_is_named_after_its_host() {
        let servers = parse_list(
            br#"<servers><server name=" " ip="a.example.org" port="1"/></servers>"#,
            &AddressPolicy::PublicOnly,
        )
        .expect("valid");
        assert_eq!(servers[0].name, "a.example.org");
    }

    #[test]
    fn keeps_only_usable_hosts_and_codes() {
        let public = AddressPolicy::PublicOnly;
        assert_eq!(clean_host("8.8.8.8", &public).as_deref(), Some("8.8.8.8"));
        assert_eq!(clean_host("10.0.0.1", &public), None);
        assert_eq!(clean_host("-bad.example", &public), None);
        assert_eq!(clean_host("a..b", &public), None);
        // A local fixture list (tests, E2E) may name only the addresses its policy allows.
        let local = AddressPolicy::Only(Arc::new(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]));
        assert_eq!(
            clean_host("127.0.0.1", &local).as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(clean_host("8.8.8.8", &local), None);
        assert_eq!(clean_country_code("de"), "DE");
        assert_eq!(clean_country_code("D1"), "");
    }

    #[tokio::test]
    async fn pings_are_spaced_out() {
        let pacer = PingPacer::new(PING_SPACING, 100);
        let start = Instant::now();
        let mut starts = Vec::new();
        for _ in 0..5 {
            let _slot = pacer.slot().await;
            starts.push(start.elapsed());
        }
        assert!(starts[0] < PING_SPACING, "the first ping waits for nothing");
        // Each ping gets its own slot: the n-th starts no earlier than n spacings in. Waking
        // up late moves a ping later, never earlier.
        for (n, at) in starts.iter().enumerate() {
            assert!(*at >= PING_SPACING * n as u32, "{starts:?}");
        }
    }

    #[tokio::test]
    async fn concurrent_pings_are_spaced_and_capped() {
        let pacer = Arc::new(PingPacer::new(PING_SPACING, 2));
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let start = Instant::now();
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let (pacer, active, peak) = (pacer.clone(), active.clone(), peak.clone());
                tokio::spawn(async move {
                    let _slot = pacer.slot().await;
                    let at = start.elapsed();
                    let now = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    peak.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                    // A ping waiting for its answer.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    at
                })
            })
            .collect();
        let mut starts = Vec::new();
        for task in tasks {
            starts.push(task.await.expect("task"));
        }
        starts.sort();
        assert!(peak.load(std::sync::atomic::Ordering::SeqCst) <= 2);
        // Each ping gets its own slot: the n-th starts no earlier than n spacings in. Waking
        // up late moves a ping later, never earlier.
        for (n, at) in starts.iter().enumerate() {
            assert!(*at >= PING_SPACING * n as u32, "{starts:?}");
        }
    }

    /// Serves one HTTP response on localhost and returns what the client sent.
    async fn serve(body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let url = format!("http://{}/v1/list", listener.local_addr().expect("addr"));
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.expect("read");
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.expect("write");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (url, task)
    }

    #[tokio::test]
    async fn the_list_request_names_mumcord_and_accepts_gzip() {
        let (url, seen) = serve(FIXTURE).await;
        let browser = PublicServers::with_source(
            url,
            AddressPolicy::Only(Arc::new(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])),
            Arc::new(ServerQueries::default()),
        );
        let servers = browser.list("Mumcord/9.9.9").await.expect("list");
        // The local policy drops the public IP literal; host names are kept.
        assert_eq!(servers.len(), 1);
        let request = seen.await.expect("server").to_ascii_lowercase();
        assert!(request.contains("user-agent: mumcord/9.9.9"), "{request}");
        // The list server answers 501 to a request that does not accept gzip.
        assert!(request.contains("accept-encoding: gzip"), "{request}");
        assert!(!request.contains("cookie") && !request.contains("referer"));
    }

    #[tokio::test]
    async fn the_real_list_is_never_fetched_from_a_private_address() {
        let (url, _) = serve(FIXTURE).await;
        let browser = PublicServers::with_source(
            url,
            AddressPolicy::PublicOnly,
            Arc::new(ServerQueries::default()),
        );
        assert!(matches!(
            browser.list("Mumcord/9.9.9").await,
            Err(PublicListError::Fetch(FetchError::Blocked))
        ));
    }

    #[tokio::test]
    async fn a_listed_private_address_is_never_pinged() {
        let browser = PublicServers::with_source(
            String::new(),
            AddressPolicy::PublicOnly,
            Arc::new(ServerQueries::default()),
        );
        assert_eq!(browser.ping("127.0.0.1", 64738).await, None);
    }
}
