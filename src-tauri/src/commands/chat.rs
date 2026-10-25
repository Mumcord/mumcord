//! Sending and receiving text messages (FR-1.28–1.30).

use mumcord_engine::{ChatMessageView, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiError, ApiResult, ChannelId, Conversation, ErrorCode, SessionId};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// Locally unique id; also the paging cursor for `history_load`.
    pub id: String,
    pub conversation: Conversation,
    pub sender_name: String,
    pub sender_cert_hash: Option<String>,
    /// Sender's session while still connected; `null` for history entries.
    pub sender_session: Option<SessionId>,
    /// True for messages the own user sent.
    pub own: bool,
    /// Local receive (or send) time, RFC 3339.
    pub sent_at: String,
    /// Sanitized HTML; the UI sanitizes again before rendering.
    pub body: String,
    /// Links whose preview card was dismissed by the user (`preview_dismiss`).
    pub dismissed_previews: Vec<String>,
    /// Name of the person whose history this message was synced from (FR-1.60); `null` for
    /// messages received or sent here. Show "from ‹name›'s history".
    pub synced_from: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MessageReceived {
    pub message: ChatMessage,
}

/// Sends Mumble-compatible HTML (the UI converts Markdown and scales images).
/// Fails with `messageTooLarge` above the server limit or `permissionDenied`.
#[tauri::command]
#[specta::specta]
pub fn send_channel_message(
    engine: State<'_, Engine>,
    channel_id: ChannelId,
    html: String,
) -> ApiResult<ChatMessage> {
    Ok(engine.send_channel_message(channel_id, &html)?.into())
}

#[tauri::command]
#[specta::specta]
pub fn send_private_message(
    engine: State<'_, Engine>,
    session: SessionId,
    html: String,
) -> ApiResult<ChatMessage> {
    Ok(engine.send_private_message(session, &html)?.into())
}

impl From<ChatMessageView> for ChatMessage {
    fn from(v: ChatMessageView) -> Self {
        Self {
            id: v.id,
            conversation: v.conversation.into(),
            sender_name: v.sender_name,
            sender_cert_hash: v.sender_cert_hash,
            sender_session: v.sender_session,
            own: v.own,
            sent_at: rfc3339(v.sent_at),
            body: v.body,
            dismissed_previews: v.dismissed_previews,
            synced_from: v.synced_from,
        }
    }
}

/// Unix milliseconds as RFC 3339 in UTC, e.g. `2026-10-24T08:15:30.250Z`.
fn rfc3339(millis: i64) -> String {
    let secs = millis.div_euclid(1000);
    let ms = millis.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        tod / 3600,
        tod % 3600 / 60,
        tod % 60
    )
}

/// Writes the image of a `data:` URL to `path` (the viewer's "Save image", FR-1.56). The data is
/// already in the UI, so nothing is fetched again.
#[tauri::command]
#[specta::specta]
pub async fn save_image(path: String, data_url: String) -> ApiResult<()> {
    let bytes = decode_image_data_url(&data_url)?;
    std::fs::write(&path, bytes).map_err(|err| ApiError::new(ErrorCode::Io, err.to_string()))
}

/// The bytes of a base64 `data:image/...` URL; anything else is rejected.
pub(crate) fn decode_image_data_url(data_url: &str) -> Result<Vec<u8>, ApiError> {
    use base64::Engine as _;
    let invalid = || ApiError::new(ErrorCode::InvalidInput, "not an image data URL");
    let rest = data_url.strip_prefix("data:image/").ok_or_else(invalid)?;
    let (_, payload) = rest.split_once(";base64,").ok_or_else(invalid)?;
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::{decode_image_data_url, rfc3339};

    #[test]
    fn formats_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(951_782_400_123), "2000-02-29T00:00:00.123Z");
        assert_eq!(rfc3339(1_792_829_730_250), "2026-10-24T08:15:30.250Z");
    }

    #[test]
    fn decodes_image_data_urls_only() {
        assert_eq!(
            decode_image_data_url("data:image/png;base64,AQID").expect("valid data URL"),
            vec![1, 2, 3]
        );
        assert!(decode_image_data_url("data:text/html;base64,AQID").is_err());
        assert!(decode_image_data_url("https://example.com/a.png").is_err());
        assert!(decode_image_data_url("data:image/png;base64,***").is_err());
    }
}
