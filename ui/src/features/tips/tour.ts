/**
 * Aaron's tips (#637): a short tour of the main window, offered once right after the first-start
 * setup wizard and started again from Settings → About → Tips. Nothing is stored: the offer
 * only follows the wizard, so later starts never show it by themselves.
 */
import { useSyncExternalStore } from "react";

/**
 * The tips in tour order; each has a title and a body under `tips.<id>` in the "tips" texts.
 * Aaron points at the element marked `data-tour="<id>"`; the last tip has no element.
 */
export const TIPS = [
  "servers",
  "channels",
  "talk",
  "panel",
  "search",
  "chat",
  "apps",
  "themes",
  "end",
] as const;

export type TipId = (typeof TIPS)[number];

/** Nothing shown, the first-start offer, or the tip at `index` of [`TIPS`]. */
export type Tour = { kind: "hidden" } | { kind: "offer" } | { kind: "tip"; index: number };

const HIDDEN: Tour = { kind: "hidden" };

let tour: Tour = HIDDEN;
const listeners = new Set<() => void>();

function set(next: Tour): void {
  tour = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useTour(): Tour {
  return useSyncExternalStore(subscribe, () => tour);
}

/** The first-start wizard is done: Aaron offers the tour. */
export function offerTour(): void {
  set({ kind: "offer" });
}

export function startTour(): void {
  set({ kind: "tip", index: 0 });
}

/** The next tip; after the last one the tour ends. */
export function nextTip(): void {
  if (tour.kind !== "tip") return;
  set(tour.index + 1 < TIPS.length ? { kind: "tip", index: tour.index + 1 } : HIDDEN);
}

export function previousTip(): void {
  if (tour.kind === "tip" && tour.index > 0) set({ kind: "tip", index: tour.index - 1 });
}

/** Closes the offer or the tour at once: "Not now", "Skip tour", Done, × and Escape. */
export function closeTour(): void {
  if (tour.kind !== "hidden") set(HIDDEN);
}
