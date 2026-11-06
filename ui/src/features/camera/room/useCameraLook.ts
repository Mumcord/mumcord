/**
 * How the room draws the own camera and the speaking frame: "Mirror my self-view" (FR-8.66,
 * default on) and "Reduce motion" (spec 08 §5.7: System follows the operating system, On and Off
 * override it). The settings record reaches this store through `PreferencesProvider`, which
 * passes every loaded or saved record, so the room needs no settings screen.
 */
import { useSyncExternalStore } from "react";
import type { Settings } from "../../../api";
import { reduceMotionOn } from "../../settings/camera/cameraSettings";

export type CameraLookSettings = Pick<Settings, "cameraMirrorSelf" | "cameraReduceMotion">;

export interface CameraLook {
  mirror: boolean;
  /** Static speaking frame and no layout animation, from the setting or the system. */
  reduceMotion: boolean;
}

const REDUCE_QUERY = "(prefers-reduced-motion: reduce)";

let current: CameraLookSettings = { cameraMirrorSelf: true, cameraReduceMotion: "system" };
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/** Called with every loaded or saved settings record (see `PreferencesProvider`). */
export function setCameraLookSettings(settings: CameraLookSettings): void {
  if (
    settings.cameraMirrorSelf === current.cameraMirrorSelf &&
    settings.cameraReduceMotion === current.cameraReduceMotion
  ) {
    return;
  }
  current = {
    cameraMirrorSelf: settings.cameraMirrorSelf,
    cameraReduceMotion: settings.cameraReduceMotion,
  };
  notify();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  // The system setting can change while the room is open.
  const query = typeof matchMedia === "function" ? matchMedia(REDUCE_QUERY) : null;
  query?.addEventListener("change", listener);
  return () => {
    listeners.delete(listener);
    query?.removeEventListener("change", listener);
  };
}

function systemReducesMotion(): boolean {
  return typeof matchMedia === "function" && matchMedia(REDUCE_QUERY).matches;
}

export function useCameraLook(): CameraLook {
  const mirror = useSyncExternalStore(subscribe, () => current.cameraMirrorSelf);
  const reduceMotion = useSyncExternalStore(subscribe, () =>
    reduceMotionOn(current.cameraReduceMotion, systemReducesMotion()),
  );
  return { mirror, reduceMotion };
}

/** Test helper: back to the defaults. */
export function resetCameraLook(): void {
  current = { cameraMirrorSelf: true, cameraReduceMotion: "system" };
  notify();
}
