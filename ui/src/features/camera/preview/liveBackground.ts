/**
 * The background the camera sends with right now (FR-8.40). The engine does not report it, so the
 * preview dialog and the room bar's picker share it here; until either has set it, it is the
 * saved default. Changing it here never saves it as the default.
 */
import { useEffect, useState, useSyncExternalStore } from "react";
import { getApi, type CameraBackground } from "../../../api";
import { BLUR_LIGHT } from "./previewLogic";

export interface LiveBackground {
  background: CameraBackground;
  blurStrength: number;
}

let live: LiveBackground | null = null;
const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Records the background the camera uses now, without telling the engine. */
export function setLiveBackground(next: LiveBackground): void {
  live = next;
  for (const listener of listeners) listener();
}

/** Switches the running camera to `next` (camera_set_background) without saving it. */
export function applyLiveBackground(next: LiveBackground): void {
  setLiveBackground(next);
  getApi()
    .cameraSetBackground(next.background, next.blurStrength)
    .catch((error: unknown) => {
      console.error("Changing the camera background failed", error);
    });
}

/** Forgets the live background (tests). */
export function resetLiveBackground(): void {
  live = null;
}

const FALLBACK: LiveBackground = { background: { kind: "blur" }, blurStrength: BLUR_LIGHT };

/** The live background, or the saved default while no camera has set one in this window. */
export function useLiveBackground(): LiveBackground {
  const current = useSyncExternalStore(subscribe, () => live);
  const [saved, setSaved] = useState<LiveBackground | null>(null);
  const known = current !== null;

  useEffect(() => {
    if (known) return;
    let active = true;
    getApi()
      .settingsGet()
      .then(
        (settings) => {
          if (active)
            setSaved({
              background: settings.cameraDefaultBackground,
              blurStrength: settings.cameraBlurStrength,
            });
        },
        (error: unknown) => {
          console.error("Loading the default camera background failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, [known]);

  return current ?? saved ?? FALLBACK;
}
