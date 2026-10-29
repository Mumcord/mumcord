/**
 * Engine images arrive as `data:` URLs (the webview never loads from the web). WebKitGTK copies an
 * image's whole address into the hit-test result it builds on every mouse move, so a GIF of a few
 * megabytes in `<img src>` kept a core busy while the pointer moved over it (issue #109). Such
 * images are shown through a short `blob:` URL of the same bytes instead; the `data:` URL stays the
 * image's identity for checks and saving.
 */
import { useCallback, useSyncExternalStore } from "react";

const DATA_URL = /^data:([^;,]+);base64,/;

/** One `blob:` URL per shown `data:` URL, shared by everything showing it. */
const entries = new Map<string, { url: string; users: number }>();
/** Live `blob:` URLs and the `data:` URLs they show, for {@link originalSrc}. */
const dataUrls = new Map<string, string>();

function objectUrl(src: string, type: string): string {
  const binary = atob(src.slice(src.indexOf(",") + 1));
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return URL.createObjectURL(new Blob([bytes], { type }));
}

function acquire(src: string, type: string) {
  const entry = entries.get(src);
  if (entry) {
    entry.users++;
    return;
  }
  const url = objectUrl(src, type);
  entries.set(src, { url, users: 1 });
  dataUrls.set(url, src);
}

function release(src: string) {
  const entry = entries.get(src);
  if (!entry || --entry.users > 0) return;
  entries.delete(src);
  dataUrls.delete(entry.url);
  URL.revokeObjectURL(entry.url);
}

/**
 * The address to put into `<img src>` for `src`: a `blob:` URL for a base64 `data:` URL, revoked
 * once nothing shows that image any more. `undefined` until that URL exists (right after mount),
 * so the image never briefly gets the long `data:` URL.
 */
export function useDisplaySrc(src: string | undefined): string | undefined {
  const type = src === undefined ? undefined : DATA_URL.exec(src)?.[1];
  const subscribe = useCallback(() => {
    if (src === undefined || type === undefined) return () => undefined;
    acquire(src, type);
    return () => {
      release(src);
    };
  }, [src, type]);
  return useSyncExternalStore(subscribe, () =>
    src === undefined || type === undefined ? src : entries.get(src)?.url,
  );
}

/** The `data:` URL behind a live address from {@link useDisplaySrc}; other addresses unchanged. */
export function originalSrc(src: string): string {
  return dataUrls.get(src) ?? src;
}
