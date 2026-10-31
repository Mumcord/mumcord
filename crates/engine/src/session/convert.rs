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
        max_users: channel.max_users.filter(|&max| max > 0),
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
        release: info.release.clone(),
        os: info.os.clone(),
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
        max_bandwidth: info.max_bandwidth,
        max_users: info.max_users,
        recording_allowed: info.recording_allowed.unwrap_or(false),
        server_label: None,
        server_address: None,
        server_port: None,
        tls_fingerprint: None,
        users_online: u32::try_from(state.users.len()).unwrap_or(u32::MAX),
        session_id: state.session,
        registered: state.own_user().is_some_and(|u| u.user_id.is_some()),
        current_channel_permissions: state
            .own_user()
            .and_then(|u| state.permissions.get(&u.channel_id).copied()),
        mumcord_users: 0, // Filled in by the snapshot function, which knows the Mumcord peers.
        tls_trusted: true,
        tls_issuer: None,
        tls_valid_from: None,
        tls_valid_until: None,
        tls_certificate_pem: None,
    }
}

/// Server-provided HTML is untrusted: strip scripts, handlers and remote resources.
pub use crate::chat::sanitize_html;

/// Classifies a failed connection attempt.
pub fn connect_failure(err: &ConnectError) -> Failure {
    use ErrorKind as K;
    let e = |kind, msg: String| EngineError::new(kind, msg);
    match err {
        ConnectError::Io(io) if cfg!(target_os = "macos") && is_no_route(io) => {
            Failure::Transient(e(K::LocalNetworkBlocked, err.to_string()))
        }
        ConnectError::Resolve(_)
        | ConnectError::Io(_)
        | ConnectError::Timeout
        | ConnectError::Closed => Failure::Transient(e(K::ServerUnreachable, err.to_string())),
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
                // The UI shows the server's own text for an unexplained refusal.
                RejectReason::None | RejectReason::Unknown(_) => {
                    return Failure::Fatal(e(K::PermissionDenied, message.clone()));
                }
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

fn is_no_route(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::HostUnreachable | std::io::ErrorKind::NetworkUnreachable
    )
}

/// Why a connection attempt failed, for the log: a category plus the OS error code where there
/// is one. Never contains addresses, host or user names, or server-provided text.
pub fn failure_reason(err: &ConnectError) -> String {
    use std::io::ErrorKind as Io;
    let with_code = |category: &str, io: &std::io::Error| match io.raw_os_error() {
        Some(code) => format!("{category} (os error {code})"),
        None => category.to_owned(),
    };
    match err {
        ConnectError::Resolve(io) => with_code("dns lookup failed", io),
        ConnectError::Io(io) => {
            let category = match io.kind() {
                Io::ConnectionRefused => "connection refused",
                Io::TimedOut => "connect timed out",
                Io::HostUnreachable => "host unreachable",
                Io::NetworkUnreachable => "network unreachable",
                Io::ConnectionReset => "connection reset",
                Io::ConnectionAborted => "connection aborted",
                Io::PermissionDenied => "permission denied",
                Io::AddrNotAvailable => "address not available",
                Io::UnexpectedEof => "closed during tls handshake",
                _ => "network error",
            };
            with_code(category, io)
        }
        ConnectError::Tls(tls) => tls_reason(tls),
        ConnectError::Protocol(_) => "protocol error".to_owned(),
        ConnectError::InvalidAddress(_) => "invalid address".to_owned(),
        ConnectError::CertificateRejected(_) => "server certificate not trusted".to_owned(),
        ConnectError::Rejected { reason, .. } => format!("rejected by server: {reason:?}"),
        ConnectError::Closed => "server closed the connection during login".to_owned(),
        ConnectError::Timeout => "login timed out".to_owned(),
    }
}

/// Only fieldless detail is kept: some rustls errors carry host names or free text.
fn tls_reason(err: &rustls::Error) -> String {
    use rustls::Error as T;
    match err {
        T::AlertReceived(alert) => format!("tls: alert received: {alert:?}"),
        T::PeerIncompatible(rustls::PeerIncompatible::ServerRejectedEncryptedClientHello(_)) => {
            "tls: peer incompatible: encrypted client hello rejected".to_owned()
        }
        T::PeerIncompatible(why) => format!("tls: peer incompatible: {why:?}"),
        T::PeerMisbehaved(why) => format!("tls: peer misbehaved: {why:?}"),
        T::InvalidCertificate(_) => "tls: invalid certificate".to_owned(),
        _ => "tls: other error".to_owned(),
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
    // Without the server's own text there is nothing readable to show; the UI words it.
    let message = denied.reason.clone().unwrap_or_default();
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
    fn user_limit_zero_means_none() {
        let mut state = tree();
        assert_eq!(channel_view(&state, &state.channels[&2]).max_users, None);
        state.channels.get_mut(&2).expect("raid").max_users = Some(0);
        assert_eq!(channel_view(&state, &state.channels[&2]).max_users, None);
        state.channels.get_mut(&2).expect("raid").max_users = Some(5);
        assert_eq!(channel_view(&state, &state.channels[&2]).max_users, Some(5));
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

    #[test]
    fn unexplained_rejection_shows_the_server_text() {
        let failure = connect_failure(&ConnectError::Rejected {
            reason: RejectReason::None,
            message: "Banned".to_owned(),
        });
        assert!(matches!(
            failure,
            Failure::Fatal(e) if e.kind == ErrorKind::PermissionDenied && e.message == "Banned"
        ));
    }

    #[test]
    fn no_route_points_to_local_network_permission_on_macos_only() {
        let expected = if cfg!(target_os = "macos") {
            ErrorKind::LocalNetworkBlocked
        } else {
            ErrorKind::ServerUnreachable
        };
        for kind in [
            std::io::ErrorKind::HostUnreachable,
            std::io::ErrorKind::NetworkUnreachable,
        ] {
            let failure = connect_failure(&ConnectError::Io(kind.into()));
            assert!(matches!(failure, Failure::Transient(e) if e.kind == expected));
        }
        assert!(matches!(
            connect_failure(&ConnectError::Io(std::io::ErrorKind::ConnectionRefused.into())),
            Failure::Transient(e) if e.kind == ErrorKind::ServerUnreachable
        ));
    }

    #[test]
    fn failure_reason_names_the_category() {
        use std::io::{Error, ErrorKind as Io};
        let io = |kind: Io| failure_reason(&ConnectError::Io(Error::from(kind)));
        assert_eq!(io(Io::ConnectionRefused), "connection refused");
        assert_eq!(io(Io::TimedOut), "connect timed out");
        assert_eq!(io(Io::HostUnreachable), "host unreachable");
        assert_eq!(io(Io::NetworkUnreachable), "network unreachable");
        assert_eq!(
            failure_reason(&ConnectError::Resolve(Error::other(
                "nodename nor servname"
            ))),
            "dns lookup failed"
        );
        assert_eq!(failure_reason(&ConnectError::Timeout), "login timed out");
        assert_eq!(
            failure_reason(&ConnectError::Rejected {
                reason: RejectReason::WrongServerPassword,
                message: String::new(),
            }),
            "rejected by server: WrongServerPassword"
        );
        assert_eq!(
            failure_reason(&ConnectError::Tls(rustls::Error::AlertReceived(
                rustls::AlertDescription::HandshakeFailure
            ))),
            "tls: alert received: HandshakeFailure"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn failure_reason_carries_the_os_error_code() {
        // 113 is EHOSTUNREACH on Linux.
        let err = ConnectError::Io(std::io::Error::from_raw_os_error(113));
        assert_eq!(failure_reason(&err), "host unreachable (os error 113)");
    }

    #[test]
    fn failure_reason_contains_no_personal_data() {
        let host = "alice-home.example.org";
        let errors = [
            ConnectError::InvalidAddress(host.to_owned()),
            ConnectError::Rejected {
                reason: RejectReason::UsernameInUse,
                message: "alice is already connected".to_owned(),
            },
            ConnectError::Tls(rustls::Error::InvalidCertificate(
                rustls::CertificateError::NotValidForNameContext {
                    expected: rustls::pki_types::ServerName::try_from(host).expect("valid name"),
                    presented: vec![host.to_owned()],
                },
            )),
            ConnectError::Tls(rustls::Error::General(host.to_owned())),
        ];
        for err in &errors {
            let reason = failure_reason(err);
            assert!(!reason.contains("alice"), "{reason}");
        }
    }
}
