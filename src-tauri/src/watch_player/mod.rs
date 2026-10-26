//! Loopback pages for the YouTube and Twitch players (FR-2.41a).
//!
//! YouTube's embedded player refuses to play inside the app's own `tauri://localhost` origin
//! (error 153: no http(s) referrer), and Twitch only accepts an https or `localhost` parent. The
//! app therefore frames one small static page from a loopback-only HTTP server, and that page
//! frames the player: `player.html` for the watch party's YouTube player, `embed.html` for a
//! video played inline from a chat card. The server binds 127.0.0.1 and ::1 on the same random
//! port, is addressed as `localhost` (the name Twitch accepts over plain http, which may resolve
//! to either address first), answers only under a
//! random 128-bit path token and a matching `Host` header, serves nothing but the two pages, and
//! runs only while the user is in a party or a chat embed plays. It logs no requests: the token
//! and the videos watched stay out of the logs.

use std::collections::HashSet;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Mutex;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const PAGE: &str = include_str!("player.html");
/// A browser's GET for the page fits easily; anything larger is not the app's frame.
const MAX_REQUEST: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Random IPv4 ports to try before giving up on finding the same port free on IPv6.
const BIND_ATTEMPTS: usize = 8;

/// Origins the app's own page runs at: Linux and macOS, Windows, and the dev server in debug
/// builds. Only these may talk to the page.
fn app_origins() -> &'static [&'static str] {
    if cfg!(debug_assertions) {
        &[
            "tauri://localhost",
            "http://tauri.localhost",
            "http://localhost:1420",
        ]
    } else {
        &["tauri://localhost", "http://tauri.localhost"]
    }
}

/// The player server, started on first use and stopped when no party with the user and no chat
/// embed is left.
#[derive(Default)]
pub struct PlayerServer {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    running: Option<Running>,
    /// Servers on which the user is in a watch party.
    parties: HashSet<u32>,
    /// Chat embeds playing through the server.
    embeds: usize,
}

impl State {
    /// The page's URL; starts the server if it is not running. Needs a Tokio runtime.
    fn url(&mut self, page: Page) -> io::Result<String> {
        let base = match &self.running {
            Some(running) => running.base.clone(),
            None => {
                let running = start()?;
                let base = running.base.clone();
                self.running = Some(running);
                base
            }
        };
        Ok(format!("{base}/{}", page.file()))
    }

    fn stop_if_unused(&mut self) {
        if self.parties.is_empty()
            && self.embeds == 0
            && let Some(running) = self.running.take()
        {
            // The listener is dropped as soon as the task sees the signal. A send error means
            // the task already ended; there is nothing left to stop.
            let _ = running.stop.send(());
        }
    }
}

struct Running {
    /// `http://localhost:<port>/<token>`.
    base: String,
    stop: oneshot::Sender<()>,
}

/// The two pages the server knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    /// The watch party's YouTube player, driven by play/pause/seek messages.
    Party,
    /// A YouTube or Twitch player opened from a chat card.
    Embed,
}

impl Page {
    fn file(self) -> &'static str {
        match self {
            Self::Party => "player.html",
            Self::Embed => "embed.html",
        }
    }

    /// The player origins the page may frame.
    fn frame_src(self) -> &'static str {
        match self {
            Self::Party => "https://www.youtube-nocookie.com",
            Self::Embed => {
                "https://www.youtube-nocookie.com https://player.twitch.tv https://clips.twitch.tv"
            }
        }
    }
}

impl PlayerServer {
    /// URL of the watch party's player page; starts the server if it is not running. Needs a
    /// Tokio runtime.
    pub fn party_url(&self) -> io::Result<String> {
        self.lock().url(Page::Party)
    }

    /// URL of the page for a chat embed that starts playing; each call needs a later
    /// [`close_embed`](Self::close_embed). Needs a Tokio runtime.
    pub fn open_embed(&self) -> io::Result<String> {
        let mut state = self.lock();
        let url = state.url(Page::Embed)?;
        state.embeds += 1;
        Ok(url)
    }

    /// A chat embed stopped; the port closes once nothing plays.
    pub fn close_embed(&self) {
        let mut state = self.lock();
        state.embeds = state.embeds.saturating_sub(1);
        state.stop_if_unused();
    }

    /// Follows the parties of one server; the port closes once nothing plays.
    pub fn party_changed(&self, server_id: u32, joined: bool) {
        let mut state = self.lock();
        if joined {
            state.parties.insert(server_id);
        } else {
            state.parties.remove(&server_id);
        }
        state.stop_if_unused();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // The state stays consistent at every point a panic could happen; keep serving.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn start() -> io::Result<Running> {
    let (v4, v6) = bind()?;
    let port = v4.local_addr()?.port();
    let v4 = async_listener(v4)?;
    let v6 = v6.map(async_listener).transpose()?;
    let token = format!("{:032x}", rand::random::<u128>());
    let base = format!("http://localhost:{port}/{token}");
    let (stop, stopped) = oneshot::channel();
    tokio::spawn(serve(v4, v6, Site { port, token }, stopped));
    Ok(Running { base, stop })
}

/// Binds 127.0.0.1 on a random port and ::1 on the same port, so `localhost` reaches the server
/// whichever address the web view resolves it to. Without IPv6, or if no shared free port turns
/// up, the server runs on IPv4 alone: `localhost` then resolves to 127.0.0.1 or falls back to it.
fn bind() -> io::Result<(std::net::TcpListener, Option<std::net::TcpListener>)> {
    let mut attempt = 1;
    loop {
        let v4 = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        match std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            Ok(v6) => return Ok((v4, Some(v6))),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse && attempt < BIND_ATTEMPTS => {
                attempt += 1;
            }
            Err(e) => {
                tracing::warn!(error = %e, "watch party player server: IPv6 loopback unavailable");
                return Ok((v4, None));
            }
        }
    }
}

fn async_listener(listener: std::net::TcpListener) -> io::Result<TcpListener> {
    listener.set_nonblocking(true)?;
    TcpListener::from_std(listener)
}

/// What a request must name to get the page.
struct Site {
    port: u16,
    token: String,
}

async fn serve(
    v4: TcpListener,
    v6: Option<TcpListener>,
    site: Site,
    mut stopped: oneshot::Receiver<()>,
) {
    let site = std::sync::Arc::new(site);
    loop {
        tokio::select! {
            _ = &mut stopped => return,
            accepted = accept(Some(&v4)) => handle(accepted, &site),
            accepted = accept(v6.as_ref()) => handle(accepted, &site),
        }
    }
}

/// Accepts on the listener, or waits forever without one.
async fn accept(listener: Option<&TcpListener>) -> io::Result<(TcpStream, SocketAddr)> {
    match listener {
        Some(listener) => listener.accept().await,
        None => std::future::pending().await,
    }
}

fn handle(accepted: io::Result<(TcpStream, SocketAddr)>, site: &std::sync::Arc<Site>) {
    match accepted {
        Ok((stream, _)) => {
            let site = site.clone();
            tokio::spawn(async move {
                if let Err(e) = answer(stream, &site).await {
                    tracing::debug!(error = %e, "watch party player request failed");
                }
            });
        }
        Err(e) => tracing::warn!(error = %e, "watch party player server: accept failed"),
    }
}

async fn answer(mut stream: TcpStream, site: &Site) -> io::Result<()> {
    let request = tokio::time::timeout(READ_TIMEOUT, read_head(&mut stream))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "request head timed out"))??;
    let response = if let Some(page) = page_requested(&request, site) {
        page_response(page)
    } else {
        response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            &[],
            "Not found",
        )
    };
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

/// Reads up to the end of the request head; the page takes no request body.
async fn read_head(stream: &mut TcpStream) -> io::Result<String> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(&buf[..n]);
        if head.len() > MAX_REQUEST {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request head too large",
            ));
        }
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// The page of a GET for exactly `/<token>/<page>` (no query) addressed to `localhost:<port>`,
/// `127.0.0.1:<port>` or `[::1]:<port>`; the `Host` check keeps DNS-rebinding pages on other names from reading it.
fn page_requested(head: &str, site: &Site) -> Option<Page> {
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let (Some("GET"), Some(target), Some(version)) = (
        request_line.next(),
        request_line.next(),
        request_line.next(),
    ) else {
        return None;
    };
    if !version.starts_with("HTTP/1.") || request_line.next().is_some() {
        return None;
    }
    let port = site.port;
    let allowed = [
        format!("localhost:{port}"),
        format!("127.0.0.1:{port}"),
        format!("[::1]:{port}"),
    ];
    let mut hosts = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim());
    let host_ok =
        hosts.next().is_some_and(|h| allowed.iter().any(|a| a == h)) && hosts.next().is_none();
    if !host_ok {
        return None;
    }
    let file = target.strip_prefix(&format!("/{}/", site.token))?;
    [Page::Party, Page::Embed]
        .into_iter()
        .find(|page| page.file() == file)
}

fn page_response(page: Page) -> String {
    let nonce = format!("{:032x}", rand::random::<u128>());
    let origins = app_origins();
    let origins_json = serde_json::to_string(origins).unwrap_or_else(|_| "[]".to_owned());
    let body = PAGE
        .replace("__NONCE__", &nonce)
        .replace("__APP_ORIGINS__", &origins_json);
    // No `frame-ancestors`: WebKitGTK does not match the app's custom `tauri://` scheme there,
    // so naming the app's origins would block the app itself, and X-Frame-Options cannot name
    // another origin. Other pages cannot frame it anyway without the secret token, and the page
    // only talks to an app origin. It runs only its own nonce'd script and may only frame the
    // players of its page.
    let csp = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; \
         frame-src {}; base-uri 'none'; form-action 'none'",
        page.frame_src()
    );
    response(
        "200 OK",
        "text/html; charset=utf-8",
        &[
            ("Content-Security-Policy", &csp),
            // The players need the page's origin as referrer; nothing more leaves the app.
            ("Referrer-Policy", "strict-origin-when-cross-origin"),
            ("X-Content-Type-Options", "nosniff"),
            ("Cache-Control", "no-store"),
        ],
        &body,
    )
}

fn response(status: &str, content_type: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    out
}

#[cfg(test)]
mod tests;
