/**
 * In-app toasts and the per-chat count of messages that arrived while the chat was not on
 * screen (shared by toasts and system notifications for "5 new messages in #Raid").
 */
import { useSyncExternalStore } from "react";
import type { Conversation } from "../../api";
import { useShownServerId } from "../channels/serverStore";

/** At most this many toasts are visible; older ones make room. */
export const MAX_VISIBLE = 3;

/** Key of a chat across servers: the same channel id or person can exist on several. */
export function noticeKey(serverId: number, conversationKey: string): string {
  return `${String(serverId)}/${conversationKey}`;
}

export interface Toast {
  /** `noticeKey` of the chat. */
  key: string;
  /** The server the chat belongs to (FR-2.10). */
  serverId: number;
  conversation: Conversation;
  /** "‹server› · #Raid" or "‹server› · ‹sender›" for a private chat. */
  title: string;
  /** Plain, already translated summary line ("Alice: hi" or "5 new messages in #Raid"). */
  text: string;
  /** Grows with every update, so the toast restarts its timer. */
  version: number;
}

let toasts: readonly Toast[] = [];
let counts = new Map<string, number>();
let version = 0;
/** Chats with an unseen message that mentions the user. */
let mentions: ReadonlySet<string> = new Set();
const listeners = new Set<() => void>();

function setToasts(next: readonly Toast[]): void {
  toasts = next;
  for (const listener of listeners) listener();
}

/** One more message for this chat; returns the new count. */
export function bumpCount(key: string): number {
  const count = (counts.get(key) ?? 0) + 1;
  counts.set(key, count);
  return count;
}

/** An unseen message in this chat mentions the user. */
export function markMention(key: string): void {
  if (mentions.has(key)) return;
  mentions = new Set(mentions).add(key);
  for (const listener of listeners) listener();
}

/**
 * True while the chat has an unseen mention (for the "@" on its unread badge). `key` is the
 * conversation key; `serverId` defaults to the shown server.
 */
export function useHasMention(key: string | undefined, serverId?: number): boolean {
  const shown = useShownServerId();
  const server = serverId ?? shown;
  return useSyncExternalStore(
    subscribe,
    () => key !== undefined && server !== null && mentions.has(noticeKey(server, key)),
  );
}

function forgetMention(key: string): void {
  if (!mentions.has(key)) return;
  const next = new Set(mentions);
  next.delete(key);
  mentions = next;
  for (const listener of listeners) listener();
}

/** The chat was seen: start counting from zero and drop its toast. */
export function markSeen(key: string): void {
  counts.delete(key);
  forgetMention(key);
  removeToast(key);
}

/** Shows or updates the chat's toast; it moves to the newest position. */
export function showToast(toast: Omit<Toast, "version">): void {
  version += 1;
  const rest = toasts.filter((t) => t.key !== toast.key);
  setToasts([...rest, { ...toast, version }].slice(-MAX_VISIBLE));
}

export function removeToast(key: string): void {
  if (toasts.some((t) => t.key === key)) setToasts(toasts.filter((t) => t.key !== key));
}

/** A server disconnected: its counts, mentions and toasts are gone. */
export function clearServerNotifications(serverId: number): void {
  const prefix = noticeKey(serverId, "");
  for (const key of [...counts.keys()]) if (key.startsWith(prefix)) counts.delete(key);
  if ([...mentions].some((key) => key.startsWith(prefix))) {
    mentions = new Set([...mentions].filter((key) => !key.startsWith(prefix)));
    for (const listener of listeners) listener();
  }
  if (toasts.some((toast) => toast.serverId === serverId)) {
    setToasts(toasts.filter((toast) => toast.serverId !== serverId));
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Visible toasts, oldest first. */
export function useToasts(): readonly Toast[] {
  return useSyncExternalStore(subscribe, () => toasts);
}

/** Test helper. */
export function resetNotifications(): void {
  toasts = [];
  counts = new Map();
  mentions = new Set();
  version = 0;
}
