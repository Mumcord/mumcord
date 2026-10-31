/**
 * Time and quality figures as the two media views show them (issue #163). Pure, so the tests can
 * check the rounding: the same numbers the sharer's bar and the viewer's chips must agree on.
 */

/** Seconds as "7:12", or "1:02:03" past an hour. */
export function clockTime(seconds: number): string {
  const whole = Math.max(0, Math.round(seconds));
  const s = whole % 60;
  const m = Math.floor(whole / 60) % 60;
  const h = Math.floor(whole / 3600);
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${String(h)}:${String(m).padStart(2, "0")}:${ss}` : `${String(m)}:${ss}`;
}

/** Kilobit per second as the chip shows it: 42300 → "42.3 Mbit/s", 900 → "900 kbit/s". */
export function bitrate(kbps: number): string {
  if (kbps >= 1000) return `${(kbps / 1000).toFixed(1)} Mbit/s`;
  return `${String(Math.round(kbps))} kbit/s`;
}

/** Whole seconds between two player positions; a negative value means the person is ahead. */
export function behindSeconds(ownMs: number, theirsMs: number): number {
  return Math.round((ownMs - theirsMs) / 1000);
}

/** Milliseconds since `atMs` as the number of whole seconds. */
export function secondsSince(atMs: number, nowMs: number): number {
  return Math.max(0, Math.round((nowMs - atMs) / 1000));
}
