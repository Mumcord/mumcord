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

/// A GIF of `frames` frames (two colours alternating).
fn gif(width: u32, height: u32, frames: usize) -> Vec<u8> {
    use image::codecs::gif::GifEncoder;
    use image::{Delay, Frame, Rgba, RgbaImage};
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut out);
        let frames = (0..frames).map(|i| {
            let colour = if i % 2 == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            Frame::from_parts(
                RgbaImage::from_pixel(width, height, Rgba(colour)),
                0,
                0,
                Delay::from_numer_denom_ms(100, 1),
            )
        });
        encoder.encode_frames(frames).expect("encode gif");
    }
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
        "/anim.gif" => response(
            "200 OK",
            &[("Content-Type", "image/gif".into())],
            &gif(64, 32, 3),
        ),
        "/wide-anim.gif" => response(
            "200 OK",
            &[("Content-Type", "image/gif".into())],
            &gif(inline::ANIMATED_MAX_SIDE + 1, 1, 2),
        ),
        "/still.gif" => response(
            "200 OK",
            &[("Content-Type", "image/gif".into())],
            &gif(1600, 900, 1),
        ),
        // The server's content type is ignored: the bytes decide.
        "/claims-text" => response(
            "200 OK",
            &[("Content-Type", "text/plain".into())],
            &png(20, 10),
        ),
        "/fake.png" => response(
            "200 OK",
            &[("Content-Type", "image/png".into())],
            b"<html><script>alert(1)</script></html>",
        ),
        "/wide.png" => response(
            "200 OK",
            &[("Content-Type", "image/png".into())],
            &png(DECODE_MAX_SIDE + 1, 1),
        ),
        // Above the preview image limit but within the inline limit (trailing bytes are ignored).
        "/big.png" => {
            let mut body = png(40, 30);
            body.resize(fetch::IMAGE_LIMIT * 2, 0);
            response("200 OK", &[("Content-Type", "image/png".into())], &body)
        }
        "/too-big.png" => {
            let mut body = png(40, 30);
            body.resize(inline::INLINE_IMAGE_LIMIT + 1, 0);
            response("200 OK", &[("Content-Type", "image/png".into())], &body)
        }
        "/to-private-image" => redirect("http://192.168.1.10/anim.gif".into()),
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

fn json_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect()
}

/// Pretends the cache file was written `days` days ago.
fn age_file(path: &std::path::Path, days: u64) {
    let old = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60 + 60);
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open")
        .set_modified(old)
        .expect("set mtime");
}

async fn cached_page(server: &TestServer, dir: &std::path::Path) -> LinkPreviews {
    let previews = LinkPreviews::new(
        Some(dir.to_path_buf()),
        only_loopback(),
        Timeouts::default(),
    );
    assert!(previews.preview(&server.url("/page")).await.is_some());
    assert_eq!(json_files(dir).len(), 1);
    previews
}

#[tokio::test]
async fn expired_previews_are_pruned_by_max_age() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().expect("dir");
    let previews = cached_page(&server, dir.path()).await;
    let file = json_files(dir.path()).remove(0);
    let size = std::fs::metadata(&file).expect("meta").len();

    age_file(&file, 6);
    assert_eq!(
        previews.prune_expired(),
        0,
        "6 days is within the default 7"
    );
    assert_eq!(json_files(dir.path()).len(), 1);

    age_file(&file, 8);
    assert_eq!(previews.prune_expired(), size, "reports the freed bytes");
    assert!(json_files(dir.path()).is_empty());
    let hits = server.hits();
    let restarted = LinkPreviews::new(
        Some(dir.path().to_path_buf()),
        only_loopback(),
        Timeouts::default(),
    );
    assert!(restarted.preview(&server.url("/page")).await.is_some());
    assert!(server.hits() > hits, "an expired preview is fetched again");
}

#[tokio::test]
async fn max_age_is_configurable_and_never_keeps_everything() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().expect("dir");
    let previews = cached_page(&server, dir.path()).await;
    let file = json_files(dir.path()).remove(0);

    age_file(&file, 40);
    previews.set_max_age_days(None);
    assert_eq!(previews.prune_expired(), 0, "never deletes nothing");
    assert_eq!(json_files(dir.path()).len(), 1);

    age_file(&file, 2);
    previews.set_max_age_days(Some(1));
    assert!(previews.prune_expired() > 0, "1 day drops a 2 day old file");
    assert!(json_files(dir.path()).is_empty());
}

#[tokio::test]
async fn clear_now_removes_everything_and_reports_the_size() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().expect("dir");
    let previews = cached_page(&server, dir.path()).await;
    let size = std::fs::metadata(json_files(dir.path()).remove(0))
        .expect("meta")
        .len();
    assert!(size > 0);
    std::fs::write(dir.path().join("keep.txt"), b"not a cache file").expect("write");

    assert_eq!(previews.clear_cache(), size);
    assert!(json_files(dir.path()).is_empty());
    assert!(dir.path().join("keep.txt").exists());
    assert_eq!(previews.clear_cache(), 0, "nothing left to free");
}

#[tokio::test]
async fn image_links_are_flagged_instead_of_carded() {
    let server = TestServer::start().await;
    let previews = LinkPreviews::new(None, only_loopback(), Timeouts::default());
    let preview = previews
        .preview(&server.url("/cover.png"))
        .await
        .expect("an image link yields a flagged preview");
    assert!(preview.image_link);
    assert_eq!(preview.title, None);
    let page = previews.preview(&server.url("/page")).await.expect("page");
    assert!(!page.image_link);
}

#[tokio::test]
async fn still_images_are_reencoded_at_display_size() {
    let server = TestServer::start().await;
    let previews = LinkPreviews::new(None, only_loopback(), Timeouts::default());
    let image = previews
        .inline_image(&server.url("/cover.png#frag"))
        .await
        .expect("image");
    assert_eq!(image.url, server.url("/cover.png"));
    assert!(!image.animated);
    assert_eq!((image.width, image.height), (inline::STILL_MAX_SIDE, 400));
    assert!(image.data_url.starts_with("data:image/jpeg;base64,"));

    // A single-frame GIF is a still image as well.
    let still = previews
        .inline_image(&server.url("/still.gif"))
        .await
        .expect("still gif");
    assert!(!still.animated);
    assert_eq!((still.width, still.height), (800, 450));
    assert!(!still.data_url.starts_with("data:image/gif"), "re-encoded");
}

#[tokio::test]
async fn animated_gifs_pass_through_unchanged() {
    let server = TestServer::start().await;
    let previews = LinkPreviews::new(None, only_loopback(), Timeouts::default());
    let image = previews
        .inline_image(&server.url("/anim.gif"))
        .await
        .expect("animated gif");
    assert!(image.animated);
    assert_eq!((image.width, image.height), (64, 32));
    let expected = format!(
        "data:image/gif;base64,{}",
        crate::people::base64(&gif(64, 32, 3))
    );
    assert_eq!(
        image.data_url, expected,
        "the original bytes, every frame kept"
    );

    assert_eq!(
        previews.inline_image(&server.url("/wide-anim.gif")).await,
        None,
        "oversized animations are refused"
    );
}

#[tokio::test]
async fn inline_images_are_sniffed_from_the_bytes() {
    let server = TestServer::start().await;
    let previews = LinkPreviews::new(None, only_loopback(), Timeouts::default());
    let image = previews
        .inline_image(&server.url("/claims-text"))
        .await
        .expect("png bytes count, whatever the server claims");
    assert_eq!((image.width, image.height), (20, 10));
    assert_eq!(previews.inline_image(&server.url("/fake.png")).await, None);
    assert_eq!(previews.inline_image(&server.url("/page")).await, None);
}

#[tokio::test]
async fn inline_images_have_size_and_dimension_caps() {
    let server = TestServer::start().await;
    let previews = LinkPreviews::new(None, only_loopback(), Timeouts::default());
    assert!(
        previews
            .inline_image(&server.url("/big.png"))
            .await
            .is_some(),
        "inline images may exceed the preview image limit"
    );
    assert_eq!(
        previews.inline_image(&server.url("/too-big.png")).await,
        None
    );
    assert_eq!(previews.inline_image(&server.url("/wide.png")).await, None);
    let fetched = FetchedImage {
        kind: fetch::ImageKind::Png,
        bytes: png(DECODE_MAX_SIDE + 1, 1),
    };
    assert_eq!(
        inline::prepare("https://e.org/x.png", &fetched).err(),
        Some(FetchError::TooLarge)
    );
}

#[tokio::test]
async fn inline_images_never_come_from_local_addresses() {
    let server = TestServer::start().await;
    let public = LinkPreviews::new(None, AddressPolicy::PublicOnly, Timeouts::default());
    assert_eq!(public.inline_image(&server.url("/anim.gif")).await, None);
    assert_eq!(public.inline_image("file:///etc/passwd").await, None);
    assert_eq!(server.hits(), 0);

    let previews = LinkPreviews::new(None, only_loopback(), Timeouts::default());
    assert_eq!(
        previews
            .inline_image(&server.url("/to-private-image"))
            .await,
        None
    );
    assert_eq!(
        server.hits(),
        1,
        "the redirect to a LAN address is not followed"
    );
}

#[tokio::test]
async fn inline_images_share_the_preview_cache_clean_up() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().expect("dir");
    let previews = LinkPreviews::new(
        Some(dir.path().to_path_buf()),
        only_loopback(),
        Timeouts::default(),
    );
    assert!(
        previews
            .inline_image(&server.url("/anim.gif"))
            .await
            .is_some()
    );
    let images = dir.path().join("images");
    let file = json_files(&images).remove(0);
    let hits = server.hits();
    let restarted = LinkPreviews::new(
        Some(dir.path().to_path_buf()),
        only_loopback(),
        Timeouts::default(),
    );
    assert!(
        restarted
            .inline_image(&server.url("/anim.gif"))
            .await
            .is_some()
    );
    assert_eq!(server.hits(), hits, "served from the disk cache");

    age_file(&file, 8);
    assert!(
        restarted.prune_expired() > 0,
        "pruned with the previews' maximum age"
    );
    assert!(json_files(&images).is_empty());

    let fresh = LinkPreviews::new(
        Some(dir.path().to_path_buf()),
        only_loopback(),
        Timeouts::default(),
    );
    assert!(fresh.inline_image(&server.url("/anim.gif")).await.is_some());
    assert!(fresh.clear_cache() > 0, "Clear now removes images too");
    assert!(json_files(&images).is_empty());
}
