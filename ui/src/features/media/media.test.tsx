/** The media view store, the time and bitrate figures and the sync state of a player (issue #163). */
import { describe, expect, it } from "vitest";
import { behindSeconds, bitrate, clockTime, dimension } from "./format";
import { closeMedia, leaveCinema, mediaView, openMedia, resetMediaView, setTab } from "./mediaView";
import { syncState, outOfSync } from "../watchparty/syncState";

describe("figures", () => {
  it("shows times as minutes and seconds", () => {
    expect(clockTime(432)).toBe("7:12");
    expect(clockTime(3723)).toBe("1:02:03");
    expect(clockTime(-1)).toBe("0:00");
  });

  it("shows a picture size as width × height", () => {
    expect(dimension(1920, 1080)).toBe("1920×1080");
    expect(dimension(0, 0)).toBe("0×0");
  });

  it("shows a bitrate in megabits when it is over a megabit", () => {
    expect(bitrate(42300)).toBe("42.3 Mbit/s");
    expect(bitrate(900)).toBe("900 kbit/s");
  });

  it("counts whole seconds between two players", () => {
    expect(behindSeconds(12000, 8000)).toBe(4);
    expect(behindSeconds(8000, 12000)).toBe(-4);
  });
});

describe("sync state", () => {
  const report = { session: 2, positionMs: 8000, ageMs: 1000, playing: true, buffering: false };

  it("is in sync within two seconds", () => {
    expect(syncState(report, 9500, 0, 0)).toEqual({ kind: "synced" });
  });

  it("names a player that is behind", () => {
    expect(syncState(report, 12000, 0, 0)).toEqual({ kind: "behind", seconds: 4 });
    expect(syncState({ ...report, positionMs: 20000 }, 12000, 0, 0)).toEqual({
      kind: "ahead",
      seconds: 8,
    });
  });

  it("shows buffering first, and nothing for a report that is too old", () => {
    expect(syncState({ ...report, buffering: true }, 12000, 0, 0)).toEqual({ kind: "buffering" });
    expect(syncState({ ...report, ageMs: 60000 }, 12000, 0, 0)).toEqual({ kind: "unknown" });
  });

  it("counts the people the pill has to name", () => {
    expect(outOfSync([{ kind: "synced" }, { kind: "buffering" }, { kind: "unknown" }])).toBe(2);
  });
});

describe("media view", () => {
  it("opens the cinema view, and leaving it keeps the mini bar", () => {
    resetMediaView();
    openMedia("watch");
    expect(mediaView().cinema).toBe("watch");
    leaveCinema();
    expect(mediaView().cinema).toBeNull();
    expect(mediaView().mini.watch).toBe(true);
  });

  it("the last view opened takes the main column", () => {
    resetMediaView();
    openMedia("watch");
    openMedia("share");
    expect(mediaView().cinema).toBe("share");
    closeMedia("share");
    expect(mediaView().cinema).toBeNull();
    setTab("viewers");
    expect(mediaView().tab).toBe("viewers");
  });
});
