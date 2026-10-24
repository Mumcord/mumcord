/**
 * Preview cards the user hid (QA.42). The engine remembers them with the stored message; this
 * keeps them for the running session as well, so a card stays hidden when the message list
 * rebuilds and for messages that were not stored.
 */
import { useSyncExternalStore } from "react";
import { getApi } from "../../api";

const NONE: readonly string[] = [];
let hidden = new Map<string, readonly string[]>();
const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Hides the card of `url` under the message and tells the engine to remember it. */
export function dismissPreview(messageId: string, url: string): void {
  const current = hidden.get(messageId) ?? NONE;
  if (current.includes(url)) return;
  hidden = new Map(hidden).set(messageId, [...current, url]);
  for (const listener of listeners) listener();
  getApi()
    .previewDismiss(messageId, url)
    .catch((error: unknown) => {
      console.error("Remembering a dismissed preview failed", error);
    });
}

/** Links of the message whose cards are hidden: those stored with it and those hidden since. */
export function useDismissedPreviews(
  messageId: string,
  stored: readonly string[],
): ReadonlySet<string> {
  const session = useSyncExternalStore(subscribe, () => hidden.get(messageId) ?? NONE);
  return new Set([...stored, ...session]);
}

/** Forgets what was hidden in this session (tests). */
export function clearDismissedPreviews(): void {
  hidden = new Map();
}
