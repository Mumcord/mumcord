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
