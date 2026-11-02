/** Pure helpers of the renderer, kept apart from the components for fast refresh and tests. */

/** "1:02:03", "4:05" or "0:09" for a number of seconds. */
export function formatRemaining(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const pad = (n: number) => String(n).padStart(2, "0");
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${String(h)}:${pad(m)}:${pad(s % 60)}` : `${String(m)}:${pad(s % 60)}`;
}

const encoder = new TextEncoder();

export function byteLength(text: string): number {
  return encoder.encode(text).length;
}

/**
 * The character counter of a byte-limited input: characters typed, and how many fit in total
 * if the rest is plain ASCII. People think in characters; the byte limit stays the hard rule.
 */
export function charCount(text: string, maxBytes: number): { used: number; max: number } {
  const used = Array.from(text).length;
  return { used, max: used + Math.max(0, maxBytes - byteLength(text)) };
}

/** The longest prefix of `text` within `max` UTF-8 bytes, never splitting a character. */
export function clampBytes(text: string, max: number): string {
  let out = "";
  for (const char of text) {
    if (byteLength(out + char) > max) break;
    out += char;
  }
  return out;
}

/** The row index the pointer is over, from the rows' vertical middles. */
export function dropIndex(middles: readonly number[], y: number): number {
  const index = middles.findIndex((middle) => y < middle);
  return index === -1 ? Math.max(middles.length - 1, 0) : index;
}
