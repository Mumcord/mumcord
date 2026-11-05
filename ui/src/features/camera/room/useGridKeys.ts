/**
 * Keyboard use of the camera tiles (spec 08 §5.5): the tiles are one tab stop with roving focus,
 * the arrow keys move between them, Home / End go to the first / last tile, and the keys of a
 * focused tile open its menu (Enter, Shift+F10), pin (P), show large (F), hide (H) or leave the
 * large view (Escape).
 */
import { useState, type FocusEvent, type KeyboardEvent } from "react";
import type { TileKey } from "./layout";

export type TileCommand = "menu" | "pin" | "large" | "hide" | "escape";

interface KeyLike {
  key: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

/** The tile index a navigation key moves to in a grid of `cols` columns; `null` for other keys. */
export function gridMove(key: string, index: number, count: number, cols: number): number | null {
  if (count === 0) return null;
  const last = count - 1;
  switch (key) {
    case "ArrowLeft":
      return Math.max(0, index - 1);
    case "ArrowRight":
      return Math.min(last, index + 1);
    case "ArrowUp":
      return index - cols >= 0 ? index - cols : index;
    case "ArrowDown":
      return index + cols <= last ? index + cols : index;
    case "Home":
      return 0;
    case "End":
      return last;
    default:
      return null;
  }
}

/** The tile command of a key press on a focused tile; `null` for every other key. */
export function tileCommand(event: KeyLike): TileCommand | null {
  if (event.ctrlKey || event.altKey || event.metaKey) return null;
  if (event.key === "F10") return event.shiftKey ? "menu" : null;
  if (event.key === "ContextMenu") return "menu";
  if (event.shiftKey) return null;
  switch (event.key) {
    case "Enter":
      return "menu";
    case "Escape":
      return "escape";
    case "p":
    case "P":
      return "pin";
    case "f":
    case "F":
      return "large";
    case "h":
    case "H":
      return "hide";
    default:
      return null;
  }
}

/** The tile elements of a container, in reading order. */
function tileElements(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>("[data-tile]")];
}

export interface GridKeys {
  /** 0 for the one tile that takes the Tab focus, -1 for the others. */
  tabIndexOf: (key: TileKey) => 0 | -1;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
  onFocus: (event: FocusEvent<HTMLElement>) => void;
}

/**
 * Roving focus over the tiles inside the element that gets `onKeyDown` / `onFocus`. `keys` are the
 * tiles in reading order; `onCommand` runs a tile command on the focused tile and says whether it
 * did something (an Escape with nothing to leave goes on to the outer handlers).
 */
export function useGridKeys(
  keys: readonly TileKey[],
  cols: number,
  onCommand: (key: TileKey, command: TileCommand, tile: HTMLElement) => boolean,
): GridKeys {
  const [active, setActive] = useState<TileKey | null>(null);
  const current = active !== null && keys.includes(active) ? active : (keys[0] ?? null);
  const keyOf = (element: HTMLElement): TileKey | undefined =>
    keys.find((key) => String(key) === element.dataset.tile);

  const onFocus = (event: FocusEvent<HTMLElement>) => {
    const tile =
      event.target instanceof HTMLElement ? event.target.closest<HTMLElement>("[data-tile]") : null;
    const key = tile === null ? undefined : keyOf(tile);
    if (key !== undefined && key !== active) setActive(key);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const tile = event.target;
    // Only the tile itself: Enter on a button inside it presses that button.
    if (!(tile instanceof HTMLElement) || tile.dataset.tile === undefined) return;
    const key = keyOf(tile);
    if (key === undefined) return;
    const elements = tileElements(event.currentTarget);
    const index = elements.indexOf(tile);
    const next = gridMove(event.key, index, elements.length, cols);
    if (next !== null) {
      event.preventDefault();
      const target = elements[next];
      if (target !== undefined && target !== tile) target.focus();
      return;
    }
    const command = tileCommand(event);
    if (command === null || !onCommand(key, command, tile)) return;
    event.preventDefault();
    if (command === "menu") return;
    // Show large moves the tile to another place, where React draws it anew: keep the focus on it.
    const container = event.currentTarget;
    const id = tile.dataset.tile;
    requestAnimationFrame(() => {
      if (container.contains(document.activeElement)) return;
      tileElements(container)
        .find((element) => element.dataset.tile === id)
        ?.focus();
    });
  };

  return {
    tabIndexOf: (key) => (key === current ? 0 : -1),
    onKeyDown,
    onFocus,
  };
}
