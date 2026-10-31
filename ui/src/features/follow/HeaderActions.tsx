import type { Conversation } from "../../api";
import { ClearHistory } from "../chat/history/ClearHistory";
import { useMarkRead } from "./useMarkRead";

/**
 * Chat header controls; keeps the chat marked read. Channels have their actions in the
 * channel-name menu (issue #170), so only private chats keep a "Clear history" button here.
 */
export function HeaderActions({ conversation }: { conversation: Conversation }) {
  useMarkRead(conversation);
  return conversation.kind === "channel" ? null : <ClearHistory conversation={conversation} />;
}
