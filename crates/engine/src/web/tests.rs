//! Fetching against a local HTTP server: previews, caching, SSRF guard and limits.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use image::{ImageFormat, Rgb, RgbImage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn png(width: u32, height: u32) -> Vec<u8> {
    let img = RgbImage::from_pixel(width, height, Rgb([200, 40, 40]));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .expect("encode");
    out
}

fn response(status: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if !headers.iter().any(|(k, _)| *k == "Content-Length") {
        out.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// A tiny HTTP/1.1 server with fixed routes; counts the requests it answered.
struct TestServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind((LOOPBACK, 0)).await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let counter = counter.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buf = [0u8; 4096];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => request.extend_from_slice(&buf[..n]),
                        }
                    }
                    counter.fetch_add(1, Ordering::SeqCst);
                    let text = String::from_utf8_lossy(&request);
                    let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    if path == "/stall" {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html>";
                        let _ = stream.write_all(head.as_bytes()).await;
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        return;
                    }
                    let _ = stream.write_all(&route(&path, addr)).await;
                });
            }
        });
        Self { addr, hits }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

fn route(path: &str, addr: SocketAddr) -> Vec<u8> {
    let html = |body: &str| {
        response(
            "200 OK",
            &[("Content-Type", "text/html; charset=utf-8".into())],
            body.as_bytes(),
        )
    };
    let redirect = |to: String| response("302 Found", &[("Location", to)], b"");
    let port = addr.port();
    match path {
        "/page" => html(
            r#"<html><head><title>Fallback</title>
            <meta property="og:title" content="Local &lt;test&gt; page">
            <meta property="og:description" content="Served by the test server">
            <meta property="og:site_name" content="Test Site">
            <meta property="og:image" content="/cover.png"></head><body>x</body></html>"#,
        ),
        "/notitle" => html("<html><head></head><body>no metadata</body></html>"),
        "/cover.png" => response(
            "200 OK",
            &[("Content-Type", "image/png".into())],
            &png(1200, 600),
        ),
        "/huge.png" => response(
            "200 OK",
            &[("Content-Type", "image/png".into())],
            &vec![0u8; fetch::IMAGE_LIMIT + 1],
        ),
        "/huge-unannounced.png" => {
            let mut bytes =
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: image/png\r\n\r\n".to_vec();
            bytes.extend(std::iter::repeat_n(0u8, fetch::IMAGE_LIMIT + 1));
            bytes
        }
        "/long-page" => {
            let mut body = String::from("<html><head><title>Long</title></head><body>");
            body.push_str(&"a".repeat(fetch::PAGE_LIMIT * 2));
            html(&body)
        }
        "/text" => response("200 OK", &[("Content-Type", "text/plain".into())], b"hello"),
        "/to-loopback-2" => redirect(format!("http://127.0.0.2:{port}/page")),
        "/to-private" => redirect("http://10.1.2.3/page".into()),
        "/to-ipv6-loopback" => redirect(format!("http://[::1]:{port}/page")),
        "/to-file" => redirect("file:///etc/passwd".into()),
        "/to-page" => redirect("/page".into()),
        "/loop" => redirect("/loop".into()),
        "/r3" => redirect("/r2".into()),
        "/r2" => redirect("/r1".into()),
        "/r1" => redirect("/page".into()),
        _ => response("404 Not Found", &[], b""),
    }
}

fn only_loopback() -> AddressPolicy {
    AddressPolicy::Only(Arc::new(vec![LOOPBACK]))
}

fn fetcher(policy: AddressPolicy) -> Fetcher {
    Fetcher::new(
        policy,
        Timeouts {
            connect: Duration::from_secs(2),
            total: Duration::from_millis(1500),
        },
    )
    .expect("fetcher")
}

const HTML: &str = "text/html";

#[tokio::test]
async fn builds_a_preview_and_caches_it() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().expect("dir");
    let previews = LinkPreviews::new(
        Some(dir.path().to_path_buf()),
        only_loopback(),
        Timeouts::default(),
    );
    let url = server.url("/page#section");
    let preview = previews.preview(&url).await.expect("preview");
    assert_eq!(preview.url, server.url("/page"));
    assert_eq!(preview.title.as_deref(), Some("Local <test> page"));
    assert_eq!(
        preview.description.as_deref(),
        Some("Served by the test server")
    );
    assert_eq!(preview.site_name.as_deref(), Some("Test Site"));
    assert_eq!(preview.video, None);
    let image = preview.image.clone().expect("image");
    let encoded = image
        .strip_prefix("data:image/jpeg;base64,")
        .expect("re-encoded as JPEG");
    assert!(encoded.len() < 60_000, "scaled down");
    assert_eq!(server.hits(), 2, "page and image");

    assert_eq!(previews.preview(&url).await, Some(preview.clone()));
    assert_eq!(server.hits(), 2, "memory cache");

    let restarted = LinkPreviews::new(
        Some(dir.path().to_path_buf()),
        only_loopback(),
        Timeouts::default(),
    );
    assert_eq!(restarted.preview(&url).await, Some(preview));
    assert_eq!(server.hits(), 2, "disk cache");
}

#[tokio::test]
async fn no_preview_without_title_or_for_non_html() {
    let server = TestServer::start().await;
    let previews = LinkPreviews::new(None, only_loopback(), Timeouts::default());
    assert_eq!(previews.preview(&server.url("/notitle")).await, None);
    assert_eq!(previews.preview(&server.url("/text")).await, None);
    assert_eq!(previews.preview("mailto:someone@example.com").await, None);
    let hits = server.hits();
    assert_eq!(previews.preview(&server.url("/notitle")).await, None);
    assert_eq!(server.hits(), hits, "failures are cached too");
}

#[tokio::test]
async fn public_only_never_contacts_local_addresses() {
    let server = TestServer::start().await;
    let fetcher = fetcher(AddressPolicy::PublicOnly);
    let port = server.addr.port();
    for url in [
        server.url("/page"),
        format!("http://localhost:{port}/page"),
        format!("http://[::1]:{port}/page"),
        format!("http://0.0.0.0:{port}/page"),
    ] {
        assert_eq!(
            fetcher.get(&url, HTML, 1024, true).await.err(),
            Some(FetchError::Blocked),
            "{url}"
        );
    }
    assert_eq!(server.hits(), 0);
    let previews = LinkPreviews::new(None, AddressPolicy::PublicOnly, Timeouts::default());
    assert_eq!(previews.preview(&server.url("/page")).await, None);
    assert_eq!(server.hits(), 0);
}

#[tokio::test]
async fn redirects_to_private_addresses_are_blocked() {
    let server = TestServer::start().await;
    let fetcher = fetcher(only_loopback());
    for path in ["/to-loopback-2", "/to-private", "/to-ipv6-loopback"] {
        assert_eq!(
            fetcher.get(&server.url(path), HTML, 1024, true).await.err(),
            Some(FetchError::Blocked),
            "{path}"
        );
    }
    assert!(
        fetcher
            .get(&server.url("/to-file"), HTML, 1024, true)
            .await
            .is_err()
    );
    assert_eq!(
        server.hits(),
        4,
        "only the redirecting requests reached the server"
    );
}

#[tokio::test]
async fn follows_at_most_three_redirects() {
    let server = TestServer::start().await;
    let fetcher = fetcher(only_loopback());
    let page = fetcher
        .page(&server.url("/r3"))
        .await
        .expect("three redirects");
    assert_eq!(page.url.as_str(), server.url("/page"));
    assert_eq!(
        fetcher.page(&server.url("/loop")).await.err(),
        Some(FetchError::TooManyRedirects)
    );
}

#[tokio::test]
async fn enforces_size_and_time_limits() {
    let server = TestServer::start().await;
    let fetcher = fetcher(only_loopback());
    assert_eq!(
        fetcher.image(&server.url("/huge.png")).await.err(),
        Some(FetchError::TooLarge)
    );
    assert_eq!(
        fetcher
            .image(&server.url("/huge-unannounced.png"))
            .await
            .err(),
        Some(FetchError::TooLarge)
    );
    let long = fetcher
        .page(&server.url("/long-page"))
        .await
        .expect("truncated page");
    assert_eq!(long.body.len(), fetch::PAGE_LIMIT);
    assert_eq!(
        fetcher.page(&server.url("/text")).await.err(),
        Some(FetchError::UnsupportedType)
    );
    let started = std::time::Instant::now();
    assert_eq!(
        fetcher.page(&server.url("/stall")).await.err(),
        Some(FetchError::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn thumbnails_are_scaled_and_reencoded() {
    let image = FetchedImage {
        kind: fetch::ImageKind::Png,
        bytes: png(1000, 500),
    };
    let url = thumbnail_data_url(&image, 480).expect("thumbnail");
    let data = url.strip_prefix("data:image/jpeg;base64,").expect("jpeg");
    assert!(!data.is_empty());
    let broken = FetchedImage {
        kind: fetch::ImageKind::Png,
        bytes: b"\x89PNG\r\n\x1a\ngarbage".to_vec(),
    };
    assert_eq!(thumbnail_data_url(&broken, 480), None);
}
