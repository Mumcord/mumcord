/**
 * Whether the public server browser (issue #68) fills the main area. Opened from the server bar,
 * closed by picking a server there, by connecting to a listed server or with Escape.
 */
import { useSyncExternalStore } from "react";

let open = false;
const listeners = new Set<() => void>();

function set(next: boolean): void {
  if (open === next) return;
  open = next;
  for (const listener of listeners) listener();
}

export function closeServerBrowser(): void {
  set(false);
}

export function toggleServerBrowser(): void {
  set(!open);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useServerBrowserOpen(): boolean {
  return useSyncExternalStore(subscribe, () => open);
}
