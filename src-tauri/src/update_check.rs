//! Optional update hint (FR-2.56): asks GitHub for the latest release at most once a day, but
//! only after the user said yes. Nothing is downloaded or installed; the UI links to the
//! release page.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mumcord_engine::store::Consent;
use mumcord_engine::web::fetch::{AddressPolicy, FetchError, Fetcher, Timeouts};
use mumcord_engine::{Engine, EngineResult};
use tauri::AppHandle;
use tauri_specta::Event;

use crate::commands::updates::{UpdateAvailable, UpdateInfo};

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/ToBee94/mumcord/releases/latest";
const RELEASE_PAGE: &str = "https://github.com/ToBee94/mumcord/releases/tag/";
/// Tests and E2E point the check at a local server; the app never reads another address.
const URL_OVERRIDE_ENV: &str = "MUMCORD_UPDATE_URL";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// How often the background loop looks at the clock; the real throttle is [`is_due`].
const LOOP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// A release document is a few KB; anything much bigger is not one.
const BODY_LIMIT: usize = 256 * 1024;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Whether a day has passed since the last check. A clock that went back counts as due.
fn is_due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|last| now < last || now - last >= DAY_MS)
}

/// A parsed `major.minor.patch[-pre]` version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    core: [u64; 3],
    /// Pre-releases sort before their release; the flag is inverted so the derived order fits.
    is_final: bool,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, _)) => (core, true),
            None => (text, false),
        };
        let mut parts = core.split('.');
        let mut next = || parts.next()?.parse::<u64>().ok();
        let version = Self {
            core: [next()?, next()?, next()?],
            is_final: !pre,
        };
        parts.next().is_none().then_some(version)
    }

    fn is_prerelease(&self) -> bool {
        !self.is_final
    }
}

/// The update to offer for a GitHub "latest release" document, if it is newer than `current`.
/// The release page is built from the validated version, never taken from the response.
fn parse_release(body: &[u8], current: &str) -> Option<UpdateInfo> {
    let current = Version::parse(current)?;
    let release: serde_json::Value = serde_json::from_slice(body).ok()?;
    let tag = release.get("tag_name")?.as_str()?;
    let latest = Version::parse(tag)?;
    let flagged = release
        .get("prerelease")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if (flagged || latest.is_prerelease()) && !current.is_prerelease() {
        return None;
    }
    if latest <= current {
        return None;
    }
    let version = tag.trim().trim_start_matches('v').to_string();
    Some(UpdateInfo {
        url: format!("{RELEASE_PAGE}v{version}"),
        version,
    })
}

/// Where to ask and which addresses may be contacted.
fn target() -> (String, AddressPolicy) {
    match std::env::var(URL_OVERRIDE_ENV) {
        Ok(url) if !url.is_empty() => (
            url,
            AddressPolicy::Only(Arc::new(vec![
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
            ])),
        ),
        _ => (LATEST_RELEASE_URL.to_string(), AddressPolicy::PublicOnly),
    }
}

/// One request; the only thing sent besides the request itself is the `User-Agent`.
async fn fetch_latest(
    url: &str,
    policy: AddressPolicy,
    timeouts: Timeouts,
    current: &str,
) -> Result<Option<UpdateInfo>, FetchError> {
    let fetcher = Fetcher::new(policy, timeouts)?;
    let agent = format!("Mumcord/{current}");
    let response = fetcher
        .get_as(
            url,
            &agent,
            "application/vnd.github+json",
            BODY_LIMIT,
            false,
        )
        .await?;
    Ok(parse_release(&response.body, current))
}

/// Runs the check when the user consented and (unless `force`) a day passed. Failures are
/// logged only: an update hint must never bother the user with an error.
pub async fn check(engine: &Engine, force: bool) -> EngineResult<Option<UpdateInfo>> {
    let mut settings = engine.settings()?;
    if settings.update_check != Consent::Yes {
        return Ok(None);
    }
    let now = now_ms();
    if !force && !is_due(settings.last_update_check, now) {
        return Ok(None);
    }
    // Stamped before the request so a failing network is not retried every hour.
    settings.last_update_check = Some(now);
    engine.update_settings(settings).await?;

    let (url, policy) = target();
    match fetch_latest(&url, policy, Timeouts::default(), env!("CARGO_PKG_VERSION")).await {
        Ok(info) => Ok(info),
        Err(e) => {
            tracing::warn!(error = %e, "update check failed");
            Ok(None)
        }
    }
}

/// Checks once a day in the background and tells the UI about a newer release.
pub fn start(app: AppHandle, engine: Engine) {
    tauri::async_runtime::spawn(async move {
        loop {
            match check(&engine, false).await {
                Ok(Some(info)) => {
                    if let Err(e) = (UpdateAvailable { info }).emit(&app) {
                        tracing::warn!(error = %e, "telling the UI about an update failed");
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %e, "update check skipped"),
            }
            tokio::time::sleep(LOOP_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn release(tag: &str, pre: bool) -> Vec<u8> {
        format!(r#"{{"tag_name":"{tag}","prerelease":{pre},"html_url":"https://evil.example/x"}}"#)
            .into_bytes()
    }

    #[test]
    fn newer_release_is_offered_with_our_own_link() {
        let info = parse_release(&release("v0.4.1", false), "0.4.0").expect("update");
        assert_eq!(info.version, "0.4.1");
        assert_eq!(
            info.url,
            "https://github.com/ToBee94/mumcord/releases/tag/v0.4.1"
        );
    }

    #[test]
    fn same_older_and_garbage_are_ignored() {
        assert!(parse_release(&release("v0.4.0", false), "0.4.0").is_none());
        assert!(parse_release(&release("v0.3.9", false), "0.4.0").is_none());
        assert!(parse_release(&release("v0.10.0", false), "0.9.0").is_some());
        assert!(parse_release(b"not json", "0.4.0").is_none());
        assert!(parse_release(&release("nightly", false), "0.4.0").is_none());
        assert!(parse_release(&release("v1.2.3.4", false), "0.4.0").is_none());
    }

    #[test]
    fn prereleases_only_for_prerelease_users() {
        assert!(parse_release(&release("v0.5.0-beta.1", true), "0.4.0").is_none());
        assert!(parse_release(&release("v0.5.0-beta.1", true), "0.4.0-beta.1").is_some());
        assert!(parse_release(&release("v0.4.0", false), "0.4.0-beta.1").is_some());
    }

    #[test]
    fn throttle_is_once_a_day() {
        assert!(is_due(None, 1_000));
        assert!(!is_due(Some(1_000), 1_000 + DAY_MS - 1));
        assert!(is_due(Some(1_000), 1_000 + DAY_MS));
        assert!(is_due(Some(5_000), 1_000), "clock went back");
    }

    /// Serves one canned HTTP response; returns the URL and the request text it saw.
    async fn serve(response: String, hang: bool) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}/latest", listener.local_addr().expect("addr"));
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.expect("read");
            if hang {
                tokio::time::sleep(Duration::from_secs(5)).await;
            } else {
                socket.write_all(response.as_bytes()).await.expect("write");
            }
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (url, task)
    }

    fn local() -> AddressPolicy {
        AddressPolicy::Only(Arc::new(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]))
    }

    fn ok(body: &[u8]) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        )
    }

    #[tokio::test]
    async fn mock_server_update_is_found_and_only_user_agent_identifies() {
        let (url, seen) = serve(ok(&release("v9.0.0", false)), false).await;
        let info = fetch_latest(&url, local(), Timeouts::default(), "0.4.0")
            .await
            .expect("fetch")
            .expect("update");
        assert_eq!(info.version, "9.0.0");
        let request = seen.await.expect("server").to_ascii_lowercase();
        assert!(request.contains("user-agent: mumcord/0.4.0"));
        assert!(!request.contains("cookie") && !request.contains("authorization"));
    }

    #[tokio::test]
    async fn server_errors_and_timeouts_are_plain_errors() {
        let (url, _) = serve(
            "HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\n\r\n".into(),
            false,
        )
        .await;
        let err = fetch_latest(&url, local(), Timeouts::default(), "0.4.0").await;
        assert!(matches!(err, Err(FetchError::Status(403))));

        let (url, _) = serve(String::new(), true).await;
        let short = Timeouts {
            connect: Duration::from_millis(200),
            total: Duration::from_millis(300),
        };
        let err = fetch_latest(&url, local(), short, "0.4.0").await;
        assert!(matches!(err, Err(FetchError::Timeout)));
    }

    #[tokio::test]
    async fn public_only_policy_never_reaches_localhost() {
        let (url, _) = serve(ok(&release("v9.0.0", false)), false).await;
        let err = fetch_latest(
            &url,
            AddressPolicy::PublicOnly,
            Timeouts::default(),
            "0.4.0",
        )
        .await;
        assert!(matches!(err, Err(FetchError::Blocked)));
    }
}
