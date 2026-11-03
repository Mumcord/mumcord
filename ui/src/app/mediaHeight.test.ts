import { describe, expect, it } from "vitest";
import {
  heightForDrag,
  heightForKey,
  loadMediaHeight,
  mediaLimits,
  percentOf,
  saveMediaHeight,
  shownHeight,
  snapPoints,
  type MediaLimits,
  type MediaRoom,
} from "./mediaHeight";

/** A 900 px chat column at the default text size with 60 px above the picture. */
const room: MediaRoom = { column: 900, overhead: 60, remPx: 16 };
const limits = mediaLimits(room) as MediaLimits;

describe("media area limits (FR-7.63)", () => {
  it("is 20 % to 70 % of the chat column on a tall window", () => {
    expect(limits).toEqual({ min: 180, max: 630 });
  });

  it("never goes under 9 rem", () => {
    // 20 % of 720 px is 144 px, 9 rem is 144 px at 16 px; with 18 px text 9 rem wins.
    expect(mediaLimits({ column: 720, overhead: 60, remPx: 18 })?.min).toBe(162);
  });

  it("leaves the chat 13 rem below the handle", () => {
    // 70 % would be 420 px; 600 − 208 − 60 leaves only 332 px.
    expect(mediaLimits({ column: 600, overhead: 60, remPx: 16 })?.max).toBe(332);
  });

  it("has no room for a handle on a short chat column (FR-7.71)", () => {
    expect(mediaLimits({ column: 440, overhead: 60, remPx: 16 })).toBeNull();
    // Large text makes the same window too short as well.
    expect(mediaLimits({ column: 600, overhead: 60, remPx: 24 })).toBeNull();
  });

  it("shrinks a stored height that no longer fits, without changing it", () => {
    expect(shownHeight(0.9, room, limits)).toBe(630);
    expect(shownHeight(0.05, room, limits)).toBe(180);
    expect(shownHeight(0.5, room, limits)).toBe(450);
  });
});

describe("dragging (FR-7.64, FR-7.69)", () => {
  it("snaps to 1/3, 1/2 and 2/3 within 12 px", () => {
    expect(snapPoints(room, limits)).toEqual([300, 450, 600]);
    expect(heightForDrag(311, room, limits, false)).toEqual({
      px: 300,
      snapped: true,
      collapse: false,
    });
    expect(heightForDrag(463, room, limits, false).px).toBe(463);
  });

  it("does not snap while Alt is held", () => {
    expect(heightForDrag(445, room, limits, true)).toEqual({
      px: 445,
      snapped: false,
      collapse: false,
    });
  });

  it("stops hard at the limits without a snap there", () => {
    expect(heightForDrag(1000, room, limits, false)).toEqual({
      px: 630,
      snapped: false,
      collapse: false,
    });
    expect(heightForDrag(150, room, limits, false).px).toBe(180);
  });

  it("offers to minimise below about 12 %", () => {
    expect(heightForDrag(120, room, limits, false)).toMatchObject({ px: 180, collapse: false });
    expect(heightForDrag(100, room, limits, false)).toMatchObject({ px: 180, collapse: true });
  });
});

describe("keyboard (FR-7.67)", () => {
  it("moves 2 rem with the arrows, the handle going the arrow's way", () => {
    expect(heightForKey("ArrowDown", 400, room, limits)).toBe(432);
    expect(heightForKey("ArrowUp", 400, room, limits)).toBe(368);
    expect(heightForKey("ArrowUp", 190, room, limits)).toBe(180);
  });

  it("jumps between snap points and the limits", () => {
    expect(heightForKey("PageDown", 400, room, limits)).toBe(450);
    expect(heightForKey("PageDown", 450, room, limits)).toBe(600);
    expect(heightForKey("PageDown", 600, room, limits)).toBe(630);
    expect(heightForKey("PageUp", 450, room, limits)).toBe(300);
    expect(heightForKey("PageUp", 300, room, limits)).toBe(180);
    expect(heightForKey("Home", 400, room, limits)).toBe(180);
    expect(heightForKey("End", 400, room, limits)).toBe(630);
  });

  it("resets with Enter and ignores other keys", () => {
    expect(heightForKey("Enter", 400, room, limits)).toBe("auto");
    expect(heightForKey("a", 400, room, limits)).toBeNull();
  });

  it("speaks whole percent of the chat column", () => {
    expect(percentOf(300, room)).toBe(33);
  });
});

describe("remembered height (FR-7.70)", () => {
  it("starts at Auto and keeps a saved share", () => {
    localStorage.clear();
    expect(loadMediaHeight()).toBe("auto");
    saveMediaHeight(0.4);
    expect(loadMediaHeight()).toBe(0.4);
    saveMediaHeight("auto");
    expect(loadMediaHeight()).toBe("auto");
  });

  it("falls back to Auto on broken values", () => {
    localStorage.setItem("mumcord.mediaHeight", "3");
    expect(loadMediaHeight()).toBe("auto");
    localStorage.setItem("mumcord.mediaHeight", "{");
    expect(loadMediaHeight()).toBe("auto");
    localStorage.clear();
  });
});
