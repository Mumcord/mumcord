const UNITS: readonly [Intl.RelativeTimeFormatUnit, number][] = [
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
];

/** "2 min ago" style text for an RFC 3339 time; anything under a minute reads as "now". */
export function relativeTime(iso: string, now: number, language: string): string {
  const format = new Intl.RelativeTimeFormat(language, { numeric: "auto", style: "short" });
  const seconds = Math.round((Date.parse(iso) - now) / 1000);
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return format.format(Math.trunc(seconds / size), unit);
  }
  return format.format(0, "second");
}
