import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import type { Conversation, User } from "../../api";
import {
  getSelectedConversation,
  getServerStateOf,
  getShownServerId,
  sameConversation,
  selectConversation,
  showServer,
  useSelectedConversation,
  useServers as useConnectedServers,
  useServerState,
  useShownServerId,
} from "../channels";
import { subscribeIncoming } from "../chat";
import { htmlToText } from "../chat/sanitize";
import { createMessageAnnouncer } from "../chat/messageAnnouncer";
import { PrivatePresence, useFriends } from "../friends";
import { previewOnFocus, releasePreview, requestPreview } from "../members/profilePreviewStore";
import { isConversationMuted } from "../messages/conversationsStore";
import { useServers as useSavedServers } from "../servers";
import { collectEntries, person, personName, type Entry } from "./directEntries";
import { getFollowState, rememberDirectChat, useFollowStates } from "./followStore";
import { UnreadBadge } from "./UnreadBadge";
import "./follow.css";

const SNIPPET_CHARS = 100;

/**
 * Open private chats ("Direct messages") with unread badges, plus new-message notifications.
 * `listHidden` keeps only the notifications, while the private messages overview shows the chats.
 */
export function DirectMessages({ listHidden = false }: { listHidden?: boolean }) {
  const { t } = useTranslation("follow");
  const states = useFollowStates();
  const connected = useConnectedServers();
  const { servers } = useSavedServers();
  const shown = useShownServerId();
  const { conversation } = useSelectedConversation();
  const friends = useFriends();
  const live = useRef({ servers });
  useEffect(() => {
    live.current = { servers };
  });

  const serverName = (id: number): string => {
    const server = servers.find((s) => s.id === id);
    return server ? server.label || server.address : "";
  };

  function nameOf(entry: Entry): string {
    const c = entry.chat.conversation;
    const users = connected.get(entry.serverId)?.users ?? new Map<number, User>();
    const friend = c.kind === "private" ? friends.find((f) => f.certHash === c.certHash) : null;
    return personName(users, c) ?? entry.chat.name ?? friend?.name ?? t("dm.unknown");
  }

  // A person who is not online on their server gets the limited preview from the stored name,
  // like in the chat history.
  function previewTarget(entry: Entry): { session: number | null; name: string } {
    const users = connected.get(entry.serverId)?.users ?? new Map<number, User>();
    const session = person(users, entry.chat.conversation)?.session ?? null;
    return { session, name: nameOf(entry) };
  }

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

  const entries = collectEntries(
    states,
    servers.map((s) => s.id),
    friends,
  );
  if (listHidden || entries.length === 0) return null;
  return (
    <section className="direct-messages" aria-labelledby="dm-title">
      <h2 id="dm-title">{t("dm.title")}</h2>
      <ul>
        {entries.map((entry) => (
          <li key={`${String(entry.serverId)}/${entry.chat.key}`}>
            <button
              type="button"
              className="direct-messages__item"
              aria-label={
                serverName(entry.serverId) === ""
                  ? undefined
                  : t("dm.withServer", { name: nameOf(entry), server: serverName(entry.serverId) })
              }
              aria-current={
                entry.serverId === shown && sameConversation(conversation, entry.chat.conversation)
              }
              onPointerEnter={(event) => {
                const target = previewTarget(entry);
                if (event.pointerType !== "touch") {
                  requestPreview(
                    target.session,
                    event.currentTarget,
                    undefined,
                    target.name,
                    entry.serverId,
                  );
                }
              }}
              onPointerLeave={(event) => {
                if (event.pointerType !== "touch") releasePreview();
              }}
              onFocus={(event) => {
                const target = previewTarget(entry);
                previewOnFocus(
                  target.session,
                  event.currentTarget,
                  undefined,
                  target.name,
                  entry.serverId,
                );
              }}
              onBlur={releasePreview}
              onClick={() => {
                showServer(entry.serverId);
                selectConversation(entry.chat.conversation);
              }}
            >
              <PrivatePresence
                className="direct-messages__presence"
                compact
                online={
                  connected.get(entry.serverId) !== undefined &&
                  person(
                    connected.get(entry.serverId)?.users ?? new Map(),
                    entry.chat.conversation,
                  ) !== undefined
                }
                certHash={
                  entry.chat.conversation.kind === "private"
                    ? entry.chat.conversation.certHash
                    : null
                }
              />
              <span className="direct-messages__name">{nameOf(entry)}</span>
              {serverName(entry.serverId) !== "" && (
                <span className="direct-messages__server" data-tooltip={serverName(entry.serverId)}>
                  {serverName(entry.serverId)}
                </span>
              )}
              <UnreadBadge
                count={entry.unread}
                mentionKey={entry.chat.key}
                serverId={entry.serverId}
              />
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
