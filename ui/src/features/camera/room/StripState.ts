/**
 * The camera strip next to a screen share or watch party (spec 08 §5.4, FR-8.71–8.74): where it
 * sits (Cameras beside / Above / Hidden, remembered per device as `camera_strip_position`) and
 * which strip tile Show large swapped into the main picture until Escape.
 */
import { useSyncExternalStore } from "react";
import { getApi, type CameraStripPosition } from "../../../api";
import { useMediaView, type StageId } from "../../media/mediaView";
import { usePopouts } from "../../media/popoutStore";
import type { TileKey } from "./layout";

export interface StripState {
  position: CameraStripPosition;
  /** The strip tile shown in place of the main picture (FR-8.73); `null` shows the main picture. */
  large: TileKey | null;
}

const initial: StripState = { position: "beside", large: null };
let state = initial;
let loaded = false;
const listeners = new Set<() => void>();

function change(next: StripState): void {
  state = next;
  for (const listener of listeners) listener();
}

/** Reads the saved position once, the first time anything shows the strip. */
function load(): void {
  if (loaded) return;
  loaded = true;
  getApi()
    .settingsGet()
    .then(
      (settings) => {
        if (settings.cameraStripPosition !== state.position)
          change({ ...state, position: settings.cameraStripPosition });
      },
      (error: unknown) => {
        console.error("Loading the camera strip position failed", error);
      },
    );
}

function subscribe(listener: () => void): () => void {
  load();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The main items the cameras make room for (FR-8.71). */
export type StripMain = Extract<StageId, "share" | "watch">;

/** The main item the strip goes with, or `null` while the cameras are not a strip. */
export function useStripMain(serverId: number | null): StripMain | null {
  const view = useMediaView();
  const popped = usePopouts().camera.target;
  if (serverId === null || !view.stage.includes("camera") || popped?.serverId === serverId)
    return null;
  return view.cinema === "share" || view.cinema === "watch" ? view.cinema : null;
}

export function useStripState(): StripState {
  return useSyncExternalStore(subscribe, () => state);
}

/** Moves the strip and saves the choice for this device (FR-8.72). */
export function setStripPosition(position: CameraStripPosition): void {
  if (state.position === position) return;
  // Hidden shows no tiles, so nothing can stay large.
  change({ ...state, position, large: position === "hidden" ? null : state.large });
  const api = getApi();
  api
    .settingsGet()
    .then((current) => api.settingsUpdate({ ...current, cameraStripPosition: position }))
    .catch((error: unknown) => {
      console.error("Saving the camera strip position failed", error);
    });
}

/** Show large (F) on a strip tile: it takes the main picture's place (FR-8.73). */
export function showLarge(key: TileKey): void {
  if (state.large !== key) change({ ...state, large: key });
}

/** Escape, the tile going away or the strip ending: the main picture comes back. */
export function leaveLarge(): void {
  if (state.large !== null) change({ ...state, large: null });
}

/** Test helper: back to the start, the saved position unread. */
export function resetStripState(): void {
  loaded = false;
  change(initial);
}
