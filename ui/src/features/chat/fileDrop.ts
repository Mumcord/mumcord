/**
 * Image files dropped from the file manager onto the message input.
 *
 * In the desktop app the native drag-and-drop handler takes file drops (HTML5 drops of files are
 * unreliable there: WebKitGTK delivers only `file://` links without `File` objects). It reports
 * the drop position and the file paths; the composer checks whether the drop landed on it and asks
 * the engine for the contents of the dropped files.
 */
import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";
import { EngineError, getApi } from "../../api";

/** A point in physical pixels, relative to the top-left corner of the webview. */
export interface PhysicalPoint {
  x: number;
  y: number;
}

/** Whether a native drop position lies inside an element's box (CSS pixels). */
export function isOverRect(point: PhysicalPoint, rect: DOMRectReadOnly, scale: number): boolean {
  const x = point.x / scale;
  const y = point.y / scale;
  return (
    rect.width > 0 &&
    rect.height > 0 &&
    x >= rect.left &&
    x <= rect.right &&
    y >= rect.top &&
    y <= rect.bottom
  );
}

/** True inside the desktop app, where file drops arrive as native drag-and-drop events. */
export function hasNativeFileDrop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Subscribes to native file drags over the window; returns the unsubscribe function. */
export function onNativeFileDrop(handler: (event: DragDropEvent) => void): () => void {
  let stopped = false;
  let unlisten: (() => void) | undefined;
  getCurrentWebview()
    .onDragDropEvent((event) => {
      handler(event.payload);
    })
    .then(
      (fn) => {
        if (stopped) fn();
        else unlisten = fn;
      },
      (err: unknown) => {
        console.error("listening to file drops failed", err);
      },
    );
  return () => {
    stopped = true;
    unlisten?.();
  };
}

export type DropFailureReason = "notImage" | "sourceTooLarge" | "unreadable";

export interface DropFailure {
  name: string;
  reason: DropFailureReason;
}

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function failureReason(err: unknown): DropFailureReason {
  if (!(err instanceof EngineError)) return "unreadable";
  if (err.code === "invalidInput") return "notImage";
  if (err.code === "messageTooLarge") return "sourceTooLarge";
  return "unreadable";
}

function decodeBase64(data: string): Uint8Array<ArrayBuffer> {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** Reads dropped files through the engine, which accepts only images that were just dropped. */
export async function readDroppedFiles(
  paths: readonly string[],
): Promise<{ files: File[]; failures: DropFailure[] }> {
  const files: File[] = [];
  const failures: DropFailure[] = [];
  for (const path of paths) {
    try {
      const image = await getApi().readDroppedImage(path);
      files.push(new File([decodeBase64(image.data)], image.name, { type: image.mime }));
    } catch (err) {
      failures.push({ name: baseName(path), reason: failureReason(err) });
    }
  }
  return { files, failures };
}
