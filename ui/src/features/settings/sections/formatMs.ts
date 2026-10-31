/** Microseconds as milliseconds with at most one decimal, in the app language. */
export function formatMs(us: number, locale: string): string {
  return new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(us / 1000);
}
