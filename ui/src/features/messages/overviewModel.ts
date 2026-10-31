/**
 * Rows of the private messages overview (issue #173): one merged list of the private
 * conversations of every connected server, pinned first, then newest first. Pure, so the order,
 * search and filters are tested without rendering.
 */
import type { Conversation, PmConversationPrefs } from "../../api";
import { conversationKey } from "../chat/chatStore";
import { isMuted, type LastMessage } from "./conversationsStore";

export type Filter = "all" | "unread" | "requests";

/** A private chat known for this session (open, unread or messaged), from the follow store. */
export interface OpenChat {
  serverId: number;
  conversation: Conversation;
  /** Name known when the chat was opened. */
  name: string | null;
  unread: number;
}

export interface Row {
  /** Stable React key and DOM id part. */
  id: string;
  serverId: number;
  conversation: Conversation;
  /** Certificate hash of the other person; `null` for a chat bound to this session. */
  certHash: string | null;
  name: string;
  serverName: string;
  online: boolean;
  last: LastMessage | null;
  unread: number;
  pinned: boolean;
  muted: boolean;
}

export interface RowsInput {
  chats: readonly OpenChat[];
  last: ReadonlyMap<number, ReadonlyMap<string, LastMessage>>;
  prefs: readonly PmConversationPrefs[];
  /** Certificate hashes of friends: their chats on several servers show as one row (FR-1.65). */
  friends: ReadonlySet<string>;
  /** Live name of the person on that server, `undefined` when they are not online there. */
  liveName: (serverId: number, conversation: Conversation) => string | undefined;
  serverName: (serverId: number) => string;
  unknownName: string;
  now: number;
}

function certHashOf(conversation: Conversation): string | null {
  return conversation.kind === "private" ? conversation.certHash : null;
}

interface Draft {
  serverId: number;
  conversation: Conversation;
  storedName: string | null;
  last: LastMessage | null;
  unread: number;
}

function newer(a: LastMessage | null, b: LastMessage | null): LastMessage | null {
  if (a === null) return b;
  if (b === null) return a;
  return b.at > a.at ? b : a;
}

/** All conversation rows, pinned first, then by the newest message (no message: last, by name). */
export function buildRows(input: RowsInput): Row[] {
  const drafts = new Map<string, Draft>();
  const add = (
    serverId: number,
    conversation: Conversation,
    storedName: string | null,
    last: LastMessage | null,
    unread: number,
  ) => {
    const certHash = certHashOf(conversation);
    const id =
      certHash !== null && input.friends.has(certHash)
        ? `friend:${certHash}`
        : `${String(serverId)}:${conversationKey(conversation)}`;
    const existing = drafts.get(id);
    if (!existing) {
      drafts.set(id, { serverId, conversation, storedName, last, unread });
      return;
    }
    existing.unread += unread;
    existing.storedName ??= storedName;
    const latest = newer(existing.last, last);
    // A friend's row shows the server where they are online, else the one with the newest message.
    const onlineHere = input.liveName(serverId, conversation) !== undefined;
    const onlineThere = input.liveName(existing.serverId, existing.conversation) !== undefined;
    const lastIsNewer = last !== null && latest === last && latest !== existing.last;
    if (onlineHere !== onlineThere ? onlineHere : lastIsNewer) {
      existing.serverId = serverId;
      existing.conversation = conversation;
    }
    existing.last = latest;
  };

  const seen = new Set<string>();
  for (const chat of input.chats) {
    const key = conversationKey(chat.conversation);
    seen.add(`${String(chat.serverId)}:${key}`);
    const last = input.last.get(chat.serverId)?.get(key) ?? null;
    add(chat.serverId, chat.conversation, chat.name ?? last?.peerName ?? null, last, chat.unread);
  }
  for (const [serverId, server] of input.last) {
    for (const [key, last] of server) {
      if (seen.has(`${String(serverId)}:${key}`)) continue;
      add(serverId, last.conversation, last.peerName, last, 0);
    }
  }

  const rows: Row[] = [...drafts].map(([id, d]) => {
    const certHash = certHashOf(d.conversation);
    const live = input.liveName(d.serverId, d.conversation);
    const prefs = input.prefs.find((p) => p.serverId === d.serverId && p.certHash === certHash);
    return {
      id,
      serverId: d.serverId,
      conversation: d.conversation,
      certHash,
      name: live ?? d.storedName ?? input.unknownName,
      serverName: input.serverName(d.serverId),
      online: live !== undefined,
      last: d.last,
      unread: d.unread,
      pinned: certHash !== null && (prefs?.pinned ?? false),
      muted: isMuted(input.prefs, d.serverId, certHash, input.now),
    };
  });
  return rows.sort(
    (a, b) =>
      Number(b.pinned) - Number(a.pinned) ||
      (b.last?.at ?? -Infinity) - (a.last?.at ?? -Infinity) ||
      a.name.localeCompare(b.name),
  );
}

/** Whether a row matches the search: name, server or preview text, ignoring case. */
export function matches(row: Row, query: string): boolean {
  const q = query.trim().toLocaleLowerCase();
  if (q === "") return true;
  return [row.name, row.serverName, row.last?.text ?? ""].some((s) =>
    s.toLocaleLowerCase().includes(q),
  );
}

/** Number of conversations with unread messages, for the "Unread" filter count. */
export function unreadConversations(rows: readonly Row[]): number {
  return rows.filter((r) => r.unread > 0).length;
}
