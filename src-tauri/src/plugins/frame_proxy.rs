//! Windows only: the local proxy the app's WebView2 sends all its traffic through (S0 closing
//! measure, FR-9.2). Chromium opens connections a page never asked for in a way CSP can stop
//! (DNS prefetch, preconnect, `<object>`, `ping`, meta refresh, self navigation, downloads). With
//! every connection going through this proxy, only allowed hosts are ever contacted:
//!
//! - the websites approved plugins declare in `net.origins` (exact host and port, read on every
//!   connection, so approving or revoking an origin takes effect at once), and
//! - the hosts the chat's video and music embeds need, port 443 only ([`EMBED_PROVIDERS`]).
//!
//! Only `CONNECT` tunnels are relayed; plain HTTP is refused. Loopback (the app's own player
//! page and Developer-mode `http://localhost` origins) bypasses the proxy ([`browser_args`]).
//! The proxy only decides whether to connect; TLS stays end-to-end between the web view and the
//! website.

#[cfg(windows)]
use std::sync::Arc;

#[cfg(windows)]
use tauri::{AppHandle, Manager, Runtime};

#[cfg(windows)]
use super::Plugins;

/// The registrable domains a chat embed provider's player loads from, its player origin's
/// first (the origins the app frames: tauri.conf.json `frame-src` and the loopback player
/// page's `frame-src`).
pub struct EmbedProvider {
    pub domains: &'static [&'static str],
}

/// The single list of embed providers the proxy lets through. A test checks that every player
/// origin the app frames is covered here, so a new provider cannot silently break on Windows.
pub const EMBED_PROVIDERS: &[EmbedProvider] = &[
    EmbedProvider {
        domains: &[
            "youtube-nocookie.com",
            "youtube.com",
            "ytimg.com",
            "googlevideo.com",
            "ggpht.com",
            "gstatic.com",
            "google.com",
            "googleapis.com",
        ],
    },
    EmbedProvider {
        domains: &[
            "twitch.tv",
            "ttvnw.net",
            "jtvnw.net",
            "twitchcdn.net",
            "twitchsvc.net",
            "live-video.net",
        ],
    },
    EmbedProvider {
        domains: &["spotify.com", "scdn.co", "spotifycdn.com"],
    },
    EmbedProvider {
        domains: &["soundcloud.com", "sndcdn.com"],
    },
];

/// The port embed hosts may be reached on.
const HTTPS_PORT: u16 = 443;

/// Whether `host` (lower-case ASCII) is an embed provider's domain or a subdomain of one.
fn is_embed_host(host: &str) -> bool {
    EMBED_PROVIDERS
        .iter()
        .flat_map(|provider| provider.domains)
        .any(|domain| {
            host == *domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|rest| rest.ends_with('.'))
        })
}

/// The `host` and `port` of a `CONNECT host:port HTTP/1.x` request line, host lower-cased.
/// `None` for any other method or a malformed target (IP literals in brackets included).
fn connect_target(request_line: &str) -> Option<(String, u16)> {
    let mut parts = request_line.split_ascii_whitespace();
    if parts.next()? != "CONNECT" {
        return None;
    }
    let (host, port) = parts.next()?.rsplit_once(':')?;
    if !parts.next()?.starts_with("HTTP/1.") || parts.next().is_some() {
        return None;
    }
    let valid_host = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
    if !valid_host {
        return None;
    }
    Some((host.to_ascii_lowercase(), port.parse().ok()?))
}

/// Whether the proxy connects to `host:port`: an embed host on 443, or a plugin's approved
/// origin (`declared` answers for the exact host and port).
fn allowed(host: &str, port: u16, declared: impl Fn(&str, u16) -> bool) -> bool {
    (port == HTTPS_PORT && is_embed_host(host)) || declared(host, port)
}

/// The WebView2 arguments that route the web view's traffic through the proxy on `port`.
///
/// Chromium bypasses a proxy for loopback and link-local addresses on its own. `<-loopback>`
/// switches that built-in rule off (it does not add a bypass), so link-local hosts cannot be
/// reached past the proxy; the loopback hosts are then named again, because the plain-HTTP player
/// page on `http://localhost:<port>` would otherwise be sent to the proxy and refused. Later
/// rules win over earlier ones, so the loopback hosts have to follow `<-loopback>`.
fn browser_args(port: u16) -> String {
    format!(
        "--proxy-server=http://127.0.0.1:{port} \
         --proxy-bypass-list=<-loopback>;localhost;127.0.0.1;[::1]"
    )
}

/// Starts the proxy on a free loopback port and returns the WebView2 arguments that route the
/// app's traffic through it. `None` (logged) if no port could be bound; the app then runs
/// without this measure rather than not at all.
#[cfg(windows)]
pub fn start<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let listener = match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, "the web view proxy could not start");
            return None;
        }
    };
    let port = listener.local_addr().ok()?.port();
    if let Err(error) = listener.set_nonblocking(true) {
        tracing::error!(%error, "the web view proxy could not start");
        return None;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(%error, "the web view proxy could not start");
                return;
            }
        };
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let plugins = app.try_state::<Arc<Plugins>>().map(|s| Arc::clone(&s));
                    tauri::async_runtime::spawn(relay(stream, plugins));
                }
                Err(error) => tracing::warn!(%error, "the web view proxy missed a connection"),
            }
        }
    });
    Some(browser_args(port))
}

/// Answers one proxy connection: reads the request head, then either tunnels to an allowed
/// target or refuses with 403.
#[cfg(windows)]
async fn relay(mut client: tokio::net::TcpStream, plugins: Option<Arc<Plugins>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A CONNECT head is a few hundred bytes; anything longer is not one.
    const MAX_HEAD: usize = 4096;
    let mut head = Vec::with_capacity(512);
    let mut buf = [0u8; 512];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match client.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
        if head.len() > MAX_HEAD {
            return;
        }
    }
    let line = String::from_utf8_lossy(&head);
    let target = line.lines().next().and_then(connect_target);
    let declared = |host: &str, port: u16| {
        plugins
            .as_ref()
            .is_some_and(|p| p.webviews().allows_host(host, port))
    };
    let Some((host, port)) = target.filter(|(host, port)| allowed(host, *port, declared)) else {
        // Hosts are not logged: they may come from what the user browses in an embed.
        tracing::debug!("the web view proxy refused a connection");
        let _ = client
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    };
    let mut upstream = match tokio::net::TcpStream::connect((host.as_str(), port)).await {
        Ok(upstream) => upstream,
        Err(_) => {
            let _ = client
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            return;
        }
    };
    if client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .is_err()
    {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The watch party and embed player page is plain HTTP on `localhost`. Sent to the proxy it
    /// is refused, and the YouTube player stays black.
    #[test]
    fn loopback_bypasses_the_proxy() {
        let args = browser_args(4321);
        let rules = args
            .split_ascii_whitespace()
            .find_map(|arg| arg.strip_prefix("--proxy-bypass-list="))
            .map(|list| list.split(';').collect::<Vec<_>>());
        assert_eq!(
            rules,
            Some(vec!["<-loopback>", "localhost", "127.0.0.1", "[::1]"])
        );
        assert!(args.contains("--proxy-server=http://127.0.0.1:4321 "));
    }

    #[test]
    fn connect_lines_are_parsed_strictly() {
        assert_eq!(
            connect_target("CONNECT API.Example:8443 HTTP/1.1"),
            Some(("api.example".to_owned(), 8443))
        );
        for line in [
            "GET http://api.example/ HTTP/1.1",
            "CONNECT api.example HTTP/1.1",
            "CONNECT api.example:x HTTP/1.1",
            "CONNECT api.example:70000 HTTP/1.1",
            "CONNECT [::1]:443 HTTP/1.1",
            "CONNECT a/b:443 HTTP/1.1",
            "CONNECT :443 HTTP/1.1",
            "CONNECT api.example:443",
            "CONNECT api.example:443 HTTP/1.1 extra",
        ] {
            assert_eq!(connect_target(line), None, "{line}");
        }
    }

    #[test]
    fn embed_hosts_pass_on_443_only() {
        let none = |_: &str, _: u16| false;
        for host in [
            "www.youtube-nocookie.com",
            "i.ytimg.com",
            "rr1---sn-abc.googlevideo.com",
            "player.twitch.tv",
            "video-weaver.fra05.hls.ttvnw.net",
            "open.spotify.com",
            "encore.scdn.co",
            "w.soundcloud.com",
            "i1.sndcdn.com",
        ] {
            assert!(allowed(host, 443, none), "{host}");
            assert!(!allowed(host, 80, none), "{host}");
            assert!(!allowed(host, 8443, none), "{host}");
        }
        for host in [
            "evil.example",
            "notspotify.com",
            "spotify.com.evil.example",
            "scdn.co.evil.example",
            "com",
        ] {
            assert!(!allowed(host, 443, none), "{host}");
        }
    }

    #[test]
    fn declared_origins_pass_with_their_exact_port() {
        let declared = |host: &str, port: u16| host == "api.example" && port == 8443;
        assert!(allowed("api.example", 8443, declared));
        assert!(!allowed("api.example", 443, declared));
        assert!(!allowed("other.example", 8443, declared));
    }

    /// Every player origin the app frames must be covered by a listed provider; otherwise that
    /// embed would stop loading on Windows.
    #[test]
    fn every_framed_player_origin_is_a_listed_provider() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let csp = config["app"]["security"]["csp"].as_str().unwrap();
        let app_frames = csp
            .split(';')
            .find_map(|d| d.trim().strip_prefix("frame-src "))
            .unwrap();
        let player_frames = [
            crate::watch_player::Page::Party.frame_src(),
            crate::watch_player::Page::Embed.frame_src(),
        ];
        let framed: Vec<&str> = std::iter::once(app_frames)
            .chain(player_frames)
            .flat_map(str::split_ascii_whitespace)
            .filter(|source| source.starts_with("https://"))
            .collect();
        assert!(framed.len() >= 5, "{framed:?}");
        for source in framed {
            let origin = tauri::Url::parse(source).unwrap();
            assert!(
                is_embed_host(origin.host_str().unwrap()),
                "{source} is framed but not covered by EMBED_PROVIDERS"
            );
        }
    }
}
