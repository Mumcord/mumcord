/**
 * Engine images arrive as `data:` URLs (the webview never loads from the web). WebKitGTK copies an
 * image's whole address into the hit-test result it builds on every mouse move, so a GIF of a few
 * megabytes in `<img src>` kept a core busy while the pointer moved over it (issue #109). Such
 * images are shown through a short `blob:` URL of the same bytes instead; the `data:` URL stays the
 * image's identity for checks, saving and the viewer.
 *
 * Object URLs are kept for the app run, like the engine images they stand for (`loadInlineImage`
 * keeps those for the run as well), so repeated renders reuse one URL.
 */

const DATA_URL = /^data:([^;,]+);base64,/;

const blobUrls = new Map<string, string>();
const dataUrls = new Map<string, string>();

/** The address to put into `<img src>` for `src`: a `blob:` URL for a base64 `data:` URL. */
export function displaySrc(src: string): string {
  const known = blobUrls.get(src);
  if (known !== undefined) return known;
  const match = DATA_URL.exec(src);
  if (!match) return src;
  const binary = atob(src.slice(match[0].length));
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  const url = URL.createObjectURL(new Blob([bytes], { type: match[1] ?? "" }));
  blobUrls.set(src, url);
  dataUrls.set(url, src);
  return url;
}

/** The `data:` URL behind an address from {@link displaySrc}; other addresses unchanged. */
export function originalSrc(src: string): string {
  return dataUrls.get(src) ?? src;
}
