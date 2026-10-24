//! Types shared by several command areas: errors, identifiers and conversations.

use std::fmt;

use serde::{Deserialize, Serialize};
use specta::Type;

/// Local id of a saved server.
pub type ServerId = u32;
/// Channel id as assigned by the connected server.
pub type ChannelId = u32;
/// Session id of a person on the connected server.
pub type SessionId = u32;

/// Result type of every command.
pub type ApiResult<T> = Result<T, ApiError>;

/// Machine-readable error category; the UI maps it to a plain-language message.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The command exists in the contract but the engine does not implement it yet.
    NotImplemented,
    /// The command needs an active connection.
    NotConnected,
    /// The server could not be reached (DNS, refused, timeout).
    ServerUnreachable,
    /// The server rejected the server password.
    WrongPassword,
    /// The username is already in use or registered to someone else.
    NameInUse,
    /// The server rejected the client certificate.
    CertificateRejected,
    /// A trusted server presented a different certificate; the user must decide.
    CertificateChanged,
    /// The server is too old for this feature (e.g. following needs 1.4).
    ServerTooOld,
    /// The server's ACL denied the action; `message` carries the server's reason.
    PermissionDenied,
    /// The server's channel-listener limit is reached.
    ListenerLimitReached,
    /// A message, image or avatar exceeds the server's size limit.
    MessageTooLarge,
    /// An argument was invalid (bad address, empty name, malformed link).
    InvalidInput,
    /// The referenced server, channel, person or file does not exist.
    NotFound,
    /// The OS secure storage is unavailable or refused access.
    SecureStorageUnavailable,
    /// Reading or writing a local file failed.
    Io,
    /// Unexpected internal failure.
    Internal,
}

/// Error returned by every failing command.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: ErrorCode,
    /// Technical detail or the server's denial text; never contains secrets.
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn not_implemented() -> Self {
        Self::new(ErrorCode::NotImplemented, "not implemented yet")
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Shared body of all stub handlers until the engine implements them.
pub fn not_implemented<T>() -> ApiResult<T> {
    Err(ApiError::not_implemented())
}

/// A chat conversation: a channel or a private chat with one person.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Conversation {
    #[serde(rename_all = "camelCase")]
    Channel { channel_id: ChannelId },
    /// Private chat keyed by the person's certificate hash; persists across sessions.
    #[serde(rename_all = "camelCase")]
    Private { cert_hash: String },
    /// Private chat with a person without certificate; lives only for this session.
    #[serde(rename_all = "camelCase")]
    PrivateSession { session: SessionId },
}
