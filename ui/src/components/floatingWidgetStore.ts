/**
 * Session memory of floating widgets (FR-2.70): position, size and collapse per widget type, and
 * the stacking order. Kept in memory only; a restart starts from the defaults again.
 */
import { useSyncExternalStore } from "react";
import type { Rect } from "./floatingWidgetGeometry";

export interface WidgetLayout {
  /** The size and position the user chose, before fitting it into the current window. */
  rect: Rect;
  collapsed: boolean;
}

/** Widgets stack in this z-index range: above the shell, below menus (70) and popovers (45+). */
export const Z_BASE = 30;
const Z_SLOTS = 10;

let layouts = new Map<string, WidgetLayout>();
let order: readonly string[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getWidgetLayout(kind: string): WidgetLayout | undefined {
  return layouts.get(kind);
}

export function setWidgetLayout(kind: string, layout: WidgetLayout): void {
  layouts = new Map(layouts);
  layouts.set(kind, layout);
  notify();
}

/** The widget's layout, starting from `initial` the first time in this session. */
export function useWidgetLayout(kind: string, initial: () => WidgetLayout): WidgetLayout {
  const stored = useSyncExternalStore(subscribe, () => layouts.get(kind));
  return stored ?? initial();
}

/** Puts a widget on top of the others (the last one clicked wins). */
export function bringToFront(kind: string): void {
  if (order[order.length - 1] === kind) return;
  order = [...order.filter((other) => other !== kind), kind];
  notify();
}

export function useZIndex(kind: string): number {
  const index = useSyncExternalStore(subscribe, () => order.indexOf(kind));
  return Z_BASE + Math.min(Math.max(index, 0), Z_SLOTS - 1);
}

/** Test helper: forgets all layouts and the stacking order. */
export function resetFloatingWidgets(): void {
  layouts = new Map();
  order = [];
  notify();
}
