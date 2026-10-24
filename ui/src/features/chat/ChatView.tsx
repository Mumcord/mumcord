/**
 * Chat column (FR-1.28–FR-1.30): header, message list and input for the selected conversation
 * (channel or private chat). B22 adds the Follow toggle through `renderHeaderActions`. Sender
 * names open B20's profile card; focus returns to the name when it closes.
 */
import { useEffect, useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import type { Conversation } from "../../api";
import { useSelectedConversation, useServerState } from "../channels";
import { useProfileCard } from "../members";
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

  const name = info.name ?? t("header.unknownPerson");
  const title = info.isChannel ? t("header.channel", { name }) : name;

  useEffect(() => {
    if (!log.loaded && !log.loading) ensureLoaded(conversation);
  }, [conversation, log.loaded, log.loading]);

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
      <MessageList
        log={log}
        label={t("list.label", { name: title })}
        onLoadOlder={() => {
          void loadOlder(conversation);
        }}
        onOpenProfile={openProfile}
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
