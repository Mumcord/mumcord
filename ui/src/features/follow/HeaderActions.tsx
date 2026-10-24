import type { Conversation } from "../../api";
import { ClearHistory } from "../chat/history/ClearHistory";
import { FollowToggle } from "./FollowToggle";
import { useMarkRead } from "./useMarkRead";

/** Chat header controls: follow toggle (channels) and "Clear history"; keeps the chat marked read. */
export function HeaderActions({ conversation }: { conversation: Conversation }) {
  useMarkRead(conversation);
  return (
    <>
      {conversation.kind === "channel" && <FollowToggle channelId={conversation.channelId} />}
      <ClearHistory conversation={conversation} />
    </>
  );
}
