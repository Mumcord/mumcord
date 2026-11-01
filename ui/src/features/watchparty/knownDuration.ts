/**
 * The length of the playing video as the player reports it (issue #163). Only the entry that plays
 * has a known length, so the panel shows a duration on the Now playing card and nowhere else; the
 * cinema view writes it, the panel in the right column reads it.
 */
import { useSyncExternalStore } from "react";

let known: { videoId: string; durationMs: number } | null = null;
const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function setKnownDuration(videoId: string, durationMs: number): void {
  if (durationMs <= 0 || (known?.videoId === videoId && known.durationMs === durationMs)) return;
  known = { videoId, durationMs };
  for (const listener of listeners) listener();
}

/** The length of `videoId` in milliseconds, or `null` while the player has not said it. */
export function useKnownDuration(videoId: string): number | null {
  const current = useSyncExternalStore(subscribe, () => known);
  return current?.videoId === videoId ? current.durationMs : null;
}

/** Test helper: nothing known. */
export function resetKnownDuration(): void {
  known = null;
  for (const listener of listeners) listener();
}
