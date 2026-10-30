//! Saved servers and `mumble://` link parsing (FR-1.7, FR-1.8, FR-1.11).

use mumcord_engine::{Engine, PasswordChange, ServerFields, ServerView, TestConnectionView};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{
    ApiError, ApiResult, ErrorCode, ServerId, to_api_server_id, to_engine_server_id,
};

/// Default Mumble port.
const DEFAULT_PORT: u16 = 64738;

/// A saved server as shown in the server bar; the list order is the bar order.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedServer {
    pub id: ServerId,
    pub label: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    /// The password itself stays in the OS secure storage and is never sent to the UI.
    pub has_password: bool,
    /// "Save history" switch (FR-1.33); changed via `history_set_saving`.
    pub save_history: bool,
    /// Connect this server when the app starts (FR-2.7).
    pub always_connect_at_start: bool,
    /// This was the voice server when Mumcord last quit; only it offers "Rejoin" (FR-2.3).
    pub was_voice_server_at_quit: bool,
    /// No toasts or desktop notifications from this server; badges stay (FR-2.10).
    pub notifications_muted: bool,
    /// `@all` and `@channel` do not count as mentions on this server (issue #70).
    pub group_mentions_muted: bool,
    /// Theme shown with this server; `null` uses the app theme (FR-2.17).
    pub theme_id: Option<String>,
    /// Identity this server connects with; `null` uses the default identity (issue #148).
    pub identity_id: Option<u32>,
}

/// How a server edit treats the stored password.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PasswordUpdate {
    /// Leave the stored password as it is (on add: no password).
    Keep,
    Clear,
    Set {
        value: String,
    },
}

/// Fields for adding or editing a saved server.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerInput {
    pub label: String,
    pub address: String,
    /// Default 64738.
    pub port: u16,
    pub username: String,
    pub password: PasswordUpdate,
}

/// Values of the add/edit dialog for a test connection (issue #151).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerTestInput {
    pub address: String,
    pub port: u16,
    pub username: String,
    /// `keep` uses the saved password of `serverId`; `set` uses the typed one.
    pub password: PasswordUpdate,
    /// Saved server the test belongs to (saved password and certificate decision).
    pub server_id: Option<ServerId>,
    /// Identity to try; `null` uses the default identity.
    pub identity_id: Option<u32>,
}

/// How a tested server certificate relates to the saved trust decision.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CertificateState {
    Trusted,
    Unknown,
    Changed,
}

/// Result of a test connection.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestConnection {
    /// Round trip of a Mumble ping over the control connection.
    pub ping_ms: u32,
    /// Version string the server reported, e.g. "1.5.0".
    pub version: String,
    /// SHA-256 fingerprint of the server certificate, as shown to users.
    pub fingerprint: String,
    pub certificate: CertificateState,
}

impl From<mumcord_engine::CertificateState> for CertificateState {
    fn from(state: mumcord_engine::CertificateState) -> Self {
        match state {
            mumcord_engine::CertificateState::Trusted => Self::Trusted,
            mumcord_engine::CertificateState::Unknown => Self::Unknown,
            mumcord_engine::CertificateState::Changed => Self::Changed,
        }
    }
}

impl From<TestConnectionView> for TestConnection {
    fn from(v: TestConnectionView) -> Self {
        Self {
            ping_ms: v.ping_ms,
            version: v.version,
            fingerprint: v.fingerprint,
            certificate: v.certificate.into(),
        }
    }
}

impl From<ServerTestInput> for mumcord_engine::ServerTestInput {
    fn from(i: ServerTestInput) -> Self {
        Self {
            address: i.address,
            port: i.port,
            username: i.username,
            password: i.password.into(),
            server_id: i.server_id.map(to_engine_server_id),
            identity_id: i.identity_id.map(super::types::to_engine_identity_id),
        }
    }
}

/// Parsed `mumble://[user[:password]@]host[:port]/[channel/path]` link.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleLink {
    pub address: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Channel names from the root down; empty for no channel.
    pub channel_path: Vec<String>,
}

/// A `mumble://` link was opened from outside (startup argument or second launch).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkOpened {
    pub link: MumbleLink,
    pub outcome: LinkOutcome,
}

/// What Mumcord did with a `mumble://` link (FR-2.13). The UI brings the named server into
/// view and, for `unknownServer`, offers to save it (FR-1.11).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LinkOutcome {
    /// The server was connected already; a channel path was joined. Show it.
    #[serde(rename_all = "camelCase")]
    Shown { server_id: ServerId },
    /// A connection to the saved server was started. Show it.
    #[serde(rename_all = "camelCase")]
    Connecting { server_id: ServerId },
    /// No saved server matches; the UI opens the add-server dialog filled from the link.
    UnknownServer,
    /// The server is saved but the connection limit is reached (`serverLimitReached`).
    #[serde(rename_all = "camelCase")]
    LimitReached { server_id: ServerId },
    /// Connecting failed for another reason.
    #[serde(rename_all = "camelCase")]
    Failed {
        server_id: ServerId,
        error: ApiError,
    },
}

impl From<ServerView> for SavedServer {
    fn from(v: ServerView) -> Self {
        Self {
            id: to_api_server_id(v.id),
            label: v.label,
            address: v.address,
            port: v.port,
            username: v.username,
            has_password: v.has_password,
            save_history: v.save_history,
            always_connect_at_start: v.always_connect_at_start,
            was_voice_server_at_quit: v.was_voice_server_at_quit,
            notifications_muted: v.notifications_muted,
            group_mentions_muted: v.group_mentions_muted,
            theme_id: v.theme_id,
            identity_id: v.identity_id.map(super::types::to_api_identity_id),
        }
    }
}

impl From<PasswordUpdate> for PasswordChange {
    fn from(update: PasswordUpdate) -> Self {
        match update {
            PasswordUpdate::Keep => Self::Keep,
            PasswordUpdate::Clear => Self::Clear,
            PasswordUpdate::Set { value } => Self::Set(value),
        }
    }
}

impl From<ServerInput> for ServerFields {
    fn from(i: ServerInput) -> Self {
        Self {
            label: i.label,
            address: i.address,
            port: i.port,
            username: i.username,
            password: i.password.into(),
        }
    }
}

/// Saved servers in server-bar order.
#[tauri::command]
#[specta::specta]
pub async fn servers_list(engine: State<'_, Engine>) -> ApiResult<Vec<SavedServer>> {
    Ok(engine
        .servers()
        .await?
        .into_iter()
        .map(SavedServer::from)
        .collect())
}

/// One-shot login probe for the add/edit dialog: ping, server version and certificate state.
#[tauri::command]
#[specta::specta]
pub async fn server_test_connection(
    engine: State<'_, Engine>,
    input: ServerTestInput,
) -> ApiResult<TestConnection> {
    Ok(engine.server_test_connection(input.into()).await?.into())
}

/// Adds a server at the end of the server bar.
#[tauri::command]
#[specta::specta]
pub async fn server_add(engine: State<'_, Engine>, input: ServerInput) -> ApiResult<SavedServer> {
    Ok(engine.server_add(input.into()).await?.into())
}

#[tauri::command]
#[specta::specta]
pub async fn server_update(
    engine: State<'_, Engine>,
    id: ServerId,
    input: ServerInput,
) -> ApiResult<SavedServer> {
    Ok(engine
        .server_update(to_engine_server_id(id), input.into())
        .await?
        .into())
}

/// Removes the server with its password, trust decision, follows and history.
#[tauri::command]
#[specta::specta]
pub async fn server_remove(engine: State<'_, Engine>, id: ServerId) -> ApiResult<()> {
    Ok(engine.server_remove(to_engine_server_id(id)).await?)
}

/// Marks the server "Always connect at start" (FR-2.7). Errors: `serverLimitReached` when
/// more servers would be marked than `maxConnectedServers` allows.
#[tauri::command]
#[specta::specta]
pub async fn server_set_always_connect(
    engine: State<'_, Engine>,
    server_id: ServerId,
    enabled: bool,
) -> ApiResult<SavedServer> {
    Ok(engine
        .server_set_always_connect(to_engine_server_id(server_id), enabled)
        .await?
        .into())
}

/// Mutes or unmutes all notifications of the server, mentions included (FR-2.10).
#[tauri::command]
#[specta::specta]
pub async fn server_set_notifications_muted(
    engine: State<'_, Engine>,
    server_id: ServerId,
    muted: bool,
) -> ApiResult<SavedServer> {
    Ok(engine
        .server_set_notifications_muted(to_engine_server_id(server_id), muted)
        .await?
        .into())
}

/// Turns `@all` and `@channel` mentions on the server off or back on (issue #70).
#[tauri::command]
#[specta::specta]
pub async fn server_set_group_mentions_muted(
    engine: State<'_, Engine>,
    server_id: ServerId,
    muted: bool,
) -> ApiResult<SavedServer> {
    Ok(engine
        .server_set_group_mentions_muted(to_engine_server_id(server_id), muted)
        .await?
        .into())
}

/// Sets the theme shown with the server; `null` goes back to the app theme (FR-2.17).
#[tauri::command]
#[specta::specta]
pub async fn server_set_theme(
    engine: State<'_, Engine>,
    server_id: ServerId,
    theme_id: Option<String>,
) -> ApiResult<SavedServer> {
    Ok(engine
        .server_set_theme(to_engine_server_id(server_id), theme_id)
        .await?
        .into())
}

/// Sets the server-bar order; `ids` must contain every saved server exactly once.
#[tauri::command]
#[specta::specta]
pub fn servers_reorder(engine: State<'_, Engine>, ids: Vec<ServerId>) -> ApiResult<()> {
    let ids: Vec<_> = ids.into_iter().map(to_engine_server_id).collect();
    Ok(engine.servers_reorder(&ids)?)
}

/// Parses a pasted `mumble://` link; fails with `invalidInput` if malformed.
#[tauri::command]
#[specta::specta]
pub fn parse_mumble_link(url: String) -> ApiResult<MumbleLink> {
    parse_link(&url)
}

/// Parses `mumble://[user[:password]@]host[:port]/[channel/path][?query]`.
pub fn parse_link(input: &str) -> ApiResult<MumbleLink> {
    let invalid = |msg: &str| ApiError::new(ErrorCode::InvalidInput, msg);
    let url = tauri::Url::parse(input.trim()).map_err(|_| invalid("not a valid link"))?;
    if !url.scheme().eq_ignore_ascii_case("mumble") {
        return Err(invalid("not a mumble:// link"));
    }
    let host = url
        .host_str()
        .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
        .filter(|h| !h.is_empty())
        .ok_or_else(|| invalid("the link has no server address"))?;
    let decode = |part: &str| percent_decode(part).ok_or_else(|| invalid("bad escape in link"));
    let username = match url.username() {
        "" => None,
        user => Some(decode(user)?),
    };
    let password = url.password().map(decode).transpose()?;
    let channel_path = match url.path_segments() {
        Some(segments) => segments
            .filter(|s| !s.is_empty())
            .map(decode)
            .collect::<ApiResult<Vec<_>>>()?,
        None => Vec::new(),
    };
    Ok(MumbleLink {
        address: host.to_owned(),
        port: url.port().unwrap_or(DEFAULT_PORT),
        username,
        password,
        channel_path,
    })
}

/// Decodes `%XX` escapes as UTF-8; `None` on malformed escapes or invalid UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_link() {
        let link = parse_link(
            "mumble://Anna%20B:p%40ss@voice.example.org:64739/Raid/Team%201?version=1.2.0",
        )
        .expect("valid");
        assert_eq!(
            link,
            MumbleLink {
                address: "voice.example.org".into(),
                port: 64739,
                username: Some("Anna B".into()),
                password: Some("p@ss".into()),
                channel_path: vec!["Raid".into(), "Team 1".into()],
            }
        );
    }

    #[test]
    fn parses_minimal_and_ipv6_links() {
        let link = parse_link("mumble://example.org").expect("valid");
        assert_eq!(link.port, DEFAULT_PORT);
        assert_eq!(link.username, None);
        assert_eq!(link.password, None);
        assert!(link.channel_path.is_empty());

        let link = parse_link("mumble://[::1]:1234/").expect("valid");
        assert_eq!(link.address, "::1");
        assert_eq!(link.port, 1234);
    }

    #[test]
    fn rejects_bad_links() {
        for bad in [
            "",
            "http://example.org",
            "mumble://",
            "mumble:///Raid",
            "not a link",
        ] {
            let err = parse_link(bad).expect_err(bad);
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad}");
        }
        assert!(parse_link("mumble://example.org/%ZZ").is_err());
    }
}
