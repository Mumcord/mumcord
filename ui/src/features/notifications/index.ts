/**
 * Message notifications (FR-1.48c, QA.16): in-app toasts and system notifications for new
 * messages in chats that are not on screen. Mount `<NotificationsHost>` once in the main window.
 */
export { NotificationsHost, type NotificationsHostProps } from "./NotificationsHost";
export { resetNotifications, useHasMention } from "./notificationStore";
