/**
 * Which server the camera preview dialog (FR-8.10) is open for. The user-panel button and the
 * Apps menu open it; the global hotkey starts the preview in the engine (FR-8.20), so a preview
 * reported by `cameraChanged` opens it too. The dialog closes it on Cancel, Escape or once on air.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";

let target: number | null = null;
let boundApi: MumcordApi | undefined;
let unsubscribe: Unsubscribe | undefined;
const listeners = new Set<() => void>();

function set(next: number | null): void {
  if (target === next) return;
  target = next;
  for (const listener of listeners) listener();
}

function ensureBound(): void {
  const api = getApi();
  if (api === boundApi) return;
  unsubscribe?.();
  boundApi = api;
  unsubscribe = api.on("cameraChanged", ({ view }) => {
    if (view.state === "preview" && target === null) set(view.serverId);
  });
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Opens the preview dialog for `serverId`; the camera itself starts in the dialog. */
export function openCameraPreview(serverId: number): void {
  set(serverId);
}

export function closeCameraPreview(): void {
  set(null);
}

/** Test helper: closes the dialog and forgets the binding. */
export function resetCameraPreview(): void {
  unsubscribe?.();
  unsubscribe = undefined;
  boundApi = undefined;
  target = null;
}

/** The server the preview dialog is open for, or `null` while it is closed. */
export function useCameraPreviewTarget(): number | null {
  return useSyncExternalStore(subscribe, () => target);
}
