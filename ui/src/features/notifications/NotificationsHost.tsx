import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type Conversation } from "../../api";
import { selectConversation, subscribeServerChanges, useSelectedConversation } from "../channels";
import { conversationKey, subscribeIncoming } from "../chat";
import { handleIncoming } from "./incoming";
import { clearNotifications, markSeen } from "./notificationStore";
import { Toasts } from "./Toasts";
import "./notifications.css";

export interface NotificationsHostProps {
  /** A full-window overlay (settings) covers the chat. */
  overlayOpen: boolean;
  /** Called before a notification opens a chat, e.g. to close the settings overlay. */
  onOpenChat: () => void;
}

function dismissSystemNotification(conversation: Conversation): void {
  getApi()
    .notificationDismiss(conversation)
    .catch((error: unknown) => {
      console.error("Dismissing a desktop notification failed", error);
    });
}

/** Moves keyboard focus to the chat once it has rendered: the composer, else the chat region. */
function focusChat(): void {
  requestAnimationFrame(() => {
    const target =
      document.querySelector<HTMLElement>("#chat textarea") ??
      document.querySelector<HTMLElement>("#chat");
    target?.focus();
  });
}

/**
 * Message notifications (FR-1.48c): listens for incoming messages, shows toasts bottom right and
 * opens the chat when a toast or system notification is clicked.
 */
export function NotificationsHost({ overlayOpen, onOpenChat }: NotificationsHostProps) {
  const { t } = useTranslation("notifications");
  const { conversation: selected } = useSelectedConversation();
  const live = useRef({ t, overlayOpen, onOpenChat, selected });
  useEffect(() => {
    live.current = { t, overlayOpen, onOpenChat, selected };
  });

  const open = useCallback((conversation: Conversation) => {
    live.current.onOpenChat();
    selectConversation(conversation);
    markSeen(conversationKey(conversation));
    dismissSystemNotification(conversation);
    focusChat();
  }, []);

  useEffect(() => {
    function seeSelected(): void {
      const { selected: current, overlayOpen: covered } = live.current;
      if (current === null || covered) return;
      markSeen(conversationKey(current));
      dismissSystemNotification(current);
    }
    const unsubscribers = [
      subscribeIncoming((message) => {
        void handleIncoming(message, live.current);
      }),
      getApi().on("notificationActivated", ({ conversation }) => {
        open(conversation);
      }),
      subscribeServerChanges((change, state) => {
        if (change.kind === "synced" || state.serverId === null) clearNotifications();
      }),
    ];
    // Coming back to the window shows the selected chat again.
    window.addEventListener("focus", seeSelected);
    return () => {
      for (const unsubscribe of unsubscribers) unsubscribe();
      window.removeEventListener("focus", seeSelected);
    };
  }, [open]);

  useEffect(() => {
    if (selected === null || overlayOpen) return;
    markSeen(conversationKey(selected));
    dismissSystemNotification(selected);
  }, [selected, overlayOpen]);

  return <Toasts onOpen={open} />;
}
