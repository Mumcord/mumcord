//! Direct connections between Mumcord users (FR-2.35, FR-2.36).

use mumcord_engine::Engine;
use mumcord_engine::p2p::{self as engine_p2p, ConnectionPath, RelayTransport};
use mumcord_engine::peer;
use mumcord_engine::peer::RelayRefusal as EngineRefusal;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::rfc3339;
use super::types::{ApiResult, ServerId, SessionId, to_engine_server_id};

/// A direct connection would be used for the first time while `allowDirectConnections` is
/// still `unset`; the UI asks once and answers with `p2p_consent_decide`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DirectConsentRequired {
    pub server_id: ServerId,
}

/// Stores the answer to `directConsentRequired` as `allowDirectConnections` and continues or
/// drops the waiting direct connection.
#[tauri::command]
#[specta::specta]
pub async fn p2p_consent_decide(engine: State<'_, Engine>, allow: bool) -> ApiResult<()> {
    Ok(engine.p2p_consent_decide(allow).await?)
}

/// How a P2P connection travels (FR-4.19): "Direct", "Via relay (UDP)" or
/// "Via relay (TCP 443)".
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum P2pPath {
    Direct,
    RelayUdp,
    RelayTcp,
}

/// The open P2P connection to a person, for the connection info in the user info and the
/// screen share viewer header (FR-4.19).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct P2pLink {
    pub path: P2pPath,
    /// "IP hidden" when true, else "IP visible": this client sent no addresses and the
    /// connection goes through the relay.
    pub ip_hidden: bool,
}

/// A P2P connection to `session` opened or changed (`link` set) or closed (`null`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct P2pLinkChanged {
    pub server_id: ServerId,
    pub session: SessionId,
    pub link: Option<P2pLink>,
}

impl From<engine_p2p::P2pLink> for P2pLink {
    fn from(l: engine_p2p::P2pLink) -> Self {
        Self {
            path: match l.path {
                ConnectionPath::Direct => P2pPath::Direct,
                ConnectionPath::Relay(RelayTransport::Udp) => P2pPath::RelayUdp,
                ConnectionPath::Relay(RelayTransport::Tcp) => P2pPath::RelayTcp,
            },
            ip_hidden: l.ip_hidden,
        }
    }
}

/// The open P2P connection to `session`, or `null` without one; changes arrive as
/// `p2pLinkChanged`.
#[tauri::command]
#[specta::specta]
pub fn p2p_link(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
) -> ApiResult<Option<P2pLink>> {
    Ok(engine
        .p2p_link(to_engine_server_id(server_id), session)
        .map(Into::into))
}

/// Why the relay refused a connection (spec 04 section 6.5).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum RelayRefusal {
    UnknownToken,
    Expired,
    NotAllowed,
    BotGone,
    RelayFull,
    PeerLimit,
    RateLimited,
    Version,
    BadRequest,
    /// The relay's owner turned off what the connection was for (FR-7.4).
    UseNotAllowed,
    /// The broadcast has no room for another viewer.
    BroadcastFull,
}

/// What the relay may be used for (FR-7.7): "Images", "Files", "Screen sharing".
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum RelayUse {
    Images,
    Files,
    ScreenShare,
}

/// Why the relay cannot be used; the UI shows the text of FR-4.4, FR-4.11 or FR-4.14.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RelayUnavailable {
    /// "This server has no relay".
    NoRelay,
    NotConnected,
    /// The announced relay changed and is not trusted.
    Untrusted,
    /// Two relays are announced.
    Conflict,
    /// "The relay bot left the server."
    BotLeft,
    /// "The relay is not reachable right now (reported by the relay bot)."
    RelayDown,
    /// "The relay bot did not answer."
    BotNoAnswer,
    /// "The relay refused the connection: {reason}".
    Refused {
        refusal: RelayRefusal,
    },
    /// "The relay could not be reached over UDP or TCP 443."
    Unreachable,
    /// "The other person can't use the relay (older Mumcord or relay turned off)."
    PeerCannotRelay,
    Denied,
    PeerGone,
    RateLimited,
    RelayFull,
    /// "This server's relay doesn't allow {images / file sharing / screen sharing}" (FR-7.7).
    UseNotAllowed {
        #[serde(rename = "use")]
        relay_use: RelayUse,
    },
}

/// State of the server's relay in the server info panel (FR-4.20).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RelayStatus {
    /// No relay announced: "Mumpeer relay: No".
    None,
    Available,
    Unavailable {
        reason: RelayUnavailable,
    },
    /// "The relay of this server changed" with "Trust new relay" / "Keep blocked";
    /// `keptBlocked` once the user chose to keep it blocked.
    Untrusted {
        #[serde(rename = "keptBlocked")]
        kept_blocked: bool,
    },
    /// "Two relays are announced on this server; none is used."
    Conflict,
}

/// The relay of a connected server as the server info panel shows it (FR-4.20).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelayInfo {
    /// User names of the announcing relay bots ("Mumbot: Yes"); empty for "No".
    pub bots: Vec<String>,
    /// Host of the announced relay.
    pub host: Option<String>,
    pub status: RelayStatus,
    /// When the pinned relay was first accepted (RFC 3339).
    pub first_seen: Option<String>,
    /// "Relay allows: …" (FR-7.7): all uses for an older bot, empty without a relay.
    pub uses: Vec<RelayUse>,
    /// Most viewers per broadcast; `null` when the relay offers no broadcasts.
    pub broadcast_max: Option<u16>,
}

/// The relay of a server changed (FR-4.20).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct RelayChanged {
    pub server_id: ServerId,
    pub info: RelayInfo,
}

impl From<EngineRefusal> for RelayRefusal {
    fn from(r: EngineRefusal) -> Self {
        match r {
            EngineRefusal::UnknownToken => Self::UnknownToken,
            EngineRefusal::Expired => Self::Expired,
            EngineRefusal::NotAllowed => Self::NotAllowed,
            EngineRefusal::BotGone => Self::BotGone,
            EngineRefusal::UseNotAllowed => Self::UseNotAllowed,
            EngineRefusal::BroadcastFull => Self::BroadcastFull,
            EngineRefusal::RelayFull => Self::RelayFull,
            EngineRefusal::PeerLimit => Self::PeerLimit,
            EngineRefusal::RateLimited => Self::RateLimited,
            EngineRefusal::Version => Self::Version,
            EngineRefusal::BadRequest => Self::BadRequest,
        }
    }
}

impl From<peer::RelayUse> for RelayUse {
    fn from(u: peer::RelayUse) -> Self {
        match u {
            peer::RelayUse::Images => Self::Images,
            peer::RelayUse::Files => Self::Files,
            peer::RelayUse::ScreenShare => Self::ScreenShare,
        }
    }
}

impl From<peer::RelayUnavailable> for RelayUnavailable {
    fn from(r: peer::RelayUnavailable) -> Self {
        use peer::RelayUnavailable as R;
        match r {
            R::NoRelay => Self::NoRelay,
            R::NotConnected => Self::NotConnected,
            R::Untrusted => Self::Untrusted,
            R::Conflict => Self::Conflict,
            R::BotLeft => Self::BotLeft,
            R::RelayDown => Self::RelayDown,
            R::BotNoAnswer => Self::BotNoAnswer,
            R::Refused(refusal) => Self::Refused {
                refusal: refusal.into(),
            },
            R::Unreachable => Self::Unreachable,
            R::PeerCannotRelay => Self::PeerCannotRelay,
            R::Denied => Self::Denied,
            R::PeerGone => Self::PeerGone,
            R::RateLimited => Self::RateLimited,
            R::RelayFull => Self::RelayFull,
            R::UseNotAllowed(relay_use) => Self::UseNotAllowed {
                relay_use: relay_use.into(),
            },
        }
    }
}

impl From<peer::RelayStatus> for RelayStatus {
    fn from(s: peer::RelayStatus) -> Self {
        match s {
            peer::RelayStatus::None => Self::None,
            peer::RelayStatus::Available => Self::Available,
            peer::RelayStatus::Unavailable(reason) => Self::Unavailable {
                reason: reason.into(),
            },
            peer::RelayStatus::Untrusted { kept_blocked } => Self::Untrusted { kept_blocked },
            peer::RelayStatus::Conflict => Self::Conflict,
        }
    }
}

impl From<peer::RelayInfo> for RelayInfo {
    fn from(i: peer::RelayInfo) -> Self {
        Self {
            bots: i.bots,
            host: i.host,
            status: i.status.into(),
            first_seen: i.first_seen.map(rfc3339),
            uses: i.uses.into_iter().map(Into::into).collect(),
            broadcast_max: i.broadcast_max,
        }
    }
}

/// The relay of a server for the server info panel; "no relay" when not connected.
#[tauri::command]
#[specta::specta]
pub fn relay_info(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<RelayInfo> {
    Ok(engine.relay_info(to_engine_server_id(server_id)).into())
}

/// "Trust new relay": pins the changed relay of a connected server.
#[tauri::command]
#[specta::specta]
pub fn relay_trust_new(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    Ok(engine.relay_trust_new(to_engine_server_id(server_id))?)
}

/// "Keep blocked": the changed relay stays unused.
#[tauri::command]
#[specta::specta]
pub fn relay_keep_blocked(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    Ok(engine.relay_keep_blocked(to_engine_server_id(server_id))?)
}

/// "Forget relay": removes the relay pin of a server.
#[tauri::command]
#[specta::specta]
pub fn relay_forget(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    Ok(engine.relay_forget(to_engine_server_id(server_id))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_use_reaches_the_ui_with_its_name() {
        let reason: RelayUnavailable =
            peer::RelayUnavailable::UseNotAllowed(peer::RelayUse::ScreenShare).into();
        assert_eq!(
            serde_json::to_value(reason).expect("json"),
            serde_json::json!({"kind": "useNotAllowed", "use": "screenShare"})
        );
        let refused: RelayRefusal = EngineRefusal::UseNotAllowed.into();
        assert_eq!(refused, RelayRefusal::UseNotAllowed);
    }

    #[test]
    fn relay_info_carries_uses_and_broadcast_size() {
        let info: RelayInfo = peer::RelayInfo {
            uses: vec![peer::RelayUse::Images, peer::RelayUse::Files],
            broadcast_max: Some(25),
            ..peer::RelayInfo::default()
        }
        .into();
        assert_eq!(info.uses, [RelayUse::Images, RelayUse::Files]);
        let json = serde_json::to_value(&info).expect("json");
        assert_eq!(json["uses"], serde_json::json!(["images", "files"]));
        assert_eq!(json["broadcastMax"], 25);
    }
}
