/**
 * Chat column (FR-1.28–FR-1.30): header, message list and input for the selected conversation
 * (channel or private chat). B22 adds the Follow toggle through `renderHeaderActions`. Sender
 * names open B20's profile card; focus returns to the name when it closes. Channel chats pin the
 * description and welcome text at the top (FR-1.48b).
 */
import { useCallback, useContext, useEffect, useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type ChatMessage, type Conversation, type User } from "../../api";
import { PreferencesContext } from "../../app/preferences";
import { useSelectedConversation, useServerState } from "../channels";
import { useProfileCard } from "../members";
import { useAvatars } from "../members/useAvatars";
import {
  conversationKey,
  ensureLoaded,
  loadOlder,
  subscribeIncoming,
  useConversationLog,
} from "./chatStore";
import { Composer } from "./Composer";
import { conversationInfo } from "./conversationInfo";
import { messageLimits } from "./images";
import { createMessageAnnouncer } from "./messageAnnouncer";
import { MessageList } from "./MessageList";
import { PinnedInfoBox } from "./PinnedInfo";
import { pinnedInfo } from "./pinnedInfoModel";
import { hasImage, htmlToText } from "./sanitize";
import "./chat.css";

/** Longest message text read out by the screen reader. */
const ANNOUNCE_CHARS = 200;

export interface ChatViewProps {
  /** Extra header controls for the conversation, e.g. the Follow toggle (B22). */
  renderHeaderActions?: (conversation: Conversation) => ReactNode;
}

export function ChatView({ renderHeaderActions }: ChatViewProps) {
  const { t } = useTranslation("chat");
  const { conversation } = useSelectedConversation();
  if (conversation === null) {
    return (
      <div className="chat">
        <p className="chat__none">{t("noConversation")}</p>
      </div>
    );
  }
  return (
    <ConversationChat
      key={conversationKey(conversation)}
      conversation={conversation}
      renderHeaderActions={renderHeaderActions}
    />
  );
}

interface ConversationChatProps {
  conversation: Conversation;
  renderHeaderActions: ChatViewProps["renderHeaderActions"];
}

function ConversationChat({ conversation, renderHeaderActions }: ConversationChatProps) {
  const { t } = useTranslation("chat");
  const server = useServerState();
  const log = useConversationLog(conversation);
  const info = conversationInfo(server, conversation, log.messages);
  const limits = useMemo(() => messageLimits(server.info), [server.info]);
  const { openProfile, profileCard } = useProfileCard();
  const pinned = pinnedInfo(server, conversation);
  const pinnedChannelId = conversation.kind === "channel" ? conversation.channelId : null;
  // Outside the app shell (isolated feature tests) there are no preferences: the default applies.
  const display = useContext(PreferencesContext)?.chatDisplay ?? "cozy";
  const { users, selfSession } = server;

  // The person behind a message while they are still connected. Sessions are reused after
  // someone leaves, so a different name means the sender is gone (initials fallback).
  const senderOf = useCallback(
    (message: ChatMessage): User | undefined => {
      const session = message.own ? selfSession : message.senderSession;
      const user = session === null ? undefined : users.get(session);
      return user && (message.own || user.name === message.senderName) ? user : undefined;
    },
    [users, selfSession],
  );
  const senders = useMemo(() => {
    if (display !== "cozy") return [];
    const found = new Map<number, User>();
    for (const message of log.messages) {
      const user = senderOf(message);
      if (user) found.set(user.session, user);
    }
    return [...found.values()];
  }, [display, log.messages, senderOf]);
  const avatarOf = useAvatars(senders);
  const avatarFor = useCallback(
    (message: ChatMessage) => {
      const user = senderOf(message);
      return user ? avatarOf(user) : null;
    },
    [senderOf, avatarOf],
  );

  const name = info.name ?? t("header.unknownPerson");
  const title = info.isChannel ? t("header.channel", { name }) : name;

  useEffect(() => {
    if (!log.loaded && !log.loading) ensureLoaded(conversation);
  }, [conversation, log.loaded, log.loading]);

  // Long descriptions are only announced by hash: fetch the text once the chat is open. The
  // engine sends each request only once per description; the text arrives as a channel update.
  useEffect(() => {
    if (!pinned.needsFetch || pinnedChannelId === null) return;
    getApi()
      .channelDescriptionRequest(pinnedChannelId)
      .catch((error: unknown) => {
        console.error("Fetching the channel description failed", error);
      });
  }, [pinned.needsFetch, pinnedChannelId]);

  useEffect(() => {
    const key = conversationKey(conversation);
    const announcer = createMessageAnnouncer(
      (text) => {
        announce(text, "polite");
      },
      (count) => t("announce.more", { count }),
    );
    const unsubscribe = subscribeIncoming((message) => {
      if (message.own || conversationKey(message.conversation) !== key) return;
      const text = htmlToText(message.body).slice(0, ANNOUNCE_CHARS);
      announcer.push(
        text === "" && hasImage(message.body)
          ? t("announce.image", { name: message.senderName })
          : t("announce.message", { name: message.senderName, text }),
      );
    });
    return () => {
      unsubscribe();
      announcer.dispose();
    };
  }, [conversation, t]);

  let blockedReason: string | null = null;
  if (!info.mayWrite) blockedReason = t("composer.noPermission");
  else if (info.target === null) {
    blockedReason = info.isChannel ? t("composer.unavailable") : t("composer.offline", { name });
  }

  return (
    <div className="chat">
      <header className="chat-header">
        <h2 className="chat-header__title">
          {info.isChannel ? (
            <span className="chat-header__hash" aria-hidden="true">
              #
            </span>
          ) : null}
          <span className="visually-hidden">
            {info.isChannel ? t("header.channelPrefix") : t("header.privatePrefix")}
          </span>
          {name}
        </h2>
        {renderHeaderActions ? (
          <div className="chat-header__actions">{renderHeaderActions(conversation)}</div>
        ) : null}
      </header>
      <PinnedInfoBox blocks={pinned.blocks} />
      <MessageList
        log={log}
        label={t("list.label", { name: title })}
        onLoadOlder={() => {
          void loadOlder(conversation);
        }}
        onOpenProfile={openProfile}
        display={display}
        avatarFor={avatarFor}
      />
      <Composer
        target={info.target}
        limits={limits}
        label={t("composer.label", { name: title })}
        blockedReason={blockedReason}
      />
      {profileCard}
    </div>
  );
}
