/** Which person the "Send chat request" dialog is open for; the person menu closes before it opens. */
import { useSyncExternalStore } from "react";

export interface ChatRequestTarget {
  serverId: number;
  session: number;
  name: string;
  /** Gets the focus back when the dialog closes. */
  opener: HTMLElement | null;
}

let target: ChatRequestTarget | null = null;
const listeners = new Set<() => void>();

function set(next: ChatRequestTarget | null): void {
  target = next;
  for (const listener of listeners) listener();
}

export function openChatRequestDialog(next: ChatRequestTarget): void {
  set(next);
}

export function closeChatRequestDialog(): void {
  set(null);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useChatRequestTarget(): ChatRequestTarget | null {
  return useSyncExternalStore(subscribe, () => target);
}
