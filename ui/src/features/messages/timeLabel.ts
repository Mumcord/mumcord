/** Time of a conversation's newest message, like chat apps show it in their lists. */

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

const DAY_MS = 86_400_000;

/**
 * Today: the time ("20:44"); yesterday: `yesterday`; within the last week: the weekday ("Sun");
 * older: the date ("12 Oct"), with the year when it is not this year.
 */
export function timeLabel(at: number, now: number, locale: string, yesterday: string): string {
  const days = Math.round((startOfDay(now) - startOfDay(at)) / DAY_MS);
  if (days <= 0) return new Intl.DateTimeFormat(locale, { timeStyle: "short" }).format(at);
  if (days === 1) return yesterday;
  if (days < 7) return new Intl.DateTimeFormat(locale, { weekday: "short" }).format(at);
  const sameYear = new Date(at).getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    ...(sameYear ? {} : { year: "numeric" }),
  }).format(at);
}
