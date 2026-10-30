import type { Conversation, Friend, User } from "../../api";
import type { DirectChat, FollowState } from "./followStore";

export interface Entry {
  serverId: number;
  chat: DirectChat;
  unread: number;
}

/**
 * Open private chats of every server (FR-2.11). A friend's chats are linked by certificate hash
 * (FR-1.65): they show once, on the server where the friend is online (else the first one), with
 * the unread counts added up.
 */
export function collectEntries(
  states: ReadonlyMap<number, FollowState>,
  order: readonly number[],
  friends: readonly Friend[],
): Entry[] {
  const entries: Entry[] = [];
  const friendEntries = new Map<string, Entry>();
  const ids = [
    ...order.filter((id) => states.has(id)),
    ...[...states.keys()].filter((id) => !order.includes(id)),
  ];
  for (const serverId of ids) {
    const state = states.get(serverId);
    if (!state) continue;
    for (const chat of state.chats) {
      const unread = state.unread.get(chat.key) ?? 0;
      const c = chat.conversation;
      const friend =
        c.kind === "private" ? friends.find((f) => f.certHash === c.certHash) : undefined;
      if (!friend) {
        entries.push({ serverId, chat, unread });
        continue;
      }
      const existing = friendEntries.get(friend.certHash);
      if (!existing) {
        const entry = { serverId, chat, unread };
        friendEntries.set(friend.certHash, entry);
        entries.push(entry);
        continue;
      }
      existing.unread += unread;
      if (friend.online?.serverId === serverId) {
        existing.serverId = serverId;
        existing.chat = chat;
      }
    }
  }
  return entries;
}

/** The online person a private chat is with. */
export function person(users: ReadonlyMap<number, User>, c: Conversation): User | undefined {
  if (c.kind === "private") return [...users.values()].find((u) => u.certHash === c.certHash);
  if (c.kind === "privateSession") return users.get(c.session);
  return undefined;
}

export function personName(users: ReadonlyMap<number, User>, c: Conversation): string | null {
  return person(users, c)?.name ?? null;
}
