/**
 * Chat column (FR-1.28–FR-1.30): header, message list and input for the selected conversation
 * (channel or private chat). B22 adds the Follow toggle through `renderHeaderActions`. Sender
 * names open B20's profile card; focus returns to the name when it closes. Channel chats pin the
 * description and welcome text at the top (FR-1.48b). Replying to a message (FR-2.60) connects
 * the list and the composer.
 */
import { useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type ChatMessage, type Conversation, type User } from "../../api";
import { getShownServerId, shownServerId } from "../channels/serverStore";
import { PreferencesContext } from "../../app/preferences";
import { useSelectedConversation, useServerState } from "../channels";
import { PrivatePresence, useFriend } from "../friends";
import { useAvatars } from "../members/useAvatars";
import { IdentityBadge } from "../servers/IdentityBadge";
import { ActivityBanners } from "./ActivityBanners";
import {
  conversationKey,
  ensureLoaded,
  loadOlder,
  requestSync,
  subscribeIncoming,
  useConversationLog,
} from "./chatStore";
import { Composer } from "./Composer";
import { conversationInfo } from "./conversationInfo";
import { messageLimits } from "./images";
import { createMessageAnnouncer } from "./messageAnnouncer";
import { MessageList } from "./MessageList";
import { useBlockedView } from "./blockedView";
import { PinnedInfoBox, type PinnedBlock } from "./PinnedInfo";
import { pinnedInfo } from "./pinnedInfoModel";
import { descriptionKey, hideInfo, useHiddenHash, welcomeKey } from "./hiddenInfo";
import type { ReplyQuote } from "./reply";
import { hasImage, htmlToText } from "./sanitize";
import "./chat.css";

/** Longest message text read out by the screen reader. */
const ANNOUNCE_CHARS = 200;

export interface ChatViewProps {
  /** Extra header controls for the conversation, e.g. the Follow toggle (B22). */
  renderHeaderActions?: (conversation: Conversation) => ReactNode;
  /** Replaces the plain title of a channel chat, e.g. with the channel-name menu (#170). */
  renderChannelTitle?: (channelId: number) => ReactNode;
  /** A media view between the header and the messages (the cinema views of issue #163). */
  belowHeader?: ReactNode;
}

export function ChatView({ renderHeaderActions, renderChannelTitle, belowHeader }: ChatViewProps) {
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
      renderChannelTitle={renderChannelTitle}
      belowHeader={belowHeader}
    />
  );
}

interface ConversationChatProps {
  conversation: Conversation;
  renderHeaderActions: ChatViewProps["renderHeaderActions"];
  renderChannelTitle: ChatViewProps["renderChannelTitle"];
  belowHeader: ChatViewProps["belowHeader"];
}

function ConversationChat({
  conversation,
  renderHeaderActions,
  renderChannelTitle,
  belowHeader,
}: ConversationChatProps) {
  const { t } = useTranslation("chat");
  const server = useServerState();
  const fullLog = useConversationLog(conversation);
  const serverId = shownServerId();
  const blocked = useBlockedView(fullLog, serverId, conversation);
  const log = blocked.log;
  const info = conversationInfo(server, conversation, fullLog.messages);
  const limits = useMemo(() => messageLimits(server.info), [server.info]);
  const pinnedChannelId = conversation.kind === "channel" ? conversation.channelId : null;
  const channelKey = descriptionKey(serverId, pinnedChannelId ?? -1);
  const hiddenWelcome = useHiddenHash(welcomeKey(serverId));
  const hiddenDescription = useHiddenHash(channelKey);
  const pinned = pinnedInfo(server, conversation, {
    welcome: hiddenWelcome,
    description: hiddenDescription,
  });
  const pinnedBlocks: PinnedBlock[] = [];
  const { description, welcome } = pinned;
  if (description !== null) {
    pinnedBlocks.push({
      html: description,
      hideLabel: t("pinned.hideDescription"),
      onHide: () => {
        hideInfo(channelKey, description);
      },
    });
  }
  if (welcome !== null) {
    pinnedBlocks.push({
      html: welcome,
      hideLabel: t("pinned.hideWelcome"),
      onHide: () => {
        hideInfo(welcomeKey(serverId), welcome);
      },
    });
  }
  // Outside the app shell (isolated feature tests) there are no preferences: the default applies.
  const preferences = useContext(PreferencesContext);
  const display = preferences?.chatDisplay ?? "cozy";
  const linkPreviews = preferences?.linkPreviews ?? false;
  const { users, selfSession } = server;
  // The message being replied to (FR-2.60); a new conversation starts without one.
  const [replyTo, setReplyTo] = useState<ReplyQuote | null>(null);

  // The person behind a message while they are connected. Sessions change on every reconnect and
  // history entries have none, so the certificate decides first; a message without one falls back
  // to its session (only while it still carries the sender's name, sessions are reused) and then
  // to the name, which is unique on a server.
  const people = useMemo(() => {
    const byCert = new Map<string, User>();
    const byName = new Map<string, User>();
    for (const user of users.values()) {
      if (user.certHash !== null) byCert.set(user.certHash, user);
      byName.set(user.name, user);
    }
    return { byCert, byName };
  }, [users]);
  const senderOf = useCallback(
    (message: ChatMessage): User | undefined => {
      if (message.own) return selfSession === null ? undefined : users.get(selfSession);
      if (message.senderCertHash !== null) return people.byCert.get(message.senderCertHash);
      const user = message.senderSession === null ? undefined : users.get(message.senderSession);
      return user?.name === message.senderName ? user : people.byName.get(message.senderName);
    },
    [users, selfSession, people],
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

  const certHash = conversation.kind === "private" ? conversation.certHash : null;
  const friend = useFriend(certHash);
  // A friend who is offline and never wrote is still known by the name last seen (FR-1.65).
  const name = info.name ?? friend?.name ?? t("header.unknownPerson");
  const title = info.isChannel ? t("header.channel", { name }) : name;

  useEffect(() => {
    if (!log.loaded && !log.loading) ensureLoaded(conversation);
  }, [conversation, log.loaded, log.loading]);

  // Long descriptions are only announced by hash: fetch the text once the chat is open. The
  // engine sends each request only once per description; the text arrives as a channel update.
  useEffect(() => {
    if (!pinned.needsFetch || pinnedChannelId === null) return;
    getApi()
      .channelDescriptionRequest(shownServerId(), pinnedChannelId)
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
    const unsubscribe = subscribeIncoming((message, serverId) => {
      if (serverId !== getShownServerId()) return;
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
        {conversation.kind === "channel" && renderChannelTitle ? (
          renderChannelTitle(conversation.channelId)
        ) : (
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
        )}
        <IdentityBadge className="chat-header__identity" />
        {!info.isChannel && (
          <PrivatePresence
            className="chat-header__presence"
            online={info.target !== null}
            certHash={certHash}
          />
        )}
        {renderHeaderActions ? (
          <div className="chat-header__actions">{renderHeaderActions(conversation)}</div>
        ) : null}
      </header>
      {belowHeader}
      {pinnedChannelId !== null && <ActivityBanners channelId={pinnedChannelId} />}
      <PinnedInfoBox blocks={pinnedBlocks} />
      <MessageList
        log={log}
        label={t("list.label", { name: title })}
        onLoadOlder={() => {
          void loadOlder(conversation);
        }}
        onSyncEarlier={
          conversation.kind === "channel"
            ? () => {
                void requestSync(conversation);
              }
            : undefined
        }
        clickablePeople
        display={display}
        avatarFor={avatarFor}
        linkPreviews={linkPreviews}
        onReply={blockedReason === null ? setReplyTo : undefined}
        listKey={`${String(serverId)}:${JSON.stringify(conversation)}`}
        collapsedIds={blocked.collapsedIds}
      />
      <Composer
        target={info.target}
        limits={limits}
        label={t("composer.label", { name: title })}
        blockedReason={blockedReason}
        linkPreviews={linkPreviews}
        reply={blockedReason === null ? replyTo : null}
        onCancelReply={() => {
          setReplyTo(null);
        }}
      />
    </div>
  );
}
