/**
 * Search, filter, sort and the time text of the Registered users tab (Epic 4, issue #256).
 *
 * Kept apart from the component so the rules can be tested on their own: the window has to stay
 * smooth with thousands of accounts (FR-10.53, §9), and the server's times arrive as RFC 3339 or as
 * nothing at all (FR-10.5).
 */
import type { RegisteredUserEntry } from "../../api";

export type RegisteredFilter = "all" | "online" | "inactive";
export type RegisteredColumn = "name" | "userId" | "lastSeen" | "lastChannel";
export interface RegisteredSort {
  column: RegisteredColumn;
  ascending: boolean;
}

/** How long an account may have been unused before the tab calls it inactive (issue #256). */
export const INACTIVE_DAYS = 90;
const DAY_MS = 86_400_000;

/** Accounts whose name contains `query`; an empty query keeps them all (FR-10.53). */
export function searchRegistered(
  entries: RegisteredUserEntry[],
  query: string,
): RegisteredUserEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return entries;
  return entries.filter((entry) => entry.name.toLowerCase().includes(needle));
}

/**
 * The three filters of the tab. "Inactive 90+ days" only counts accounts the server says something
 * about: an old server sends no last seen at all (FR-10.5), and silence is not inactivity.
 */
export function filterRegistered(
  entries: RegisteredUserEntry[],
  filter: RegisteredFilter,
  now: number,
): RegisteredUserEntry[] {
  if (filter === "all") return entries;
  if (filter === "online") return entries.filter((entry) => entry.online);
  return entries.filter((entry) => isInactive(entry, now));
}

/** Whether the account has been unused for at least [`INACTIVE_DAYS`]. */
export function isInactive(entry: RegisteredUserEntry, now: number): boolean {
  if (entry.online) return false;
  const seen = Date.parse(entry.lastSeen ?? "");
  return Number.isFinite(seen) && now - seen >= INACTIVE_DAYS * DAY_MS;
}

/** Order by one column; accounts the server says nothing about always sink to the bottom. */
export function sortRegistered(
  entries: RegisteredUserEntry[],
  sort: RegisteredSort,
): RegisteredUserEntry[] {
  const order = sort.ascending ? 1 : -1;
  const byText = (a: string | null, b: string | null) =>
    (a ?? "").localeCompare(b ?? "", undefined, { sensitivity: "base", numeric: true });
  return [...entries].sort((a, b) => {
    if (sort.column === "userId") return order * (a.userId - b.userId);
    if (sort.column === "lastSeen") {
      const left = Date.parse(a.lastSeen ?? "");
      const right = Date.parse(b.lastSeen ?? "");
      const leftKnown = Number.isFinite(left);
      const rightKnown = Number.isFinite(right);
      if (leftKnown !== rightKnown) return leftKnown ? -1 : 1;
      if (!leftKnown) return 0;
      return order * (left - right);
    }
    if (sort.column === "lastChannel") {
      // An account the server never placed in a channel has nothing to order by, so it goes last
      // whichever way the column turns.
      if ((a.lastChannel === null) !== (b.lastChannel === null)) {
        return a.lastChannel === null ? 1 : -1;
      }
      return order * byText(a.lastChannel, b.lastChannel);
    }
    // Online people first: the tab is mostly used to find someone who is here.
    if (a.online !== b.online) return a.online ? -1 : 1;
    return order * byText(a.name, b.name);
  });
}

/** Whether the server reports last seen or last channel at all; an old one says neither (FR-10.5). */
export function serverReports(entry: RegisteredUserEntry): boolean {
  return entry.lastSeen !== null || entry.lastChannel !== null;
}

/** How many accounts each filter holds, for the label of its chip. */
export function countsOf(
  entries: RegisteredUserEntry[],
  now: number,
): Record<RegisteredFilter, number> {
  return {
    all: entries.length,
    online: entries.filter((entry) => entry.online).length,
    inactive: entries.filter((entry) => isInactive(entry, now)).length,
  };
}

/**
 * What the tab shows for "last seen". An account that is connected reads "Online now"; one used
 * today reads its time; one used yesterday names the day and the time; one used this year reads as
 * a day and a month, and older ones carry the year too — the same shape the ban list uses, so the
 * two tabs are read the same way.
 */
export function lastSeenText(
  entry: RegisteredUserEntry,
  language: string,
  now: number,
  labels: { online: string; unknown: string },
): string {
  if (entry.online) return labels.online;
  const at = Date.parse(entry.lastSeen ?? "");
  if (!Number.isFinite(at)) return labels.unknown;
  const when = new Date(at);
  const today = new Date(now);
  const time = new Intl.DateTimeFormat(language, { timeStyle: "short" }).format(when);
  // Days are counted between local midnights, so the evening before reads as "yesterday" and not
  // as "1 day ago" only once 24 hours have passed.
  const midnight = (date: Date) =>
    new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const days = Math.round((midnight(today) - midnight(when)) / DAY_MS);
  if (days <= 0) return time;
  // "auto" is what turns one day back into the word for yesterday in every locale; the first
  // letter is raised because the phrase starts the cell.
  const relative = new Intl.RelativeTimeFormat(language, { numeric: "auto" });
  if (days === 1) {
    // Only yesterday carries the time: it is close enough to matter, and further back the day is
    // the whole story (mockup `registered-users-dark`).
    const words = relative.format(-1, "day");
    return `${words.charAt(0).toUpperCase()}${words.slice(1)}, ${time}`;
  }
  if (days < 7) return relative.format(-days, "day");
  const sameYear = when.getFullYear() === today.getFullYear();
  return new Intl.DateTimeFormat(language, {
    day: "numeric",
    month: "short",
    ...(sameYear ? {} : { year: "numeric" }),
  }).format(when);
}

/** Up to two initials for the row's mark, as the server bar does for a server. */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const letters = words.slice(0, 2).map((word) => Array.from(word)[0] ?? "");
  return letters.join("").toUpperCase() || "?";
}
