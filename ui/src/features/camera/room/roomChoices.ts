/**
 * The viewer's own choices in the camera room: the layout (Grid / Speaker, FR-8.62), whether the
 * self-view is hidden (FR-8.66) and the pinned tiles (FR-8.63, set by the tile menu). Only this
 * window knows them; the per-tile ones are forgotten when the room closes (spec 08 §7.1).
 */
import { useSyncExternalStore } from "react";
import type { TileKey } from "./layout";

export type RoomLayout = "grid" | "speaker";

export interface RoomChoices {
  layout: RoomLayout;
  selfHidden: boolean;
  /** Pinned tiles in the order they were pinned. */
  pinned: readonly TileKey[];
}

const initial: RoomChoices = { layout: "grid", selfHidden: false, pinned: [] };
let choices = initial;
const listeners = new Set<() => void>();

function change(next: RoomChoices): void {
  choices = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useRoomChoices(): RoomChoices {
  return useSyncExternalStore(subscribe, () => choices);
}

export function setRoomLayout(layout: RoomLayout): void {
  if (choices.layout !== layout) change({ ...choices, layout });
}

export function setSelfHidden(selfHidden: boolean): void {
  if (choices.selfHidden !== selfHidden) change({ ...choices, selfHidden });
}

/** Pins or unpins a tile for this viewer only. */
export function setPinned(key: TileKey, pinned: boolean): void {
  const has = choices.pinned.includes(key);
  if (has === pinned) return;
  change({
    ...choices,
    pinned: pinned ? [...choices.pinned, key] : choices.pinned.filter((k) => k !== key),
  });
}

/** The room closed: the per-tile choices go (spec 08 §7.1). */
export function forgetTileChoices(): void {
  if (choices.pinned.length > 0) change({ ...choices, pinned: [] });
}

/** Test helper. */
export function resetRoomChoices(): void {
  change(initial);
}
