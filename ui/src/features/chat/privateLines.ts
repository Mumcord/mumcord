/**
 * Lines only this admin sees (Epic 4, issue #378, FR-10.61): the result of a slash command and,
 * from FR-10.59, a refusal that had no open dialog. They are never sent to the server and never
 * stored — they live in memory for this session.
 *
 * A line is shaped like a chat message so the message list can place it at its time; the note
 * ("Only you see this line", mockup `05-result-line`) keeps it from looking like a sent message.
 */
import { useSyncExternalStore } from "react";
import type { ChatMessage, Conversation } from "../../api";
import { conversationKey } from "./chatStore";

/** How many lines are kept per conversation; older ones go first. */
const MAX_KEPT = 200;

export interface PrivateLine {
  readonly serverId: number;
  readonly conversationKey: string;
  /** Only its `id`, `conversation`, `sentAt` and `body` matter; the list draws those. */
  readonly message: ChatMessage;
  /** "Only you see this line", already translated. */
  readonly note: string;
}

let lines = new Map<string, PrivateLine[]>();
/** One stable empty list: `useSyncExternalStore` must see the same reference every time. */
const NONE: readonly PrivateLine[] = [];
const listeners = new Set<() => void>();
let nextId = 0;

function notify(): void {
  for (const listener of listeners) listener();
}

function key(serverId: number, conversation: Conversation): string {
  return `${String(serverId)}/${conversationKey(conversation)}`;
}

function publish(key: string, list: PrivateLine[]): void {
  lines = new Map(lines);
  lines.set(key, list.length > MAX_KEPT ? list.slice(list.length - MAX_KEPT) : list);
  notify();
}

/** Bodies are HTML in the message list, so the plain text of a line goes in escaped. */
function escapeHtml(text: string): string {
  return text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

/** Adds one line at the end of a conversation. */
export function addPrivateLine(input: {
  serverId: number;
  conversation: Conversation;
  text: string;
  note: string;
}): void {
  const listKey = key(input.serverId, input.conversation);
  const line: PrivateLine = {
    serverId: input.serverId,
    conversationKey: listKey,
    note: input.note,
    message: {
      id: `local-private-${String(nextId++)}`,
      conversation: input.conversation,
      // Shown on the admin's side and without a sender: the list draws the line as a note.
      senderName: "",
      senderCertHash: null,
      senderSession: null,
      own: true,
      sentAt: new Date().toISOString(),
      body: escapeHtml(input.text),
      dismissedPreviews: [],
      syncedFrom: null,
      file: null,
    },
  };
  publish(listKey, [...(lines.get(listKey) ?? []), line]);
}

export function privateLinesFor(
  serverId: number,
  conversation: Conversation,
): readonly PrivateLine[] {
  return lines.get(key(serverId, conversation)) ?? NONE;
}

/** The lines of one conversation, oldest first. */
export function usePrivateLines(
  serverId: number,
  conversation: Conversation,
): readonly PrivateLine[] {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => privateLinesFor(serverId, conversation),
  );
}

/** For tests. */
export function resetPrivateLines(): void {
  lines = new Map();
  nextId = 0;
  notify();
}
