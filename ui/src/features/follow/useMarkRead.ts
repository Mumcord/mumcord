import { useEffect } from "react";
import { getApi, type Conversation } from "../../api";
import { unreadOf, useFollowState } from "./followStore";

/** Resets the unread count while the conversation is open in the chat column. */
export function useMarkRead(conversation: Conversation): void {
  const state = useFollowState();
  const count = unreadOf(state, conversation);
  useEffect(() => {
    if (count <= 0) return;
    getApi()
      .conversationMarkRead(conversation)
      .catch((error: unknown) => {
        console.error("Marking a conversation as read failed", error);
      });
  }, [conversation, count]);
}
