import { describe, expect, it } from "vitest";
import type { RegisteredUserEntry } from "../../api";
import {
  INACTIVE_DAYS,
  countsOf,
  filterRegistered,
  initials,
  isInactive,
  lastSeenText,
  searchRegistered,
  sortRegistered,
} from "./registeredFormat";

const DAY = 86_400_000;
const NOW = Date.parse("2026-11-07T12:00:00.000Z");

function account(patch: Partial<RegisteredUserEntry>): RegisteredUserEntry {
  return {
    userId: 1,
    name: "Tove",
    lastSeen: new Date(NOW - DAY).toISOString(),
    lastChannel: "Lobby",
    online: false,
    certHash: null,
    ...patch,
  };
}

const LIST = [
  account({ userId: 2, name: "Tove", online: true }),
  account({ userId: 1, name: "Astrid", lastSeen: new Date(NOW - 200 * DAY).toISOString() }),
  account({ userId: 3, name: "Brennan", lastSeen: null, lastChannel: null }),
  account({ userId: 4, name: "bjørn", lastChannel: "AFK" }),
];

describe("search", () => {
  it("matches the name and ignores case and spaces around the word", () => {
    expect(searchRegistered(LIST, "to").map((entry) => entry.name)).toEqual(["Tove"]);
    expect(searchRegistered(LIST, "b").map((entry) => entry.name)).toEqual(["Brennan", "bjørn"]);
    expect(searchRegistered(LIST, "  TOVE  ").map((entry) => entry.userId)).toEqual([2]);
    expect(searchRegistered(LIST, "")).toHaveLength(4);
    expect(searchRegistered(LIST, "nobody")).toEqual([]);
  });

  it("never matches the id: the tab searches names, as the spec says (FR-10.53)", () => {
    expect(searchRegistered(LIST, "2")).toEqual([]);
  });
});

describe("filters", () => {
  it("counts all, online and the 90-day inactive ones separately", () => {
    expect(countsOf(LIST, NOW)).toEqual({ all: 4, online: 1, inactive: 1 });
  });

  it("keeps only the connected accounts under Online", () => {
    expect(filterRegistered(LIST, "online", NOW).map((entry) => entry.name)).toEqual(["Tove"]);
  });

  it("keeps only accounts unused for 90 days or more under Inactive", () => {
    const exactly = account({
      userId: 9,
      lastSeen: new Date(NOW - INACTIVE_DAYS * DAY).toISOString(),
    });
    const almost = account({
      userId: 10,
      lastSeen: new Date(NOW - (INACTIVE_DAYS - 1) * DAY).toISOString(),
    });
    expect(isInactive(exactly, NOW)).toBe(true);
    expect(isInactive(almost, NOW)).toBe(false);
    expect(filterRegistered(LIST, "inactive", NOW).map((entry) => entry.name)).toEqual(["Astrid"]);
  });

  it("does not call an account inactive when the server says nothing about it (FR-10.5)", () => {
    expect(isInactive(account({ lastSeen: null }), NOW)).toBe(false);
    // A connected account is never inactive, whatever the server last reported.
    expect(
      isInactive(account({ online: true, lastSeen: new Date(NOW - 300 * DAY).toISOString() }), NOW),
    ).toBe(false);
  });
});

describe("sorting", () => {
  it("orders by name ignoring case, and puts the online account first", () => {
    const names = sortRegistered(LIST, { column: "name", ascending: true }).map((e) => e.name);
    expect(names).toEqual(["Tove", "Astrid", "bjørn", "Brennan"]);
    expect(
      sortRegistered(LIST, { column: "name", ascending: false })
        .filter((entry) => !entry.online)
        .map((entry) => entry.name),
    ).toEqual(["Brennan", "bjørn", "Astrid"]);
  });

  it("orders by id", () => {
    expect(
      sortRegistered(LIST, { column: "userId", ascending: true }).map((e) => e.userId),
    ).toEqual([1, 2, 3, 4]);
    expect(
      sortRegistered(LIST, { column: "userId", ascending: false }).map((e) => e.userId),
    ).toEqual([4, 3, 2, 1]);
  });

  it("sinks the accounts the server says nothing about to the bottom, in both directions", () => {
    const times = [
      account({ userId: 1, name: "oldest", lastSeen: new Date(NOW - 200 * DAY).toISOString() }),
      account({ userId: 2, name: "newer", lastSeen: new Date(NOW - 20 * DAY).toISOString() }),
      account({ userId: 3, name: "silent", lastSeen: null }),
    ];
    expect(
      sortRegistered(times, { column: "lastSeen", ascending: true }).map((e) => e.name),
    ).toEqual(["oldest", "newer", "silent"]);
    expect(
      sortRegistered(times, { column: "lastSeen", ascending: false }).map((e) => e.name),
    ).toEqual(["newer", "oldest", "silent"]);
  });

  it("orders by the last channel and keeps unknown channels last", () => {
    expect(
      sortRegistered(LIST, { column: "lastChannel", ascending: true }).map((e) => e.lastChannel),
    ).toEqual(["AFK", "Lobby", "Lobby", null]);
  });
});

describe("last seen", () => {
  const labels = { online: "Online now", unknown: "unknown" };

  it("reads Online now for a connected account", () => {
    expect(lastSeenText(account({ online: true, lastSeen: null }), "en", NOW, labels)).toBe(
      "Online now",
    );
  });

  it("says unknown when the server does not report the time (FR-10.5)", () => {
    expect(lastSeenText(account({ lastSeen: null }), "en", NOW, labels)).toBe("unknown");
  });

  it("shows the time alone for today, the day for yesterday, and days for a week or less", () => {
    const today = lastSeenText(
      account({ lastSeen: new Date(NOW - 3_600_000).toISOString() }),
      "en",
      NOW,
      labels,
    );
    // Today reads as a clock time only: no month, no weekday, no "ago".
    expect(today).not.toMatch(/Nov|ago|,/);
    expect(
      lastSeenText(account({ lastSeen: new Date(NOW - DAY).toISOString() }), "en", NOW, labels),
    ).toMatch(/^Yesterday, \d/);
    expect(
      lastSeenText(account({ lastSeen: new Date(NOW - 3 * DAY).toISOString() }), "en", NOW, labels),
    ).toBe("3 days ago");
  });

  it("shows day and month within the year, and the year for older accounts", () => {
    // Within the year the month and the day are enough; the year only comes back for older ones.
    expect(
      lastSeenText(
        account({ lastSeen: new Date(NOW - 100 * DAY).toISOString() }),
        "en",
        NOW,
        labels,
      ),
    ).toBe("Jul 30");
    expect(
      lastSeenText(
        account({ lastSeen: new Date("2025-03-01T08:00:00.000Z").toISOString() }),
        "en",
        NOW,
        labels,
      ),
    ).toBe("Mar 1, 2025");
  });
});

describe("the row's mark", () => {
  it("takes the first letters of the first two words", () => {
    expect(initials("Tove Eriksson")).toBe("TE");
    expect(initials("astrid")).toBe("A");
    expect(initials("  ")).toBe("?");
  });
});
