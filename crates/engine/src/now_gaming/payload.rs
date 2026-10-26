//! The plugin data payload that shares a game with other Mumcord users (FR-1.59).
//!
//! Small versioned JSON: `{"v":1,"game":…,"steam_app_id":…,"started_at":…,"join":…}`.
//! `{"v":1}` without a game means "not playing". `started_at` is the sender's clock (Unix ms).
//! Received payloads come from other clients and are untrusted: the decoder caps the name and
//! keeps a join target only when it is a well-formed `steam://connect/<host>:<port>` link.

use mumcord_protocol::PLUGIN_DATA_MAX;
use serde::{Deserialize, Serialize};

use crate::web::meta::clean;

/// Plugin data id of the payload; other ids are ignored.
pub const DATA_ID: &str = "mumcord.nowgaming";
/// The payload version this client sends. Any version >= 1 is accepted on receipt and unknown
/// fields are ignored. Minor additions keep v=1 and old clients ignore unknown fields; a breaking change gets a new data id.
pub const VERSION: u32 = 1;
/// Longest game name kept (characters).
pub const MAX_NAME: usize = 100;
/// The only kind of join target: a Steam game server address.
pub const STEAM_CONNECT: &str = "steam://connect/";

/// One shared game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedGame {
    pub name: String,
    pub steam_app_id: Option<u32>,
    /// Unix ms (sender's clock) when the game was first detected.
    pub started_at_ms: u64,
    /// `steam://connect/<host>:<port>`, see [`join_link`].
    pub join: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PayloadError {
    #[error("payload too large")]
    TooLarge,
    #[error("payload is not valid JSON")]
    Malformed,
    #[error("unknown payload version {0}")]
    Version(u32),
}

#[derive(Serialize, Deserialize, Default)]
struct Wire {
    v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    game: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    steam_app_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    join: Option<String>,
}

/// `link` if it is exactly `steam://connect/<host>:<port>` with a plain host name or IPv4
/// address and a port in range; anything else (other schemes, paths, passwords, odd characters)
/// is rejected, because the link is handed to the operating system on "Join game".
pub fn join_link(link: &str) -> Option<String> {
    let address = link.strip_prefix(STEAM_CONNECT)?;
    let (host, port) = address.rsplit_once(':')?;
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['.', '-'])
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    let port_ok = !port.is_empty()
        && port.len() <= 5
        && port.chars().all(|c| c.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|p| p > 0);
    (host_ok && port_ok).then(|| link.to_owned())
}

/// The payload for `game`, or the "not playing" payload for `None`. Always at most
/// [`PLUGIN_DATA_MAX`] bytes (a capped name and a valid join link always fit).
pub fn encode(game: Option<&SharedGame>) -> Vec<u8> {
    let wire = match game {
        None => Wire {
            v: VERSION,
            ..Wire::default()
        },
        Some(game) => Wire {
            v: VERSION,
            game: Some(clean(&game.name, MAX_NAME).unwrap_or_else(|| "?".into())),
            steam_app_id: game.steam_app_id.filter(|id| *id > 0),
            started_at: Some(game.started_at_ms),
            join: game.join.as_deref().and_then(join_link),
        },
    };
    // Serializing plain strings and numbers cannot fail.
    let json = serde_json::to_vec(&wire).unwrap_or_default();
    debug_assert!(json.len() <= PLUGIN_DATA_MAX);
    json
}

/// Reads a received payload: `Ok(None)` means the sender stopped sharing.
pub fn decode(data: &[u8]) -> Result<Option<SharedGame>, PayloadError> {
    if data.len() > PLUGIN_DATA_MAX {
        return Err(PayloadError::TooLarge);
    }
    let wire: Wire = serde_json::from_slice(data).map_err(|_| PayloadError::Malformed)?;
    if wire.v < VERSION {
        return Err(PayloadError::Version(wire.v));
    }
    let Some(name) = wire.game.as_deref().and_then(|g| clean(g, MAX_NAME)) else {
        return Ok(None);
    };
    Ok(Some(SharedGame {
        name,
        steam_app_id: wire.steam_app_id.filter(|id| *id > 0),
        started_at_ms: wire.started_at.unwrap_or(0),
        join: wire.join.as_deref().and_then(join_link),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> SharedGame {
        SharedGame {
            name: "Counter-Strike 2".into(),
            steam_app_id: Some(730),
            started_at_ms: 1_700_000_000_000,
            join: Some("steam://connect/192.0.2.10:27015".into()),
        }
    }

    #[test]
    fn round_trips_a_game() {
        let data = encode(Some(&game()));
        let json: serde_json::Value = serde_json::from_slice(&data).expect("json");
        assert_eq!(json["v"], 1);
        assert_eq!(json["game"], "Counter-Strike 2");
        assert_eq!(json["steam_app_id"], 730);
        assert_eq!(json["started_at"], 1_700_000_000_000_u64);
        assert_eq!(json["join"], "steam://connect/192.0.2.10:27015");
        assert_eq!(decode(&data), Ok(Some(game())));
    }

    #[test]
    fn optional_fields_are_left_out() {
        let plain = SharedGame {
            steam_app_id: None,
            join: None,
            ..game()
        };
        let data = encode(Some(&plain));
        assert_eq!(
            data,
            br#"{"v":1,"game":"Counter-Strike 2","started_at":1700000000000}"#
        );
        assert_eq!(decode(&data), Ok(Some(plain)));
    }

    #[test]
    fn stop_is_a_bare_version() {
        let data = encode(None);
        assert_eq!(data, br#"{"v":1}"#);
        assert_eq!(decode(&data), Ok(None));
        assert_eq!(decode(br#"{"v":1,"game":"  "}"#), Ok(None));
    }

    #[test]
    fn long_names_fit_the_server_limit() {
        let mut long = game();
        long.name = "🎮".repeat(500);
        long.join = Some(format!("steam://connect/{}:27015", "a".repeat(253)));
        let data = encode(Some(&long));
        assert!(data.len() <= PLUGIN_DATA_MAX, "{} bytes", data.len());
        let decoded = decode(&data).expect("valid").expect("game");
        assert!(
            decoded.name.chars().count() <= MAX_NAME + 1,
            "cut with an ellipsis"
        );
        assert!(decoded.join.is_some());
    }

    #[test]
    fn newer_version_with_extra_fields_decodes() {
        let decoded = decode(br#"{"v":2,"game":"x","steam_app_id":730,"future":{"a":1}}"#)
            .expect("valid")
            .expect("game");
        assert_eq!(decoded.name, "x");
        assert_eq!(decoded.steam_app_id, Some(730));
    }

    #[test]
    fn untrusted_input_is_capped_and_filtered() {
        assert_eq!(
            decode(&vec![b' '; PLUGIN_DATA_MAX + 1]),
            Err(PayloadError::TooLarge)
        );
        assert_eq!(decode(b"not json"), Err(PayloadError::Malformed));
        assert_eq!(
            decode(br#"{"v":0,"game":"x"}"#),
            Err(PayloadError::Version(0))
        );
        let decoded =
            decode(br#"{"v":1,"game":"a\u0000b\u202e","steam_app_id":0,"join":"steam://run/730"}"#)
                .expect("valid")
                .expect("game");
        assert_eq!(decoded.name, "a b");
        assert_eq!(decoded.steam_app_id, None);
        assert_eq!(decoded.join, None);
        assert_eq!(decoded.started_at_ms, 0);
    }

    #[test]
    fn only_plain_steam_connect_links_are_join_targets() {
        for ok in [
            "steam://connect/192.0.2.10:27015",
            "steam://connect/play.example.org:1",
        ] {
            assert_eq!(join_link(ok).as_deref(), Some(ok));
        }
        for bad in [
            "steam://connect/192.0.2.10",
            "steam://connect/192.0.2.10:0",
            "steam://connect/192.0.2.10:65536",
            "steam://connect/host:27015/password",
            "steam://connect/-host:27015",
            "steam://connect/:27015",
            "steam://connect/a b:27015",
            "steam://install/730",
            "https://example.org:443",
            "file:///etc/passwd:1",
        ] {
            assert_eq!(join_link(bad), None, "{bad}");
        }
    }
}
