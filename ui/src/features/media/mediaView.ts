/**
 * How the media views are shown (issue #163, FR-9.44): the watch party, a watched screen share and
 * plugin activities share one media area in the main column, with the chat below it and the panel
 * in the right column. Leaving the area keeps the built-in features as mini bars at the top of the
 * chat, where they still work.
 *
 * Every item in the area is a stage tab; one of them is the main item. A new item never takes the
 * stage while another one shows: it becomes a tab with a dot until the user picks it (FR-9.44).
 */
import { useSyncExternalStore } from "react";

export type MediaKind = "watch" | "share";
/** One item of the media area: a built-in feature or a plugin activity (`plugin:<id>`). */
export type StageId = MediaKind | `plugin:${string}`;
/** How the shared picture is shown (issue #163). */
export type FitMode = "fit" | "fill" | "oneToOne";
/** Panel tab; each feature has its own, so a switch does not surprise the other one. */
export type MediaTab = "playlist" | "watching" | "viewers";

export interface MediaView {
  /** The main item of the media area; `null` shows the chat alone. */
  cinema: StageId | null;
  /** The items of the media area in the order they came, one stage tab each. */
  stage: readonly StageId[];
  /** Items that came while another one was the main item and were not looked at yet (the dot). */
  unseen: readonly StageId[];
  /** Which features show as the mini bar at the top of the chat. */
  mini: Record<MediaKind, boolean>;
  fit: FitMode;
  tab: MediaTab;
}

const initial: MediaView = {
  cinema: null,
  stage: [],
  unseen: [],
  mini: { watch: false, share: false },
  fit: "fit",
  tab: "playlist",
};

let view: MediaView = { ...initial, mini: { ...initial.mini } };
const listeners = new Set<() => void>();

function change(next: MediaView): void {
  view = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

const isKind = (id: StageId): id is MediaKind => id === "watch" || id === "share";

/** The area shows `main`: no item of it stays a mini bar, and its dot is gone. */
function showing(stage: readonly StageId[], main: StageId): MediaView {
  const mini = { ...view.mini };
  for (const id of stage) if (isKind(id)) mini[id] = false;
  return { ...view, cinema: main, stage, unseen: view.unseen.filter((id) => id !== main), mini };
}

/**
 * Brings an item into the media area. With the area closed it opens on this item; while another
 * item is the main one, a new item only adds its tab with a dot (FR-9.44). An item that is
 * already there (its mini bar or its pop-out coming back) becomes the main item.
 */
export function openMedia(id: StageId): void {
  const known = view.stage.includes(id);
  const stage = known ? view.stage : [...view.stage, id];
  if (view.cinema === null || known) {
    if (view.cinema === id && (!isKind(id) || !view.mini[id])) return;
    change(showing(stage, id));
    return;
  }
  change({ ...view, stage, unseen: [...view.unseen, id] });
}

/** Makes an item of the area the main one (a stage tab, Alt+1 … Alt+9); nothing else stops. */
export function selectStage(id: StageId): void {
  if (!view.stage.includes(id) || (view.cinema === id && !view.unseen.includes(id))) return;
  change(showing(view.stage, id));
}

/** Leaves the main column; the features go on as mini bars (issue #163). */
export function leaveCinema(): void {
  if (view.cinema === null) return;
  const mini = { ...view.mini };
  for (const id of view.stage) if (isKind(id)) mini[id] = true;
  change({ ...view, cinema: null, mini });
}

/** The item is over (left the party, stopped watching); its tab and mini bar go with it. */
export function closeMedia(id: StageId): void {
  const at = view.stage.indexOf(id);
  if (at === -1 && view.cinema !== id && !(isKind(id) && view.mini[id])) return;
  const stage = view.stage.filter((item) => item !== id);
  // The neighbour tab takes over, so closing one item never hides the others.
  const cinema =
    view.cinema !== id
      ? view.cinema
      : (stage[Math.min(Math.max(at - 1, 0), stage.length - 1)] ?? null);
  const mini = isKind(id) ? { ...view.mini, [id]: false } : view.mini;
  change({
    ...view,
    cinema,
    stage,
    unseen: view.unseen.filter((item) => item !== id && item !== cinema),
    mini,
  });
}

export function setFit(fit: FitMode): void {
  if (view.fit === fit) return;
  change({ ...view, fit });
}

export function setTab(tab: MediaTab): void {
  if (view.tab === tab) return;
  change({ ...view, tab });
}

/** The way media is shown right now. */
export function useMediaView(): MediaView {
  return useSyncExternalStore(subscribe, () => view);
}

/** Same view without React, for the keyboard handler and mounts that decide what to render. */
export function mediaView(): MediaView {
  return view;
}

/** Test helper: back to the start. */
export function resetMediaView(): void {
  view = { ...initial, mini: { ...initial.mini } };
  for (const listener of listeners) listener();
}
