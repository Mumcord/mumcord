/**
 * What the tile menu and the tile keys do (spec 08 §5.3, §5.5): pin for me, show large, hide the
 * video (the own tile: hide the self-view) and the quality for me. Which tile's menu is open is
 * kept here too, so the tile keys (Enter, Shift+F10) and the ⋯ button open the same menu.
 */
import { useSyncExternalStore } from "react";
import { getApi, type CameraTileQuality } from "../../../api";
import type { TileKey } from "./layout";
import { setLargeTile, setPinned, setSelfHidden } from "./roomChoices";
import type { TileModel } from "./useRoomModel";

export function togglePin(tile: TileModel): void {
  setPinned(tile.key, !tile.pinned);
}

/** Shows the tile large, or leaves the large view when it already is. */
export function toggleLarge(tile: TileModel, large: TileKey | null): void {
  setLargeTile(large === tile.key ? null : tile.key);
}

/** Hides the video (FR-8.57: the engine unbinds) or shows it again; the own tile hides itself. */
export function toggleHidden(serverId: number, tile: TileModel): void {
  if (tile.state === "self") {
    setSelfHidden(true);
    return;
  }
  getApi()
    .cameraTileSetHidden(serverId, tile.session, tile.state !== "hidden")
    .catch((error: unknown) => {
      console.error("Hiding or showing a camera failed", error);
    });
}

export function setQuality(serverId: number, tile: TileModel, quality: CameraTileQuality): void {
  getApi()
    .cameraTileSetQuality(serverId, tile.session, quality)
    .catch((error: unknown) => {
      console.error("Setting the camera quality failed", error);
    });
}

export interface OpenTileMenu {
  key: TileKey;
  /** Gets the focus back when the menu closes: the ⋯ button or the tile. */
  opener: HTMLElement;
}

let open: OpenTileMenu | null = null;
const listeners = new Set<() => void>();

function change(next: OpenTileMenu | null): void {
  open = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function openTileMenu(key: TileKey, opener: HTMLElement): void {
  change({ key, opener });
}

export function closeTileMenu(): void {
  if (open !== null) change(null);
}

/** The open tile menu, if any. */
export function useOpenTileMenu(): OpenTileMenu | null {
  return useSyncExternalStore(subscribe, () => open);
}
