/** Which accepted conversation the "Revoke chat consent" confirmation is open for (issue #141). */
import { useSyncExternalStore } from "react";
import type { AcceptedConversation } from "../../api";

export interface RevokeConsentTarget {
  conversation: AcceptedConversation;
  /** Gets the focus back when the dialog closes. */
  opener: HTMLElement | null;
}

let target: RevokeConsentTarget | null = null;
const listeners = new Set<() => void>();

function set(next: RevokeConsentTarget | null): void {
  target = next;
  for (const listener of listeners) listener();
}

export function openRevokeConsentDialog(next: RevokeConsentTarget): void {
  set(next);
}

export function closeRevokeConsentDialog(): void {
  set(null);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useRevokeConsentTarget(): RevokeConsentTarget | null {
  return useSyncExternalStore(subscribe, () => target);
}
