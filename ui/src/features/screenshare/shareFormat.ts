/** Figures as the screen share views write them (issue #163); pure, so tests can check them. */

/** What one viewer gets: 1080, 60 → "1080p · 60 fps". */
export function qualityText(height: number, fps: number): string {
  return `${String(height)}p · ${String(fps)} fps`;
}

/** The clock time a share started, in the user's language: "21:04". */
export function clockOf(atMs: number, language: string): string {
  return new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(atMs);
}

/** How many bars of the five light up at a sound level 0–1. */
export function litBars(rms: number | null): number {
  if (rms === null) return 0;
  return Math.min(5, Math.ceil(Math.max(0, rms) * 5));
}
