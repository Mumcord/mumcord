import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import type { Conversation, User } from "../../api";
import {
  getSelectedConversation,
  getServerState,
  sameConversation,
  selectConversation,
  useSelectedConversation,
  useServerState,
} from "../channels";
import { subscribeIncoming } from "../chat";
import { htmlToText } from "../chat/sanitize";
import { createMessageAnnouncer } from "../chat/messageAnnouncer";
import { getFollowState, rememberDirectChat, useFollowState, type DirectChat } from "./followStore";
import { UnreadBadge } from "./UnreadBadge";
import "./follow.css";

const SNIPPET_CHARS = 100;

function personName(users: ReadonlyMap<number, User>, c: Conversation): string | null {
  if (c.kind === "private") {
    return [...users.values()].find((u) => u.certHash === c.certHash)?.name ?? null;
  }
  if (c.kind === "privateSession") return users.get(c.session)?.name ?? null;
  return null;
}

/** Open private chats ("Direct messages") with unread badges, plus new-message notifications. */
export function DirectMessages() {
  const { t } = useTranslation("follow");
  const follow = useFollowState();
  const server = useServerState();
  const { conversation } = useSelectedConversation();

  function nameOf(chat: DirectChat): string {
    return personName(server.users, chat.conversation) ?? chat.name ?? t("dm.unknown");
  }

  // Opening a private chat from anywhere adds it to the list.
  useEffect(() => {
    if (conversation && conversation.kind !== "channel") {
      rememberDirectChat(conversation, personName(server.users, conversation));
    }
  }, [conversation, server.users]);

  // Polite, throttled notification of messages in chats that are not open.
  useEffect(() => {
    const announcer = createMessageAnnouncer(
      (text) => {
        announce(text, "polite");
      },
      (count) => t("notify.more", { count }),
    );
    const unsubscribe = subscribeIncoming((message) => {
      if (message.own) return;
      if (sameConversation(getSelectedConversation(), message.conversation)) return;
      const c: Conversation = message.conversation;
      const text = htmlToText(message.body).slice(0, SNIPPET_CHARS);
      if (c.kind === "channel") {
        if (!getFollowState().followed.includes(c.channelId)) return;
        const channel = getServerState().channels.get(c.channelId)?.name ?? "";
        announcer.push(t("notify.channel", { name: message.senderName, channel, text }));
      } else {
        announcer.push(t("notify.private", { name: message.senderName, text }));
      }
    });
    return () => {
      unsubscribe();
      announcer.dispose();
    };
  }, [t]);

  if (follow.chats.length === 0) return null;
  return (
    <section className="direct-messages" aria-labelledby="dm-title">
      <h2 id="dm-title">{t("dm.title")}</h2>
      <ul>
        {follow.chats.map((chat) => (
          <li key={chat.key}>
            <button
              type="button"
              className="direct-messages__item"
              aria-current={sameConversation(conversation, chat.conversation)}
              onClick={() => {
                selectConversation(chat.conversation);
              }}
            >
              <span>{nameOf(chat)}</span>
              <UnreadBadge count={follow.unread.get(chat.key) ?? 0} mentionKey={chat.key} />
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
