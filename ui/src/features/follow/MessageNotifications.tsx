import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import type { Conversation } from "../../api";
import {
  getSelectedConversation,
  getServerStateOf,
  getShownServerId,
  sameConversation,
  useSelectedConversation,
  useServerState,
} from "../channels";
import { subscribeIncoming } from "../chat";
import { htmlToText } from "../chat/sanitize";
import { createMessageAnnouncer } from "../chat/messageAnnouncer";
import { isConversationMuted } from "../messages/conversationsStore";
import { useServers as useSavedServers } from "../servers";
import { personName } from "./directEntries";
import { getFollowState, rememberDirectChat } from "./followStore";

const SNIPPET_CHARS = 100;

/**
 * Screen-reader notifications for new messages in chats that are not open, and remembering opened
 * private chats for the private messages overview (#173). Renders nothing; mount once.
 */
export function MessageNotifications() {
  const { t } = useTranslation("follow");
  const { servers } = useSavedServers();
  const { conversation } = useSelectedConversation();
  const live = useRef({ servers });
  useEffect(() => {
    live.current = { servers };
  });

  // Opening a private chat from anywhere adds it to the list.
  const selectedUsers = useServerState().users;
  useEffect(() => {
    if (conversation && conversation.kind !== "channel") {
      rememberDirectChat(conversation, personName(selectedUsers, conversation));
    }
  }, [conversation, selectedUsers]);

  // Polite, throttled notification of messages in chats that are not open.
  useEffect(() => {
    const announcer = createMessageAnnouncer(
      (text) => {
        announce(text, "polite");
      },
      (count) => t("notify.more", { count }),
    );
    const unsubscribe = subscribeIncoming((message, serverId) => {
      if (message.own) return;
      const saved = live.current.servers.find((s) => s.id === serverId);
      if (saved?.notificationsMuted || isConversationMuted(serverId, message.conversation)) return;
      const onShown = serverId === getShownServerId();
      if (onShown && sameConversation(getSelectedConversation(), message.conversation)) return;
      const server = saved ? saved.label || saved.address : "";
      const c: Conversation = message.conversation;
      const text = htmlToText(message.body).slice(0, SNIPPET_CHARS);
      if (c.kind === "channel") {
        if (!getFollowState(serverId).followed.includes(c.channelId)) return;
        const channel = getServerStateOf(serverId).channels.get(c.channelId)?.name ?? "";
        const key = onShown ? "notify.channel" : "notify.channelOn";
        announcer.push(t(key, { name: message.senderName, channel, text, server }));
      } else {
        const key = onShown ? "notify.private" : "notify.privateOn";
        announcer.push(t(key, { name: message.senderName, text, server }));
      }
    });
    return () => {
      unsubscribe();
      announcer.dispose();
    };
  }, [t]);

  return null;
}
