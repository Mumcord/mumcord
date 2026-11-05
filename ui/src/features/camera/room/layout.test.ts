/** Layout rules of the camera room (spec 08 FR-8.55, FR-8.63, FR-8.64) and the FR-8.69 bundling. */
import { afterEach, describe, expect, it, vi } from "vitest";
import { BUNDLE_MS, bundleChanges, createAnnouncer } from "./announcements";
import {
  fitGridWidth,
  gridShape,
  heightStep,
  speakerTile,
  stableOrder,
  withPinnedFirst,
} from "./layout";

describe("grid shape", () => {
  it.each([
    [1, 1, 1],
    [2, 2, 1],
    [3, 2, 2],
    [4, 2, 2],
    [5, 3, 2],
    [6, 3, 2],
    [7, 3, 3],
    [9, 3, 3],
  ])("%i cameras make %i × %i", (count, cols, rows) => {
    expect(gridShape(count)).toEqual({ cols, rows });
  });

  it("letterboxes 16:9 tiles into the area", () => {
    // Wide and short: the height decides. Two rows of 90 px tiles are 160 px wide each.
    expect(fitGridWidth({ cols: 2, rows: 2 }, 1000, 180, 0)).toBeCloseTo(320);
    // Narrow and tall: the width decides.
    expect(fitGridWidth({ cols: 2, rows: 2 }, 400, 2000, 10)).toBeCloseTo(400);
  });
});

describe("tile order", () => {
  it("keeps the order in which cameras turned on, new ones last", () => {
    const first = stableOrder([], [5, 3]);
    expect(first).toEqual([5, 3]);
    // The engine orders by session; the room keeps the turn-on order.
    expect(stableOrder(first, [2, 3, 5])).toEqual([5, 3, 2]);
    expect(stableOrder([5, 3, 2], [2, 5])).toEqual([5, 2]);
  });

  it("returns the same list when nothing changed", () => {
    const order = [4, "self"] as const;
    expect(stableOrder(order, ["self", 4])).toBe(order);
  });

  it("moves pinned tiles first in pin order", () => {
    expect(withPinnedFirst([1, 2, 3, "self"], [3, "self"])).toEqual([3, "self", 1, 2]);
    expect(withPinnedFirst([1, 2], [9])).toEqual([1, 2]);
  });
});

describe("speaker", () => {
  it("prefers the pin, then the last speaker, then who was shown, then the first tile", () => {
    expect(speakerTile([1, 2, 3], [3], 2, 1)).toBe(3);
    expect(speakerTile([1, 2, 3], [], 2, 1)).toBe(2);
    expect(speakerTile([1, 2, 3], [], 9, 1)).toBe(1);
    expect(speakerTile([1, 2, 3], [], null, null)).toBe(1);
    expect(speakerTile([], [], null, null)).toBeNull();
  });
});

describe("tile height", () => {
  it("rounds up to 180 / 360 / 540 / 720 device pixels", () => {
    expect(heightStep(50)).toBe(180);
    expect(heightStep(181)).toBe(360);
    expect(heightStep(540)).toBe(540);
    expect(heightStep(2000)).toBe(720);
  });
});

describe("camera announcements", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("bundles by direction and keeps only each person's last change", () => {
    expect(
      bundleChanges([
        { name: "Jonas", on: true },
        { name: "Maya", on: true },
        { name: "Ben", on: false },
        { name: "Maya", on: false },
      ]),
    ).toEqual([
      { on: true, names: ["Jonas"] },
      { on: false, names: ["Ben", "Maya"] },
    ]);
  });

  it("speaks the changes of 2 s at once", () => {
    vi.useFakeTimers();
    const spoken = vi.fn();
    const announcer = createAnnouncer(spoken);
    announcer.push({ name: "Jonas", on: true });
    vi.advanceTimersByTime(BUNDLE_MS - 100);
    announcer.push({ name: "Maya", on: true });
    expect(spoken).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    expect(spoken).toHaveBeenCalledWith([{ on: true, names: ["Jonas", "Maya"] }]);
    announcer.push({ name: "Ben", on: false });
    announcer.cancel();
    vi.advanceTimersByTime(BUNDLE_MS);
    expect(spoken).toHaveBeenCalledTimes(1);
  });
});
