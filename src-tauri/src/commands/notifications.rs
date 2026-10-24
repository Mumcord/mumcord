//! Message notifications: toast or system notification per new message (FR-1.48c, QA.16).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::types::{ApiResult, Conversation};
use crate::notifications;

/// One new message as the UI would announce it. Texts are translated and plain (no HTML).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageNotice {
    pub conversation: Conversation,
    /// The UI currently shows this chat (selected, not covered by settings).
    pub on_screen: bool,
    /// Notification title, e.g. "#Raid" or "5 new messages in #Raid".
    pub summary: String,
    /// E.g. "Alice: see you at eight".
    pub body: String,
    /// Label of the click action, e.g. "Open chat" (some desktops show it as a button).
    pub action_label: String,
    /// The message mentions the user: plays the mention ping instead of the message sound.
    pub mention: bool,
}

/// What became of a new message.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum NotifyOutcome {
    /// Nothing to show (chat on screen, setting, or window in the background with "In-app").
    None,
    /// The UI shows its in-app toast.
    Toast,
    /// A system notification was shown.
    Desktop,
}

/// The user clicked a system notification; the window is already in front. Open this chat.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct NotificationActivated {
    pub conversation: Conversation,
}

/// Decides by window focus and the notification setting; shows the system notification itself.
#[tauri::command]
#[specta::specta]
pub async fn notify_message(
    app: AppHandle,
    engine: State<'_, Engine>,
    notice: MessageNotice,
) -> ApiResult<NotifyOutcome> {
    let mode = engine.settings()?.notifications;
    // The sound is independent of the notification mode; no session just means no sound.
    if let Err(e) = engine.message_sound(&notice.conversation.clone().into(), notice.mention) {
        tracing::debug!(error = %e, "message sound skipped");
    }
    Ok(notifications::notify(&app, mode, notice).await)
}

/// The user opened this chat: its system notification is outdated.
#[tauri::command]
#[specta::specta]
pub async fn notification_dismiss(app: AppHandle, conversation: Conversation) -> ApiResult<()> {
    notifications::dismiss(&app, &conversation).await;
    Ok(())
}
