/**
 * The camera strip next to a screen share or watch party (spec 08 §5.4, FR-8.71–8.74): where it
 * sits (Cameras beside / Above / Hidden, remembered per device as `camera_strip_position`). Which
 * tile is shown large is the room's own choice (`roomChoices.large`), the same in room and strip.
 */
import { useSyncExternalStore } from "react";
import { getApi, type CameraStripPosition } from "../../../api";
import { useMediaView, type StageId } from "../../media/mediaView";
import { usePopouts } from "../../media/popoutStore";
import { setLargeTile } from "./roomChoices";

export interface StripState {
  position: CameraStripPosition;
}

const initial: StripState = { position: "beside" };
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
  if (position === "hidden") setLargeTile(null);
  change({ ...state, position });
  const api = getApi();
  api
    .settingsGet()
    .then((current) => api.settingsUpdate({ ...current, cameraStripPosition: position }))
    .catch((error: unknown) => {
      console.error("Saving the camera strip position failed", error);
    });
}

/** Test helper: back to the start, the saved position unread. */
export function resetStripState(): void {
  loaded = false;
  change(initial);
}
