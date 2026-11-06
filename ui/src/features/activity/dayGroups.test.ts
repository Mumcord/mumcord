/**
 * The feed's day groups (FR-11.10): the events arrive newest first, so consecutive events of one
 * local calendar day belong to one group, and the newest day comes first.
 */
import { describe, expect, it } from "vitest";
import { i18n } from "../../i18n";
import type { ActivityEvent } from "./activityStore";
import { dayLabel, groupByDay } from "./dayGroups";

const t = i18n.getFixedT("en", "activity");

/** A local time, so the day is the one the user sees. */
function at(year: number, month: number, day: number, hour: number, minute: number): number {
  return new Date(year, month, day, hour, minute).getTime();
}

function event(time: number, name: string): ActivityEvent {
  return { serverId: 1, at: time, kind: "connected", name, self: false };
}

const events = [
  event(at(2026, 10, 6, 14, 32), "Nia"),
  event(at(2026, 10, 6, 9, 5), "Ben"),
  event(at(2026, 10, 5, 23, 59), "Maya"),
  event(at(2026, 10, 4, 20, 0), "Leo"),
];

describe("day groups", () => {
  it("groups consecutive events of one local day, newest day first", () => {
    const groups = groupByDay(events);
    expect(groups.map((group) => group.day)).toEqual([
      new Date(2026, 10, 6).getTime(),
      new Date(2026, 10, 5).getTime(),
      new Date(2026, 10, 4).getTime(),
    ]);
    expect(groups[0]?.events).toEqual([events[0], events[1]]);
    expect(groups[1]?.events).toEqual([events[2]]);
    expect(groups[2]?.events).toEqual([events[3]]);
  });

  it("splits the list at a day boundary", () => {
    const justAfterMidnight = at(2026, 10, 6, 0, 1);
    const justBeforeMidnight = at(2026, 10, 5, 23, 59);
    expect(groupByDay([event(justAfterMidnight, "Nia"), event(justBeforeMidnight, "Ben")])).toEqual(
      [
        { day: new Date(2026, 10, 6).getTime(), events: [event(justAfterMidnight, "Nia")] },
        { day: new Date(2026, 10, 5).getTime(), events: [event(justBeforeMidnight, "Ben")] },
      ],
    );
  });

  it("labels a group Today, Yesterday, then the date", () => {
    const now = new Date(2026, 10, 6, 12, 0);
    const groups = groupByDay(events);
    expect(dayLabel(groups[0]?.day ?? 0, now, "en", t)).toBe("Today");
    expect(dayLabel(groups[1]?.day ?? 0, now, "en", t)).toBe("Yesterday");
    const date = groups[2]?.day ?? 0;
    expect(dayLabel(date, now, "en", t)).toBe(
      new Intl.DateTimeFormat("en", { dateStyle: "medium" }).format(date),
    );
  });
});
