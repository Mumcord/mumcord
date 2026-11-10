import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type Conversation } from "../../api";
import {
  selectConversation,
  showServer,
  useSelectedConversation,
  useShownServerId,
} from "../channels";
import { conversationKey, subscribeIncoming } from "../chat";
import { isConversationMuted } from "../messages/conversationsStore";
import { useServers } from "../servers";
import { markConversationRead } from "./centreStore";
import { handleIncoming } from "./incoming";
import { handleActivity } from "../activity/activityNotify";
import { subscribeActivityAdded } from "../activity/activityStore";
import { bindRelayTrustNotices } from "../p2p/relayTrust";
import { bindDeniedNotices } from "../admin/deniedNotice";
import { bindSuppressionNotices } from "./suppressionNotice";
import { RelayTrustDialog } from "../p2p/RelayTrustDialog";
import { NotificationCentre } from "./NotificationCentre";
import { clearServerNotifications, markSeen, noticeKey } from "./notificationStore";
import { Toasts } from "./Toasts";
import "./notifications.css";

export interface NotificationsHostProps {
  /** A full-window overlay (settings) covers the chat. */
  overlayOpen: boolean;
  /** Called before a notification opens a chat, e.g. to close the settings overlay. */
  onOpenChat: () => void;
}

function dismissSystemNotification(serverId: number, conversation: Conversation): void {
  getApi()
    .notificationDismiss(serverId, conversation)
    .catch((error: unknown) => {
      console.error("Dismissing a desktop notification failed", error);
    });
}

/** The chat is on screen: its toast, count and notification centre entries are done. */
function seen(serverId: number, conversation: Conversation): void {
  markSeen(noticeKey(serverId, conversationKey(conversation)));
  markConversationRead(serverId, conversation);
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
  const { t: tActivity } = useTranslation("activity");
  const { t: tP2p } = useTranslation("p2p");
  const { t: tAdmin } = useTranslation("admin");
  const { t: tSettings } = useTranslation("settings");
  const { conversation: selected } = useSelectedConversation();
  const shown = useShownServerId();
  const { servers } = useServers();
  const serverName = useCallback(
    (id: number) => {
      const server = servers.find((s) => s.id === id);
      return server ? server.label || server.address : undefined;
    },
    [servers],
  );
  const muted = useCallback(
    (id: number, conversation: Conversation) =>
      (servers.find((s) => s.id === id)?.notificationsMuted ?? false) ||
      isConversationMuted(id, conversation),
    [servers],
  );
  const live = useRef({
    t,
    tActivity,
    tP2p,
    tAdmin,
    tSettings,
    overlayOpen,
    onOpenChat,
    selected,
    shown,
    serverName,
    muted,
  });
  useEffect(() => {
    live.current = {
      t,
      tActivity,
      tP2p,
      tAdmin,
      tSettings,
      overlayOpen,
      onOpenChat,
      selected,
      shown,
      serverName,
      muted,
    };
  });

  // Shows the chat's server first (FR-2.10); the selection is remembered per server.
  const open = useCallback((serverId: number, conversation: Conversation) => {
    live.current.onOpenChat();
    showServer(serverId);
    selectConversation(conversation);
    seen(serverId, conversation);
    dismissSystemNotification(serverId, conversation);
    focusChat();
  }, []);

  useEffect(() => {
    function seeSelected(): void {
      const { selected: current, overlayOpen: covered, shown: server } = live.current;
      if (current === null || covered || server === null) return;
      seen(server, current);
      dismissSystemNotification(server, current);
    }
    const unsubscribers = [
      subscribeIncoming((message, serverId) => {
        void handleIncoming(message, serverId, {
          t: live.current.t,
          overlayOpen: live.current.overlayOpen,
          serverName: (id) => live.current.serverName(id),
          muted: (id, conversation) => live.current.muted(id, conversation),
        });
      }),
      // Activity events notify like messages (#597).
      subscribeActivityAdded((event) => {
        void handleActivity(event, {
          t: live.current.tActivity,
          openLabel: live.current.t("open"),
          serverName: (id) => live.current.serverName(id),
        });
      }),
      // The server's refusals in plain words, unless a dialog shows them (#248).
      bindDeniedNotices(getApi(), () => ({
        t: live.current.tAdmin,
        serverName: (id) => live.current.serverName(id),
      })),
      // Noise suppression that is too slow for this computer says so (#753).
      bindSuppressionNotices(getApi(), () => ({
        t: live.current.t,
        tSettings: live.current.tSettings,
      })),
      // A changed relay waits for the user's decision (#600).
      bindRelayTrustNotices(getApi(), () => ({
        t: live.current.tP2p,
        serverName: (id) => live.current.serverName(id),
      })),
      getApi().on("notificationActivated", ({ serverId, conversation }) => {
        open(serverId, conversation);
      }),
      getApi().on("serverSynced", ({ snapshot }) => {
        clearServerNotifications(snapshot.serverId);
      }),
      getApi().on("connectionStateChanged", ({ serverId, state }) => {
        if (state.state === "disconnected") clearServerNotifications(serverId);
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
    if (selected === null || overlayOpen || shown === null) return;
    seen(shown, selected);
    dismissSystemNotification(shown, selected);
  }, [selected, overlayOpen, shown]);

  return (
    <>
      <Toasts onOpen={open} />
      <NotificationCentre overlayOpen={overlayOpen} />
      <RelayTrustDialog />
    </>
  );
}
