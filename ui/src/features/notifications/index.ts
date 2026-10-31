/**
 * Message notifications (FR-1.48c, QA.16): in-app toasts and system notifications for new
 * messages in chats that are not on screen. Mount `<NotificationsHost>` once in the main window;
 * it also hosts the notification centre (#143), opened by `<NotificationBell>` in the chat header.
 */
export { NotificationsHost, type NotificationsHostProps } from "./NotificationsHost";
export { resetNotifications, useHasMention } from "./notificationStore";
export { NotificationBell } from "./NotificationBell";
export { resetCentre } from "./centreStore";
