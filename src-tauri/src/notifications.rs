//! Message notifications (FR-1.48c, QA.16): whether a new message gets an in-app toast, a
//! system notification or nothing, and the system notifications themselves.
//!
//! The UI builds the (already translated and sanitized) texts and keeps the per-chat message
//! count; this module decides by focus and setting, shows one system notification per chat
//! (replaced on every new message instead of stacking) and turns a click on it into
//! `NotificationActivated`. Message texts are never logged.
//!
//! Platforms: Linux talks to the freedesktop notification service directly (click = "default"
//! action, `desktop-entry` hint for the app icon). Windows and macOS show the notification
//! without replacement or click handling, as the available APIs give neither for an
//! unpackaged desktop app.

use std::collections::HashMap;

use mumcord_engine::{ConnState, Engine, EngineEvent, store::NotificationMode};
use tauri::{AppHandle, Manager};

use crate::commands::notifications::{MessageNotice, NotifyOutcome};
use crate::commands::types::{Conversation, ServerId, to_api_server_id};

/// Whether a new message gets a toast, a system notification or nothing.
///
/// `focused`: the main window is visible, not minimized and has focus. `on_screen`: the UI
/// shows this message's chat (selected and not covered by settings); only counts while focused.
pub fn decide(mode: NotificationMode, focused: bool, on_screen: bool) -> NotifyOutcome {
    if focused && on_screen {
        return NotifyOutcome::None;
    }
    match (mode, focused) {
        (NotificationMode::Off, _) | (NotificationMode::InApp, false) => NotifyOutcome::None,
        (NotificationMode::Desktop, _) | (NotificationMode::Both, false) => NotifyOutcome::Desktop,
        (NotificationMode::InApp | NotificationMode::Both, true) => NotifyOutcome::Toast,
    }
}

/// Stable map key of a conversation.
fn key(server_id: ServerId, conversation: &Conversation) -> String {
    match conversation {
        Conversation::Channel { channel_id } => format!("{server_id}:c:{channel_id}"),
        Conversation::Private { cert_hash } => format!("{server_id}:p:{cert_hash}"),
        Conversation::PrivateSession { session } => format!("{server_id}:s:{session}"),
    }
}

/// The system notification currently shown per chat of a server, so a new message replaces it.
#[derive(Debug, Default)]
pub struct Groups {
    by_key: HashMap<String, (ServerId, Conversation, u32)>,
}

impl Groups {
    /// Notification id to replace for this chat; 0 asks for a new one.
    #[cfg(any(target_os = "linux", test))]
    pub fn replaces(&self, server_id: ServerId, conversation: &Conversation) -> u32 {
        self.by_key
            .get(&key(server_id, conversation))
            .map_or(0, |(_, _, id)| *id)
    }

    pub fn shown(&mut self, server_id: ServerId, conversation: Conversation, id: u32) {
        self.by_key
            .insert(key(server_id, &conversation), (server_id, conversation, id));
    }

    #[cfg(any(target_os = "linux", test))]
    fn take_id(&mut self, id: u32) -> Option<(ServerId, Conversation)> {
        let key = self
            .by_key
            .iter()
            .find_map(|(key, (_, _, shown))| (*shown == id).then(|| key.clone()))?;
        self.by_key
            .remove(&key)
            .map(|(server_id, conversation, _)| (server_id, conversation))
    }

    /// A notification action: the server and chat to open for a click ("default"), else `None`.
    #[cfg(any(target_os = "linux", test))]
    pub fn action(&mut self, id: u32, action_key: &str) -> Option<(ServerId, Conversation)> {
        if action_key == "default" {
            self.take_id(id)
        } else {
            None
        }
    }

    /// The notification went away (expired, dismissed or clicked).
    #[cfg(any(target_os = "linux", test))]
    pub fn closed(&mut self, id: u32) {
        self.take_id(id);
    }

    /// The chat was opened in the app: its notification is outdated.
    pub fn dismiss(&mut self, server_id: ServerId, conversation: &Conversation) -> Option<u32> {
        self.by_key
            .remove(&key(server_id, conversation))
            .map(|(_, _, id)| id)
    }

    /// Resync or disconnect of a server: its channel ids and sessions no longer apply.
    pub fn clear_server(&mut self, server_id: ServerId) -> Vec<u32> {
        let mut ids = Vec::new();
        self.by_key.retain(|_, (server, _, id)| {
            let stale = *server == server_id;
            if stale {
                ids.push(*id);
            }
            !stale
        });
        ids
    }
}

/// Body text for notification services that interpret a subset of HTML.
#[cfg(any(target_os = "linux", test))]
pub fn escape_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// App state: shown notifications and the platform connection.
#[derive(Default)]
pub struct Notifier {
    // Held across a show so two quick messages of one chat never create two notifications.
    groups: tokio::sync::Mutex<Groups>,
    #[cfg(target_os = "linux")]
    linux: linux::Service,
}

/// Registers the notifier; call once during setup.
pub fn init(app: &AppHandle) {
    app.manage(Notifier::default());
}

fn main_window_focused(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    window.is_visible().unwrap_or(false)
        && !window.is_minimized().unwrap_or(false)
        && window.is_focused().unwrap_or(false)
}

/// The server's "Mute notifications" switch (FR-2.10) silences everything, mentions and
/// sounds included.
/// A failed lookup does not mute, so a message is never lost to a storage error.
pub async fn server_muted(app: &AppHandle, server_id: ServerId) -> bool {
    match app.state::<Engine>().servers().await {
        Ok(servers) => muted_in(
            servers
                .iter()
                .map(|s| (to_api_server_id(s.id), s.notifications_muted)),
            server_id,
        ),
        Err(e) => {
            tracing::warn!(error = %e, "reading the server's mute setting failed");
            false
        }
    }
}

fn muted_in(mut servers: impl Iterator<Item = (u32, bool)>, server_id: u32) -> bool {
    servers.any(|(id, muted)| id == server_id && muted)
}

/// Decides for one new message and shows the system notification if that is the outcome.
/// A failing system notification falls back to the toast.
pub async fn notify(
    app: &AppHandle,
    mode: NotificationMode,
    notice: MessageNotice,
) -> NotifyOutcome {
    if server_muted(app, notice.server_id).await {
        return NotifyOutcome::None;
    }
    let outcome = decide(mode, main_window_focused(app), notice.on_screen);
    if outcome != NotifyOutcome::Desktop {
        return outcome;
    }
    let notifier = app.state::<Notifier>();
    let mut groups = notifier.groups.lock().await;
    match show(app, &notifier, &groups, &notice).await {
        Ok(Some(id)) => {
            groups.shown(notice.server_id, notice.conversation, id);
            NotifyOutcome::Desktop
        }
        Ok(None) => NotifyOutcome::Desktop,
        Err(e) => {
            tracing::warn!(error = %e, "showing a desktop notification failed; using the in-app toast");
            NotifyOutcome::Toast
        }
    }
}

/// Removes the notification of a chat the user just opened.
pub async fn dismiss(app: &AppHandle, server_id: ServerId, conversation: &Conversation) {
    let notifier = app.state::<Notifier>();
    let id = notifier
        .groups
        .lock()
        .await
        .dismiss(server_id, conversation);
    if let Some(id) = id {
        close(&notifier, vec![id]).await;
    }
}

/// Engine event hook: a (re)sync or a disconnect makes the shown notifications of that
/// server stale.
pub fn on_engine_event(app: &AppHandle, server_id: ServerId, event: &EngineEvent) {
    let stale = matches!(event, EngineEvent::ServerSynced(_))
        || matches!(
            event,
            EngineEvent::StateChanged(ConnState::Disconnected { .. })
        );
    if !stale {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let notifier = app.state::<Notifier>();
        let ids = notifier.groups.lock().await.clear_server(server_id);
        if !ids.is_empty() {
            close(&notifier, ids).await;
        }
    });
}

/// Shows or replaces the chat's notification; returns its id where the platform has one.
#[cfg(target_os = "linux")]
async fn show(
    app: &AppHandle,
    notifier: &Notifier,
    groups: &Groups,
    notice: &MessageNotice,
) -> Result<Option<u32>, String> {
    notifier
        .linux
        .show(
            app,
            groups.replaces(notice.server_id, &notice.conversation),
            notice,
        )
        .await
        .map(Some)
}

#[cfg(target_os = "linux")]
async fn close(notifier: &Notifier, ids: Vec<u32>) {
    notifier.linux.close(ids).await;
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn show(
    app: &AppHandle,
    _notifier: &Notifier,
    _groups: &Groups,
    notice: &MessageNotice,
) -> Result<Option<u32>, String> {
    let identifier = app.config().identifier.clone();
    let summary = notice.summary.clone();
    let body = notice.body.clone();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            // Without it macOS attributes the notification to another app. Settable only once.
            static APPLICATION: std::sync::Once = std::sync::Once::new();
            APPLICATION.call_once(|| {
                if let Err(e) = notify_rust::set_application(&identifier) {
                    tracing::warn!(error = %e, "setting the notification app failed");
                }
            });
        }
        let mut notification = notify_rust::Notification::new();
        notification
            .summary(&summary)
            .body(&body)
            .appname("Mumcord");
        #[cfg(target_os = "windows")]
        notification.app_id(&identifier);
        notification.show().map(|_| ()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(None)
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn close(_notifier: &Notifier, _ids: Vec<u32>) {}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
async fn show(
    _app: &AppHandle,
    _notifier: &Notifier,
    _groups: &Groups,
    _notice: &MessageNotice,
) -> Result<Option<u32>, String> {
    Err("desktop notifications are not supported on this platform".into())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
async fn close(_notifier: &Notifier, _ids: Vec<u32>) {}

/// A click on a notification: brings the window to the front and tells the UI which server and
/// chat to open.
#[cfg(target_os = "linux")]
fn activate(
    app: &AppHandle,
    server_id: ServerId,
    conversation: Conversation,
    activation_token: Option<String>,
) {
    use tauri_specta::Event;

    present(app, activation_token);
    if let Err(e) = (crate::commands::notifications::NotificationActivated {
        server_id,
        conversation,
    })
    .emit(app)
    {
        tracing::warn!(error = %e, "telling the UI about a notification click failed");
    }
}

/// On Wayland the compositor only raises the window with the activation token the
/// notification service handed out for the click; GTK passes it on with the startup id.
#[cfg(target_os = "linux")]
fn present(app: &AppHandle, activation_token: Option<String>) {
    let handle = app.clone();
    let result = app.run_on_main_thread(move || {
        use gtk::prelude::GtkWindowExt;

        if let (Some(token), Some(window)) = (activation_token, handle.get_webview_window("main")) {
            match window.gtk_window() {
                Ok(gtk_window) => gtk_window.set_startup_id(&token),
                Err(e) => tracing::warn!(error = %e, "passing the activation token failed"),
            }
        }
        crate::window::show(&handle);
    });
    if let Err(e) = result {
        tracing::warn!(error = %e, "bringing the window to the front failed");
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;

    use futures_util::StreamExt;
    use tauri::{AppHandle, Manager};
    use zbus::zvariant::Value;

    use super::{MessageNotice, Notifier, activate, escape_markup};

    #[zbus::proxy(
        interface = "org.freedesktop.Notifications",
        default_service = "org.freedesktop.Notifications",
        default_path = "/org/freedesktop/Notifications"
    )]
    trait Notifications {
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            app_name: &str,
            replaces_id: u32,
            app_icon: &str,
            summary: &str,
            body: &str,
            actions: &[&str],
            hints: HashMap<&str, Value<'_>>,
            expire_timeout: i32,
        ) -> zbus::Result<u32>;

        fn close_notification(&self, id: u32) -> zbus::Result<()>;

        #[zbus(signal)]
        fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

        /// Sent right before `ActionInvoked` by servers that support XDG activation.
        #[zbus(signal)]
        fn activation_token(&self, id: u32, activation_token: String) -> zbus::Result<()>;

        #[zbus(signal)]
        fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
    }

    /// Session bus connection, opened on the first notification.
    #[derive(Default)]
    pub struct Service {
        proxy: tokio::sync::OnceCell<NotificationsProxy<'static>>,
    }

    impl Service {
        async fn proxy(&self, app: &AppHandle) -> zbus::Result<&NotificationsProxy<'static>> {
            self.proxy
                .get_or_try_init(|| async {
                    let connection = zbus::Connection::session().await?;
                    let proxy = NotificationsProxy::new(&connection).await?;
                    listen(app.clone(), proxy.clone()).await?;
                    Ok(proxy)
                })
                .await
        }

        pub async fn show(
            &self,
            app: &AppHandle,
            replaces: u32,
            notice: &MessageNotice,
        ) -> Result<u32, String> {
            let proxy = self.proxy(app).await.map_err(|e| e.to_string())?;
            let identifier = app.config().identifier.as_str();
            let hints = HashMap::from([
                // Lets the desktop show Mumcord's name and icon from its desktop entry.
                ("desktop-entry", Value::from(identifier)),
                ("category", Value::from("im.received")),
            ]);
            proxy
                .notify(
                    "Mumcord",
                    replaces,
                    identifier,
                    &notice.summary,
                    &escape_markup(&notice.body),
                    &["default", &notice.action_label],
                    hints,
                    -1,
                )
                .await
                .map_err(|e| e.to_string())
        }

        pub async fn close(&self, ids: Vec<u32>) {
            let Some(proxy) = self.proxy.get() else {
                return;
            };
            for id in ids {
                if let Err(e) = proxy.close_notification(id).await {
                    tracing::debug!(error = %e, "closing a desktop notification failed");
                }
            }
        }
    }

    /// Follows clicks and closes of our notifications for the app's lifetime.
    async fn listen(app: AppHandle, proxy: NotificationsProxy<'static>) -> zbus::Result<()> {
        let mut actions = proxy.receive_action_invoked().await?;
        let mut tokens = proxy.receive_activation_token().await?;
        let mut closed = proxy.receive_notification_closed().await?;
        tauri::async_runtime::spawn(async move {
            let mut pending_tokens: HashMap<u32, String> = HashMap::new();
            loop {
                tokio::select! {
                    Some(signal) = tokens.next() => {
                        if let Ok(args) = signal.args() {
                            pending_tokens.insert(args.id, args.activation_token);
                        }
                    }
                    Some(signal) = actions.next() => {
                        let Ok(args) = signal.args() else { continue };
                        let token = pending_tokens.remove(&args.id);
                        let clicked = app
                            .state::<Notifier>()
                            .groups
                            .lock()
                            .await
                            .action(args.id, &args.action_key);
                        if let Some((server_id, conversation)) = clicked {
                            activate(&app, server_id, conversation, token);
                        }
                    }
                    Some(signal) = closed.next() => {
                        let Ok(args) = signal.args() else { continue };
                        pending_tokens.remove(&args.id);
                        app.state::<Notifier>().groups.lock().await.closed(args.id);
                    }
                    else => break,
                }
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: u32) -> Conversation {
        Conversation::Channel { channel_id: id }
    }

    #[test]
    fn the_chat_on_screen_never_notifies_while_focused() {
        for mode in [
            NotificationMode::InApp,
            NotificationMode::Desktop,
            NotificationMode::Both,
            NotificationMode::Off,
        ] {
            assert_eq!(decide(mode, true, true), NotifyOutcome::None, "{mode:?}");
        }
    }

    #[test]
    fn mode_matrix() {
        use NotificationMode::{Both, Desktop, InApp, Off};
        use NotifyOutcome as O;
        let cases = [
            (InApp, true, O::Toast),
            (InApp, false, O::None),
            (Desktop, true, O::Desktop),
            (Desktop, false, O::Desktop),
            (Both, true, O::Toast),
            (Both, false, O::Desktop),
            (Off, true, O::None),
            (Off, false, O::None),
        ];
        for (mode, focused, expected) in cases {
            assert_eq!(
                decide(mode, focused, false),
                expected,
                "{mode:?} focused={focused}"
            );
        }
    }

    #[test]
    fn selected_chat_notifies_when_the_window_is_in_the_background() {
        assert_eq!(
            decide(NotificationMode::Both, false, true),
            NotifyOutcome::Desktop
        );
    }

    #[test]
    fn one_notification_per_chat_is_replaced() {
        let mut groups = Groups::default();
        assert_eq!(groups.replaces(1, &channel(1)), 0);
        groups.shown(1, channel(1), 7);
        groups.shown(1, channel(2), 8);
        assert_eq!(groups.replaces(1, &channel(1)), 7);
        assert_eq!(groups.replaces(1, &channel(2)), 8);
        let private = Conversation::Private {
            cert_hash: "1".into(),
        };
        assert_eq!(
            groups.replaces(1, &private),
            0,
            "keys never collide across kinds"
        );
    }

    #[test]
    fn click_opens_the_chat_once() {
        let mut groups = Groups::default();
        groups.shown(1, channel(1), 7);
        assert_eq!(groups.action(7, "other"), None);
        assert_eq!(groups.action(7, "default"), Some((1, channel(1))));
        assert_eq!(groups.action(7, "default"), None);
        assert_eq!(
            groups.replaces(1, &channel(1)),
            0,
            "next message starts a new notification"
        );
        assert_eq!(groups.action(99, "default"), None, "not ours");
    }

    #[test]
    fn close_dismiss_and_clear_forget_notifications() {
        let mut groups = Groups::default();
        groups.shown(1, channel(1), 7);
        groups.shown(1, channel(2), 8);
        groups.shown(1, channel(3), 9);
        groups.closed(7);
        assert_eq!(groups.replaces(1, &channel(1)), 0);
        assert_eq!(groups.dismiss(1, &channel(2)), Some(8));
        assert_eq!(groups.dismiss(1, &channel(2)), None);
        assert_eq!(groups.clear_server(1), vec![9]);
        assert_eq!(groups.replaces(1, &channel(3)), 0);
    }

    #[test]
    fn markup_is_escaped() {
        assert_eq!(
            escape_markup(r#"<b>a</b> & "c" 'd'"#),
            "&lt;b&gt;a&lt;/b&gt; &amp; &quot;c&quot; &#39;d&#39;"
        );
    }

    #[test]
    fn same_chat_of_two_servers_keeps_two_notifications() {
        let mut groups = Groups::default();
        groups.shown(1, channel(1), 7);
        groups.shown(2, channel(1), 8);
        assert_eq!(groups.replaces(1, &channel(1)), 7);
        assert_eq!(groups.replaces(2, &channel(1)), 8);
        assert_eq!(groups.action(8, "default"), Some((2, channel(1))));
    }

    #[test]
    fn clearing_a_server_keeps_the_others() {
        let mut groups = Groups::default();
        groups.shown(1, channel(1), 7);
        groups.shown(2, channel(1), 8);
        assert_eq!(groups.clear_server(1), vec![7]);
        assert_eq!(groups.replaces(2, &channel(1)), 8);
    }

    #[test]
    fn only_a_muted_server_is_silenced() {
        let servers = [(1, true), (2, false)];
        assert!(muted_in(servers.into_iter(), 1));
        assert!(!muted_in(servers.into_iter(), 2));
        assert!(!muted_in(servers.into_iter(), 3));
    }
}
