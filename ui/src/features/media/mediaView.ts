/**
 * How the two media views of issue #163 are shown: the watch party and a watched screen share use
 * one frame in the main column, with the chat below it and the panel in the right column. Leaving
 * the frame keeps the feature as the mini bar at the top of the chat, where it still works.
 *
 * Only one feature can fill the main column at a time; the one that was opened last wins, the
 * other stays as its mini bar. Focus (`Z`) hides the other columns and the chat and shows only the
 * picture.
 */
import { useSyncExternalStore } from "react";

export type MediaKind = "watch" | "share";
/** How the shared picture is shown (issue #163). */
export type FitMode = "fit" | "fill" | "oneToOne";
/** Panel tab; each feature has its own, so a switch does not surprise the other one. */
export type MediaTab = "playlist" | "watching" | "viewers";

export interface MediaView {
  /** Which feature owns the main column; `null` shows the chat alone. */
  cinema: MediaKind | null;
  /** Which features show as the mini bar at the top of the chat. */
  mini: Record<MediaKind, boolean>;
  /** Focus mode: only the picture. */
  focus: boolean;
  fit: FitMode;
  tab: MediaTab;
}

const initial: MediaView = {
  cinema: null,
  mini: { watch: false, share: false },
  focus: false,
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

/** Opens a feature in the main column, so it stops showing as the mini bar. */
export function openMedia(kind: MediaKind): void {
  if (view.cinema === kind && !view.mini[kind]) return;
  change({ ...view, cinema: kind, mini: { ...view.mini, [kind]: false } });
}

/** Leaves the main column; the feature goes on as the mini bar (issue #163). */
export function leaveCinema(): void {
  if (view.cinema === null) return;
  const kind = view.cinema;
  change({ ...view, cinema: null, mini: { ...view.mini, [kind]: true }, focus: false });
}

/** The feature is over (left the party, stopped watching); its bar goes with it. */
export function closeMedia(kind: MediaKind): void {
  if (view.cinema !== kind && !view.mini[kind]) return;
  change({
    ...view,
    cinema: view.cinema === kind ? null : view.cinema,
    mini: { ...view.mini, [kind]: false },
    focus: view.cinema === kind ? false : view.focus,
  });
}

export function setFocus(on: boolean): void {
  if (view.focus === on) return;
  change({ ...view, focus: on });
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
