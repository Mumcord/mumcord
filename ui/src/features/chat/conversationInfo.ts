/**
 * What the chat column needs to know about the selected conversation: its title, who receives
 * messages, and whether the user may write there. Derived from the shared server state (B20).
 */
import type { ChatMessage, Conversation, User } from "../../api";
import type { ServerState } from "../channels/serverStore";

export type SendTarget =
  { kind: "channel"; channelId: number } | { kind: "private"; session: number };

export interface ConversationInfo {
  /** Channel or person name; `null` if the person is unknown (offline, no messages yet). */
  name: string | null;
  isChannel: boolean;
  /** `null` when nobody can receive messages right now (person offline, channel gone). */
  target: SendTarget | null;
  /** False when the server's ACL denies writing in the channel. */
  mayWrite: boolean;
}

function findPerson(state: ServerState, conversation: Conversation): User | undefined {
  if (conversation.kind === "privateSession") return state.users.get(conversation.session);
  if (conversation.kind === "private") {
    for (const user of state.users.values()) {
      if (user.certHash === conversation.certHash) return user;
    }
  }
  return undefined;
}

export function conversationInfo(
  state: ServerState,
  conversation: Conversation,
  messages: readonly ChatMessage[],
): ConversationInfo {
  if (conversation.kind === "channel") {
    const channel = state.channels.get(conversation.channelId);
    return {
      name: channel?.name ?? null,
      isChannel: true,
      target: channel ? { kind: "channel", channelId: channel.id } : null,
      mayWrite: channel?.permissions.textMessage ?? false,
    };
  }
  const person = findPerson(state, conversation);
  const fromHistory = [...messages].reverse().find((m) => !m.own)?.senderName ?? null;
  return {
    name: person?.name ?? fromHistory,
    isChannel: false,
    target: person ? { kind: "private", session: person.session } : null,
    mayWrite: true,
  };
}
