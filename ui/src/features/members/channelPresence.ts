import { useCallback } from "react";
import type { Conversation } from "../../api";
import { useSelectedConversation } from "../channels/selection";
import { useServerSelector, type ServerState } from "../channels/serverStore";

/** Who is in a channel, as the members button in the chat header summarises it (issue #165). */
export interface ChannelPresence {
  /** People in the channel, listeners included, like the "In <channel>" heading. */
  count: number;
  /** Names of the people talking in the channel, sorted by name. Listeners do not talk in it. */
  talkers: string[];
}

const byName = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: "base" });

/** `null` for a private chat or a channel that is not (or no longer) on the server. */
export function channelPresence(
  state: ServerState,
  conversation: Conversation | null,
): ChannelPresence | null {
  if (conversation?.kind !== "channel" || !state.channels.has(conversation.channelId)) return null;
  const channelId = conversation.channelId;
  let count = 0;
  const talkers: string[] = [];
  for (const user of state.users.values()) {
    if (user.channelId === channelId) {
      count += 1;
      if (state.talking.has(user.session)) talkers.push(user.name);
    } else if (user.listeningChannelIds.includes(channelId)) {
      count += 1;
    }
  }
  return { count, talkers: talkers.sort(byName) };
}

function samePresence(a: ChannelPresence | null, b: ChannelPresence | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.count === b.count &&
    a.talkers.length === b.talkers.length &&
    a.talkers.every((name, i) => name === b.talkers[i])
  );
}

/** The selected channel's presence; re-renders only when the count or the talkers change. */
export function useChannelPresence(): ChannelPresence | null {
  const { conversation } = useSelectedConversation();
  const select = useCallback(
    (state: ServerState) => channelPresence(state, conversation),
    [conversation],
  );
  return useServerSelector(select, samePresence);
}
