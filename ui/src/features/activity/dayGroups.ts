/**
 * The feed's day groups (FR-11.10): the events are already newest first, so consecutive events of
 * one local calendar day form one group, and the groups come in the newest day first.
 */
import type { TFunction } from "i18next";
import type { ActivityEvent } from "./activityStore";

const DAY_MS = 86_400_000;

/** Start of `at`'s local calendar day, so a day is a whole number days from another one. */
function startOfDay(at: number): number {
  const day = new Date(at);
  return new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime();
}

export interface DayGroup {
  /** Start of the day, which its label is built from. */
  day: number;
  events: ActivityEvent[];
}

export function groupByDay(events: readonly ActivityEvent[]): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const event of events) {
    const day = startOfDay(event.at);
    const last = groups[groups.length - 1];
    if (last !== undefined && last.day === day) last.events.push(event);
    else groups.push({ day, events: [event] });
  }
  return groups;
}

/** "Today", "Yesterday", then the date (FR-11.10). */
export function dayLabel(day: number, now: Date, locale: string, t: TFunction): string {
  const days = Math.round((startOfDay(now.getTime()) - day) / DAY_MS);
  if (days <= 0) return t("today");
  if (days === 1) return t("yesterday");
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(day);
}

/** A day group cut where the events from before this connection start (#594). */
export interface SplitGroup {
  group: DayGroup;
  /** This day starts with the earlier events, so the "Earlier" divider goes above its header. */
  before: boolean;
  /** The day's events of this connection, then its events from before it. */
  current: ActivityEvent[];
  earlier: ActivityEvent[];
}

/**
 * Finds the one place the "Earlier" divider goes: the first event from before this connection that
 * follows one of this connection. A log of only one kind has no divider.
 */
export function splitAtEarlier(groups: readonly DayGroup[]): SplitGroup[] {
  let seenCurrent = false;
  let divided = false;
  return groups.map((group) => {
    const index = divided ? -1 : group.events.findIndex((event) => event.earlier === true);
    const cut = index === -1 ? group.events.length : index;
    const split: SplitGroup = {
      group,
      before: !divided && seenCurrent && cut === 0,
      current: group.events.slice(0, cut),
      earlier: group.events.slice(cut),
    };
    if (cut > 0) seenCurrent = true;
    if (index !== -1 && seenCurrent) divided = true;
    return split;
  });
}
