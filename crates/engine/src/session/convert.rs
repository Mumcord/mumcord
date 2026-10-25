//! Conversions from protocol/store types to engine views, channel paths and error mapping.

use mumcord_protocol::permissions;
use mumcord_protocol::{
    Channel, CommandError, ConnectError, DenyKind, PermissionDenied, RejectReason, ServerState,
    User,
};
use mumcord_store::PersonSetting;

use super::machine::Failure;
use super::types::{
    ChannelPermissions, ChannelView, EngineError, ErrorKind, ServerInfoView, UserView,
};

/// The root channel always has id 0 in Mumble.
pub const ROOT_CHANNEL: u32 = 0;

/// Channel path below the root, e.g. `["Raid", "Team 1"]`; empty for the root itself.
pub fn channel_path(state: &ServerState, id: u32) -> Option<Vec<String>> {
    let mut path = Vec::new();
    let mut current = state.channels.get(&id)?;
    while let Some(parent) = current.parent {
        path.push(current.name.clone());
        current = state.channels.get(&parent)?;
    }
    path.reverse();
    Some(path)
}

/// Stored form of a channel path (store columns `last_channel`, `channel_path`).
pub fn join_path(path: &[String]) -> String {
    path.join("/")
}

pub fn split_path(stored: &str) -> Vec<String> {
    stored
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Finds a channel by its path below the root.
pub fn resolve_path(state: &ServerState, path: &[String]) -> Option<u32> {
    let mut id = ROOT_CHANNEL;
    state.channels.get(&id)?;
    for name in path {
        id = state.child_by_name(id, name)?.id;
    }
    Some(id)
}

pub fn channel_view(state: &ServerState, channel: &Channel) -> ChannelView {
    let bits = state.permissions.get(&channel.id).copied();
    let has = |bit: u32| bits.is_none_or(|b| b & bit != 0);
    let enter = match (channel.can_enter, bits) {
        (Some(can), _) => can,
        (None, Some(b)) => b & permissions::ENTER != 0,
        (None, None) => !channel.is_enter_restricted,
    };
    ChannelView {
        id: channel.id,
        parent_id: channel.parent,
        name: channel.name.clone(),
        position: channel.position,
        permissions: ChannelPermissions {
            enter,
            text_message: has(permissions::TEXT_MESSAGE),
            listen: has(permissions::LISTEN),
            register_self: channel.id == ROOT_CHANNEL
                && bits.is_some_and(|b| b & permissions::SELF_REGISTER != 0),
        },
        description: channel
            .description
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .map(sanitize_html),
        has_description: super::description::has_description(channel),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

pub fn user_view(user: &User, local: PersonSetting, mumcord: Option<&[String]>) -> UserView {
    UserView {
        session: user.session,
        channel_id: user.channel_id,
        name: user.name.clone(),
        cert_hash: user.cert_hash.clone(),
        registered: user.user_id.is_some(),
        self_mute: user.self_mute,
        self_deaf: user.self_deaf,
        server_mute: user.mute,
        server_deaf: user.deaf,
        suppressed: user.suppress,
        listening_channel_ids: user.listening.iter().copied().collect(),
        avatar_hash: user.texture_hash.as_deref().map(hex),
        has_avatar: user.texture_hash.is_some()
            || user.texture.as_ref().is_some_and(|t| !t.is_empty()),
        has_comment: user.comment_hash.is_some()
            || user.comment.as_ref().is_some_and(|c| !c.is_empty()),
        volume: local.volume,
        local_mute: local.local_mute,
        mumcord_features: mumcord.map(<[String]>::to_vec),
    }
}

pub fn server_info_view(state: &ServerState) -> ServerInfoView {
    let info = &state.server;
    ServerInfoView {
        version: info
            .version
            .map(|v| v.to_string())
            .or_else(|| info.release.clone())
            .unwrap_or_default(),
        follow_supported: info.supports_listening(),
        welcome_text: info
            .welcome_text
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(sanitize_html),
        default_channel_id: None,
        max_message_length: info.message_length.unwrap_or(0),
        max_image_message_length: info.image_message_length.unwrap_or(0),
        allow_html: info.allow_html.unwrap_or(true),
    }
}

/// Server-provided HTML is untrusted: strip scripts, handlers and remote resources.
pub use crate::chat::sanitize_html;

/// Classifies a failed connection attempt.
pub fn connect_failure(err: &ConnectError) -> Failure {
    use ErrorKind as K;
    let e = |kind, msg: String| EngineError::new(kind, msg);
    match err {
        ConnectError::Io(_) | ConnectError::Timeout | ConnectError::Closed => {
            Failure::Transient(e(K::ServerUnreachable, err.to_string()))
        }
        ConnectError::Tls(_) | ConnectError::Protocol(_) => {
            Failure::Transient(e(K::ServerUnreachable, err.to_string()))
        }
        ConnectError::InvalidAddress(_) => Failure::Fatal(e(K::InvalidInput, err.to_string())),
        ConnectError::CertificateRejected(_) => {
            Failure::Fatal(e(K::CertificateRejected, err.to_string()))
        }
        ConnectError::Rejected { reason, message } => {
            let kind = match reason {
                RejectReason::WrongServerPassword | RejectReason::WrongUserPassword => {
                    K::WrongPassword
                }
                RejectReason::UsernameInUse | RejectReason::InvalidUsername => K::NameInUse,
                RejectReason::NoCertificate | RejectReason::AuthenticatorFail => {
                    K::CertificateRejected
                }
                RejectReason::WrongVersion => K::ServerTooOld,
                RejectReason::ServerFull => K::ServerUnreachable,
                RejectReason::None | RejectReason::Unknown(_) => K::Internal,
            };
            // A full server may have room on the next attempt.
            let error = e(kind, format!("{reason:?}: {message}"));
            if *reason == RejectReason::ServerFull {
                Failure::Transient(error)
            } else {
                Failure::Fatal(error)
            }
        }
    }
}

pub fn command_error(err: CommandError) -> EngineError {
    match err {
        CommandError::NotConnected => EngineError::not_connected(),
        CommandError::Unsupported { .. } => {
            EngineError::new(ErrorKind::ServerTooOld, err.to_string())
        }
    }
}

pub fn denial_error(denied: &PermissionDenied) -> EngineError {
    let kind = match denied.kind {
        DenyKind::UserName => ErrorKind::NameInUse,
        DenyKind::TextTooLong => ErrorKind::MessageTooLarge,
        DenyKind::ChannelListenerLimit | DenyKind::UserListenerLimit => {
            ErrorKind::ListenerLimitReached
        }
        _ => ErrorKind::PermissionDenied,
    };
    let message = denied
        .reason
        .clone()
        .unwrap_or_else(|| format!("{:?}", denied.kind));
    EngineError::new(kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> ServerState {
        let mut state = ServerState::default();
        for (id, parent, name) in [
            (0, None, "Root"),
            (1, Some(0), "Lobby"),
            (2, Some(0), "Raid"),
            (3, Some(2), "Team 1"),
        ] {
            state.channels.insert(
                id,
                Channel {
                    id,
                    parent,
                    name: name.into(),
                    ..Channel::default()
                },
            );
        }
        state
    }

    #[test]
    fn paths_round_trip() {
        let state = tree();
        let path = channel_path(&state, 3).expect("path");
        assert_eq!(path, ["Raid", "Team 1"]);
        assert_eq!(join_path(&path), "Raid/Team 1");
        assert_eq!(resolve_path(&state, &split_path("Raid/Team 1")), Some(3));
        assert_eq!(resolve_path(&state, &[]), Some(ROOT_CHANNEL));
        assert_eq!(channel_path(&state, 0), Some(vec![]));
        assert_eq!(resolve_path(&state, &split_path("Raid/Nope")), None);
    }

    #[test]
    fn permissions_default_open_until_known() {
        let mut state = tree();
        let raid = state.channels[&2].clone();
        let view = channel_view(&state, &raid);
        assert!(view.permissions.enter && view.permissions.text_message);
        assert!(!view.permissions.register_self);

        state.permissions.insert(2, permissions::TRAVERSE);
        let view = channel_view(&state, &raid);
        assert!(!view.permissions.enter && !view.permissions.text_message);

        state.permissions.insert(0, permissions::SELF_REGISTER);
        let root = state.channels[&0].clone();
        assert!(channel_view(&state, &root).permissions.register_self);
    }

    #[test]
    fn welcome_text_is_sanitized() {
        let clean = sanitize_html("<b>hi</b><script>alert(1)</script><img src=x onerror=y>");
        assert!(clean.contains("<b>hi</b>"));
        assert!(!clean.contains("script") && !clean.contains("onerror"));
    }

    #[test]
    fn reject_reasons_map_to_codes() {
        let rejected = |reason| ConnectError::Rejected {
            reason,
            message: String::new(),
        };
        assert!(matches!(
            connect_failure(&rejected(RejectReason::WrongServerPassword)),
            Failure::Fatal(e) if e.kind == ErrorKind::WrongPassword
        ));
        assert!(matches!(
            connect_failure(&rejected(RejectReason::UsernameInUse)),
            Failure::Fatal(e) if e.kind == ErrorKind::NameInUse
        ));
        assert!(matches!(
            connect_failure(&ConnectError::Timeout),
            Failure::Transient(e) if e.kind == ErrorKind::ServerUnreachable
        ));
    }
}
