import { useMemo } from "react";
import type { ChatMessage, Conversation } from "../../api";
import { useBlockedMessages, useIsBlocked } from "../blocking";
import type { ConversationLog } from "./chatStore";

export interface BlockedView {
  /** The log as shown: without the messages of blocked users that are hidden. */
  log: ConversationLog;
  /** Messages drawn collapsed; `undefined` when there are none. */
  collapsedIds: ReadonlySet<string> | undefined;
}

/**
 * Applies blocks (issue #140) to a conversation: private messages of blocked users are never
 * shown; in a channel they are collapsed or hidden, as `settings.blockedMessages` says.
 */
export function blockedView(
  log: ConversationLog,
  conversation: Conversation,
  isBlocked: (message: ChatMessage) => boolean,
  mode: "collapse" | "hide",
): BlockedView {
  const blocked = log.messages.filter((message) => !message.own && isBlocked(message));
  if (blocked.length === 0) return { log, collapsedIds: undefined };
  if (conversation.kind === "channel" && mode === "collapse") {
    return { log, collapsedIds: new Set(blocked.map((message) => message.id)) };
  }
  const hidden = new Set(blocked);
  return {
    log: { ...log, messages: log.messages.filter((message) => !hidden.has(message)) },
    collapsedIds: undefined,
  };
}

export function useBlockedView(
  log: ConversationLog,
  serverId: number,
  conversation: Conversation,
): BlockedView {
  const isBlocked = useIsBlocked();
  const mode = useBlockedMessages();
  return useMemo(
    () =>
      blockedView(
        log,
        conversation,
        (message) => isBlocked(serverId, message.senderCertHash, message.senderName),
        mode,
      ),
    [log, conversation, isBlocked, serverId, mode],
  );
}
