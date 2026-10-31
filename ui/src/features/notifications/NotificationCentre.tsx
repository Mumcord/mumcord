import { useEffect, useRef } from "react";
import { getApi, type Conversation } from "../../api";
import { conversationKey } from "../chat";
import { useFollowStates } from "../follow/followStore";
import { CentrePanel } from "./CentrePanel";
import { isCentreShortcut } from "./centreShortcut";
import { bindCentreSources } from "./centreSources";
import { markConversationRead, setCentreOpen, toggleCentre, useCentreOpen } from "./centreStore";
import "./notificationCentre.css";

/**
 * A chat whose unread count dropped to zero was read elsewhere, e.g. "Mark as read" in the
 * private messages overview: its entries in the centre are read too (FR-3.110).
 */
function useReadSync(): void {
  const follow = useFollowStates();
  const previous = useRef(follow);
  useEffect(() => {
    for (const [serverId, before] of previous.current) {
      const after = follow.get(serverId);
      // A server that went away was not read; its entries stay as they are.
      if (!after) continue;
      const chats: Conversation[] = [
        ...before.followed.map((channelId) => ({ kind: "channel" as const, channelId })),
        ...before.chats.map((chat) => chat.conversation),
      ];
      for (const conversation of chats) {
        const key = conversationKey(conversation);
        if ((before.unread.get(key) ?? 0) > 0 && (after.unread.get(key) ?? 0) === 0) {
          markConversationRead(serverId, conversation);
        }
      }
    }
    previous.current = follow;
  }, [follow]);
}

/**
 * The notification centre (#143): collects entries while the app runs, opens with the bell or
 * Ctrl+Shift+N (Cmd+Shift+N on macOS) and closes while a full-window overlay covers the app.
 */
export function NotificationCentre({ overlayOpen }: { overlayOpen: boolean }) {
  const open = useCentreOpen();
  useEffect(() => bindCentreSources(getApi()), []);
  useReadSync();

  useEffect(() => {
    if (overlayOpen) {
      setCentreOpen(false);
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || !isCentreShortcut(event)) return;
      event.preventDefault();
      toggleCentre();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [overlayOpen]);

  return open && !overlayOpen ? <CentrePanel /> : null;
}
