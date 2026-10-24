//! Typed errors for connecting, sending commands and server denials.

use crate::proto;
use crate::tls::ServerCert;
use crate::version::MumbleVersion;

/// Malformed data on the control channel.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("frame of {0} bytes exceeds the size limit")]
    FrameTooLarge(usize),
    #[error("cannot decode {kind} message: {source}")]
    Decode {
        kind: &'static str,
        #[source]
        source: prost::DecodeError,
    },
}

/// Why the server refused the login (`Reject` message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    None,
    WrongVersion,
    InvalidUsername,
    WrongUserPassword,
    WrongServerPassword,
    UsernameInUse,
    ServerFull,
    NoCertificate,
    AuthenticatorFail,
    Unknown(i32),
}

impl RejectReason {
    pub(crate) fn from_wire(value: Option<i32>) -> Self {
        use proto::reject::RejectType as T;
        let Some(value) = value else {
            return Self::None;
        };
        match T::try_from(value) {
            Ok(T::None) => Self::None,
            Ok(T::WrongVersion) => Self::WrongVersion,
            Ok(T::InvalidUsername) => Self::InvalidUsername,
            Ok(T::WrongUserPw) => Self::WrongUserPassword,
            Ok(T::WrongServerPw) => Self::WrongServerPassword,
            Ok(T::UsernameInUse) => Self::UsernameInUse,
            Ok(T::ServerFull) => Self::ServerFull,
            Ok(T::NoCertificate) => Self::NoCertificate,
            Ok(T::AuthenticatorFail) => Self::AuthenticatorFail,
            Err(_) => Self::Unknown(value),
        }
    }
}

/// Failure while establishing a connection.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid server address: {0}")]
    InvalidAddress(String),
    /// The caller's certificate check said no. Carries the certificate so the caller can ask the
    /// user and reconnect (trust on first use, or a changed certificate).
    #[error("server certificate not trusted (SHA-1 {})", .0.sha1_hex)]
    CertificateRejected(ServerCert),
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),
    #[error("server rejected the login ({reason:?}): {message}")]
    Rejected {
        reason: RejectReason,
        message: String,
    },
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("server closed the connection during login")]
    Closed,
    #[error("login timed out")]
    Timeout,
}

/// A command could not be sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("not connected")]
    NotConnected,
    #[error("{feature} needs server {required} or newer (server is {server})")]
    Unsupported {
        feature: &'static str,
        required: MumbleVersion,
        server: MumbleVersion,
    },
}

/// Kind of a server denial (`PermissionDenied` message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyKind {
    /// Free-text denial; see `reason`.
    Text,
    /// A missing ACL permission; see `permission` and `channel_id`.
    Permission,
    SuperUser,
    ChannelName,
    TextTooLong,
    H9K,
    TemporaryChannel,
    MissingCertificate,
    UserName,
    ChannelFull,
    NestingLimit,
    ChannelCountLimit,
    /// The channel already has the maximum number of listeners.
    ChannelListenerLimit,
    /// This user already listens to the maximum number of channels.
    UserListenerLimit,
    Unknown(i32),
}

impl DenyKind {
    fn from_wire(value: Option<i32>) -> Self {
        use proto::permission_denied::DenyType as T;
        // The protocol's default for a missing type is Text.
        let Some(value) = value else {
            return Self::Text;
        };
        match T::try_from(value) {
            Ok(T::Text) => Self::Text,
            Ok(T::Permission) => Self::Permission,
            Ok(T::SuperUser) => Self::SuperUser,
            Ok(T::ChannelName) => Self::ChannelName,
            Ok(T::TextTooLong) => Self::TextTooLong,
            Ok(T::H9k) => Self::H9K,
            Ok(T::TemporaryChannel) => Self::TemporaryChannel,
            Ok(T::MissingCertificate) => Self::MissingCertificate,
            Ok(T::UserName) => Self::UserName,
            Ok(T::ChannelFull) => Self::ChannelFull,
            Ok(T::NestingLimit) => Self::NestingLimit,
            Ok(T::ChannelCountLimit) => Self::ChannelCountLimit,
            Ok(T::ChannelListenerLimit) => Self::ChannelListenerLimit,
            Ok(T::UserListenerLimit) => Self::UserListenerLimit,
            Err(_) => Self::Unknown(value),
        }
    }
}

/// A request was denied by the server.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("denied by server: {kind:?}")]
pub struct PermissionDenied {
    pub kind: DenyKind,
    /// The missing permission bit(s), see [`crate::permissions`].
    pub permission: Option<u32>,
    pub channel_id: Option<u32>,
    pub session: Option<u32>,
    /// Server-provided text (untrusted).
    pub reason: Option<String>,
    /// Offending name, for name-related denials (untrusted).
    pub name: Option<String>,
}

impl From<proto::PermissionDenied> for PermissionDenied {
    fn from(m: proto::PermissionDenied) -> Self {
        Self {
            kind: DenyKind::from_wire(m.r#type),
            permission: m.permission,
            channel_id: m.channel_id,
            session: m.session,
            reason: m.reason,
            name: m.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_mapping() {
        let m = proto::PermissionDenied {
            r#type: Some(12),
            channel_id: Some(2),
            ..Default::default()
        };
        let d = PermissionDenied::from(m);
        assert_eq!(d.kind, DenyKind::ChannelListenerLimit);
        assert_eq!(d.channel_id, Some(2));
        assert_eq!(DenyKind::from_wire(None), DenyKind::Text);
        assert_eq!(DenyKind::from_wire(Some(77)), DenyKind::Unknown(77));
    }

    #[test]
    fn reject_mapping() {
        assert_eq!(
            RejectReason::from_wire(Some(4)),
            RejectReason::WrongServerPassword
        );
        assert_eq!(
            RejectReason::from_wire(Some(5)),
            RejectReason::UsernameInUse
        );
        assert_eq!(RejectReason::from_wire(Some(99)), RejectReason::Unknown(99));
    }
}
