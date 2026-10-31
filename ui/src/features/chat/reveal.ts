/**
 * A message another feature wants to show, e.g. the notification centre (#143) jumping to a
 * mention. The message list of the chat that holds it scrolls to it, focuses and highlights it,
 * then clears the request. Message ids are unique across chats and servers.
 */
import { useSyncExternalStore } from "react";

let pending: string | null = null;
const listeners = new Set<() => void>();

function set(next: string | null): void {
  pending = next;
  for (const listener of listeners) listener();
}

/** Asks the chat to show this message once its list has it. */
export function revealMessage(messageId: string): void {
  set(messageId);
}

/** The list showed the message; later renders must not jump again. */
export function revealDone(messageId: string): void {
  if (pending === messageId) set(null);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The id of the message waiting to be shown, if any. */
export function usePendingReveal(): string | null {
  return useSyncExternalStore(subscribe, () => pending);
}
