/**
 * The chosen height of the media area (spec 07 §4.9) and the short-lived state of a drag. One
 * value for the whole app: every channel, the screen share, the watch party and later the camera
 * room use it (FR-7.70). Leaving the big view does not touch it, so "Open" on the mini bar comes
 * back at the height chosen last (FR-7.69).
 */
import { useSyncExternalStore } from "react";
import { loadMediaHeight, saveMediaHeight, type MediaHeight } from "../../app/mediaHeight";

export interface MediaResize {
  chosen: MediaHeight;
  /** A drag is going on. */
  dragging: boolean;
  /** The pointer is below the collapse line: releasing minimises to the mini bar. */
  collapsing: boolean;
  /** The next height change is a snap, a reset or the collapse, which may animate (FR-7.73). */
  animate: boolean;
}

let state: MediaResize | null = null;
const listeners = new Set<() => void>();

function current(): MediaResize {
  state ??= { chosen: loadMediaHeight(), dragging: false, collapsing: false, animate: false };
  return state;
}

function change(patch: Partial<MediaResize>): void {
  const prev = current();
  const next = { ...prev, ...patch };
  if (
    next.chosen === prev.chosen &&
    next.dragging === prev.dragging &&
    next.collapsing === prev.collapsing &&
    next.animate === prev.animate
  )
    return;
  state = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Shows a new height; `save` remembers it across restarts (drag end, key press). */
export function setMediaHeight(
  chosen: MediaHeight,
  { save = false, animate = false }: { save?: boolean; animate?: boolean } = {},
): void {
  change({ chosen, animate });
  if (save) saveMediaHeight(chosen);
}

export function setMediaDrag(dragging: boolean, collapsing = false): void {
  change({ dragging, collapsing: dragging && collapsing });
}

export function useMediaResize(): MediaResize {
  return useSyncExternalStore(subscribe, current);
}

/** Test helper: forget the state so the next read loads the stored height again. */
export function resetMediaResize(): void {
  state = null;
  for (const listener of listeners) listener();
}
