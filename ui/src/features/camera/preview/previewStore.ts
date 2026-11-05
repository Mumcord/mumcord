/** Which server the camera preview dialog is open for (FR-8.10). One dialog at a time. */
import { useSyncExternalStore } from "react";

export interface CameraPreviewTarget {
  serverId: number;
  /** Gets the focus back when the dialog closes. */
  opener: HTMLElement | null;
}

let target: CameraPreviewTarget | null = null;
const listeners = new Set<() => void>();

function set(next: CameraPreviewTarget | null): void {
  target = next;
  for (const listener of listeners) listener();
}

/**
 * Opens the preview dialog: turning the camera on always goes through it (FR-8.10). The dialog
 * starts the local preview itself; callers must not call `cameraPreviewStart`.
 */
export function openCameraPreview(serverId: number, opener: HTMLElement | null = null): void {
  if (target !== null) return;
  set({ serverId, opener });
}

export function closeCameraPreview(): void {
  set(null);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useCameraPreviewTarget(): CameraPreviewTarget | null {
  return useSyncExternalStore(subscribe, () => target);
}
