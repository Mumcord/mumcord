/** "Last checked today at 14:32", in the UI language's own date and time format. */
export function lastChecked(
  atMs: number,
  nowMs: number,
  language: string,
  t: (key: string, options: Record<string, string>) => string,
): string {
  const at = new Date(atMs);
  const time = new Intl.DateTimeFormat(language, { timeStyle: "short" }).format(at);
  const dayStart = (ms: number) => new Date(ms).setHours(0, 0, 0, 0);
  const days = Math.round((dayStart(nowMs) - dayStart(atMs)) / 86_400_000);
  if (days === 0) return t("updates.lastCheckedToday", { time });
  if (days === 1) return t("updates.lastCheckedYesterday", { time });
  const date = new Intl.DateTimeFormat(language, { dateStyle: "medium" }).format(at);
  return t("updates.lastCheckedOn", { date, time });
}
