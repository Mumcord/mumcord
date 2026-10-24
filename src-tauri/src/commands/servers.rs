//! Saved servers and `mumble://` link parsing (FR-1.7, FR-1.8, FR-1.11).

use mumcord_engine::{Engine, PasswordChange, ServerFields, ServerView};
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
            password: match i.password {
                PasswordUpdate::Keep => PasswordChange::Keep,
                PasswordUpdate::Clear => PasswordChange::Clear,
                PasswordUpdate::Set { value } => PasswordChange::Set(value),
            },
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
