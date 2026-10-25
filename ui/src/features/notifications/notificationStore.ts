/**
 * In-app toasts and the per-chat count of messages that arrived while the chat was not on
 * screen (shared by toasts and system notifications for "5 new messages in #Raid").
 */
import { useSyncExternalStore } from "react";
import type { Conversation } from "../../api";

/** At most this many toasts are visible; older ones make room. */
export const MAX_VISIBLE = 3;

export interface Toast {
  key: string;
  conversation: Conversation;
  /** "#Raid" or the sender's name for a private chat. */
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

/** True while the chat has an unseen mention (for the "@" on its unread badge). */
export function useHasMention(key: string | undefined): boolean {
  return useSyncExternalStore(subscribe, () => key !== undefined && mentions.has(key));
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

/** Server change or disconnect: chats of the old server are gone. */
export function clearNotifications(): void {
  counts = new Map();
  if (mentions.size > 0) {
    mentions = new Set();
    for (const listener of listeners) listener();
  }
  if (toasts.length > 0) setToasts([]);
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
