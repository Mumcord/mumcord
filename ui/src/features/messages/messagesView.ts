/**
 * Whether the channel column shows the private messages overview (issue #173) instead of the
 * shown server's channels. The Messages button in the server bar opens it; picking a server
 * closes it.
 */
import { useSyncExternalStore } from "react";

let open = false;
const listeners = new Set<() => void>();

function set(next: boolean): void {
  if (open === next) return;
  open = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useMessagesOpen(): boolean {
  return useSyncExternalStore(subscribe, () => open);
}

export function openMessages(): void {
  set(true);
}

export function closeMessages(): void {
  set(false);
}
