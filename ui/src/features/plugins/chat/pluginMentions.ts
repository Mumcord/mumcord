/**
 * Mentions for plugins (FR-6.16 `mentioned`): the chat decides what counts as a mention of the
 * user, so the UI reports each one; plugins learn who mentioned the user, never the text.
 */
import { useEffect } from "react";
import { getApi, type ChatMessage } from "../../../api";
import { isBlocked } from "../../blocking/blockedStore";
import { subscribeIncoming } from "../../chat/chatStore";
import { isMention } from "../../chat/ownMention";

export function reportMention(message: ChatMessage, serverId: number): void {
  const session = message.senderSession;
  if (message.own || session === null) return;
  if (isBlocked(serverId, message.senderCertHash, message.senderName)) return;
  if (!isMention(message, serverId)) return;
  getApi()
    .pluginsMentioned(serverId, session)
    .catch((error: unknown) => {
      console.error("Telling the plugins about a mention failed", error);
    });
}

/** Mount once in the app shell, so mentions count while the chat is closed too. */
export function usePluginMentions(): void {
  useEffect(() => subscribeIncoming(reportMention), []);
}
