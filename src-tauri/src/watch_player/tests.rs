use super::*;

/// Sends one raw request to the server and returns the whole response.
async fn get(addr: SocketAddr, target: &str, host: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let request = format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\n\r\n");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read response");
    response
}

/// The server's address and the path of a page URL.
fn parts(url: &str) -> (SocketAddr, String) {
    let rest = url
        .strip_prefix("http://localhost:")
        .expect("localhost URL");
    let (port, path) = rest.split_once('/').expect("path");
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port.parse().expect("port")));
    (addr, format!("/{path}"))
}

#[tokio::test]
async fn serves_the_page_only_for_token_path_and_host() {
    let server = PlayerServer::default();
    server.party_changed(1, true);
    let url = server.party_url().expect("start");
    assert_eq!(
        server.party_url().expect("running"),
        url,
        "one server per app"
    );
    let (addr, path) = parts(&url);
    assert!(path.ends_with("/player.html"));
    let host = format!("localhost:{}", addr.port());
    let token = path.split('/').nth(1).expect("token");
    assert_eq!(token.len(), 32, "128-bit token");

    let page = get(addr, &path, &host).await;
    assert!(page.starts_with("HTTP/1.1 200 OK\r\n"), "{page}");
    assert!(
        !page.contains("frame-ancestors"),
        "WebKitGTK would block the tauri:// app"
    );
    assert!(page.contains("frame-src https://www.youtube-nocookie.com;"));
    assert!(page.contains("const APP_ORIGINS = [\"tauri://localhost\""));
    assert!(!page.contains("__NONCE__"));
    let by_ip = get(addr, &path, &addr.to_string()).await;
    assert!(by_ip.starts_with("HTTP/1.1 200 OK\r\n"), "{by_ip}");
    let by_ipv6 = get(addr, &path, &format!("[::1]:{}", addr.port())).await;
    assert!(by_ipv6.starts_with("HTTP/1.1 200 OK\r\n"), "{by_ipv6}");

    let wrong_token = format!("/{}/player.html", "0".repeat(32));
    for (target, host) in [
        (wrong_token.as_str(), host.as_str()),
        (&format!("/{token}/other.html"), host.as_str()),
        (&format!("/{token}/player.html?x=1"), host.as_str()),
        ("/", host.as_str()),
        (path.as_str(), "evil.example:80"),
        (path.as_str(), &format!("localhost:{}", addr.port() + 1)),
        (path.as_str(), &format!("[::1]:{}", addr.port() + 1)),
        (path.as_str(), &format!("[::2]:{}", addr.port())),
    ] {
        let answer = get(addr, target, host).await;
        assert!(
            answer.starts_with("HTTP/1.1 404 Not Found\r\n"),
            "{target} {host}: {answer}"
        );
    }
}

/// `localhost` may resolve to ::1 first; the web view must reach the page there too.
#[tokio::test]
async fn serves_the_page_on_ipv6_loopback_at_the_same_port() {
    if std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_err() {
        eprintln!("skipped: no IPv6 loopback on this machine");
        return;
    }
    let server = PlayerServer::default();
    let (v4, path) = parts(&server.party_url().expect("start"));
    let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, v4.port()));
    for host in [
        format!("localhost:{}", v4.port()),
        format!("[::1]:{}", v4.port()),
    ] {
        let page = get(v6, &path, &host).await;
        assert!(page.starts_with("HTTP/1.1 200 OK\r\n"), "{host}: {page}");
    }
}

#[tokio::test]
async fn refuses_other_methods_and_duplicate_hosts() {
    let server = PlayerServer::default();
    let (addr, path) = parts(&server.party_url().expect("start"));
    let host = format!("localhost:{}", addr.port());
    for request in [
        format!("POST {path} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nHost: evil.example\r\n\r\n"),
        format!("GET {path} HTTP/1.1\r\n\r\n"),
    ] {
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).await.expect("read");
        assert!(
            response.starts_with("HTTP/1.1 404"),
            "{request}: {response}"
        );
    }
}

#[tokio::test]
async fn stops_when_the_last_party_ends() {
    let server = PlayerServer::default();
    server.party_changed(1, true);
    server.party_changed(2, true);
    let url = server.party_url().expect("start");
    let (addr, _) = parts(&url);

    server.party_changed(1, false);
    assert!(TcpStream::connect(addr).await.is_ok(), "a party is left");

    server.party_changed(2, false);
    assert!(server.lock().running.is_none());
    let mut closed = false;
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_err() {
            closed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(closed, "port closed");

    assert_ne!(
        server.party_url().expect("restart"),
        url,
        "fresh token after a restart"
    );
}

#[tokio::test]
async fn serves_the_embed_page_while_a_chat_embed_plays() {
    let server = PlayerServer::default();
    let first = server.open_embed().expect("start");
    let second = server.open_embed().expect("running");
    assert_eq!(first, second);
    let (addr, path) = parts(&first);
    assert!(path.ends_with("/embed.html"), "{path}");
    let page = get(addr, &path, &format!("localhost:{}", addr.port())).await;
    assert!(page.starts_with("HTTP/1.1 200 OK\r\n"), "{page}");
    assert!(page.contains(
        "frame-src https://www.youtube-nocookie.com https://player.twitch.tv https://clips.twitch.tv;"
    ));

    // A party ending does not stop the embeds.
    server.party_changed(1, false);
    server.close_embed();
    assert!(server.lock().running.is_some(), "one embed still plays");
    server.close_embed();
    assert!(
        server.lock().running.is_none(),
        "stopped with the last embed"
    );
    server.close_embed();
    assert_eq!(server.lock().embeds, 0);
}

/// The app's CSP must let the window frame the origin the server hands out; a mismatch only shows
/// in a built app as a player that never answers.
#[test]
fn app_csp_allows_the_page_origin() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../../tauri.conf.json")).expect("tauri.conf.json");
    let csp = conf["app"]["security"]["csp"].as_str().expect("csp");
    let frame_src = csp
        .split(';')
        .map(str::trim)
        .find_map(|d| d.strip_prefix("frame-src "))
        .expect("frame-src");
    assert!(
        frame_src.split(' ').any(|s| s == "http://localhost:*"),
        "{frame_src}"
    );
    // The page answers only the app origins it was given; the Linux and macOS window runs at
    // `tauri://localhost`, the Windows one at `http://tauri.localhost`.
    let page = page_response(Page::Party);
    assert!(page.contains("const APP_ORIGINS = [\"tauri://localhost\",\"http://tauri.localhost\""));
    assert!(!page.contains("frame-ancestors"), "{page}");
}

#[test]
fn party_player_hides_youtubes_own_controls() {
    // Only the app's controls drive the party (issue #163); YouTube's would bypass the sync.
    for param in [
        r#"["controls", "0"]"#,
        r#"["disablekb", "1"]"#,
        r#"["fs", "0"]"#,
        r#"["modestbranding", "1"]"#,
        r#"["iv_load_policy", "3"]"#,
    ] {
        assert!(PAGE.contains(param), "player page lacks {param}");
    }
}
