/**
 * One message line below the channel tree for follow actions started from the tree (star,
 * "Follow all"): errors and the follow-all summary. A row has no room for inline messages.
 */
import { useSyncExternalStore } from "react";

export interface FollowNotice {
  text: string;
  /** Errors are announced assertively, summaries and progress politely. */
  error: boolean;
}

let notice: FollowNotice | null = null;
const listeners = new Set<() => void>();

export function setFollowNotice(next: FollowNotice | null): void {
  notice = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useFollowNotice(): FollowNotice | null {
  return useSyncExternalStore(subscribe, () => notice);
}
