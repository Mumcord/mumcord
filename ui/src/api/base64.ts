/**
 * Decode media without a temporary binary string or a JavaScript loop on newer webviews.
 * Older supported webviews keep the atob path; the native API is deliberately feature-detected.
 */
export function decodeBase64(data: string): Uint8Array<ArrayBuffer> {
  const bytes: typeof Uint8Array & {
    fromBase64?: (source: string) => Uint8Array<ArrayBuffer>;
  } = Uint8Array;
  if (bytes.fromBase64) return bytes.fromBase64(data);

  const binary = atob(data);
  const out = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index++) out[index] = binary.charCodeAt(index);
  return out;
}
