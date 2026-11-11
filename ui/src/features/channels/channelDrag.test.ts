import { describe, expect, it } from "vitest";
import type { ChannelInfo } from "../../api";
import { channelNamePath, dropZone, isWithin, planChannelDrop } from "./channelDrag";

const allow = {
  enter: true,
  textMessage: true,
  listen: true,
  registerSelf: false,
  speak: true,
  whisper: true,
  write: true,
  moveUsers: false,
  ban: false,
  kick: false,
  muteDeafen: false,
  makeChannel: false,
  makeTempChannel: false,
  linkChannel: false,
  register: false,
  resetUserContent: false,
};

function channel(id: number, parentId: number | null, name: string, position = 0): ChannelInfo {
  return {
    id,
    parentId,
    name,
    position,
    permissions: allow,
    description: null,
    hasDescription: false,
    maxUsers: null,
    temporary: false,
    links: [],
    enterRestricted: false,
  };
}

/** The mockup's tree: Gaming holds Raid (with Tanks and Healers) and Casual. */
function tree(...extra: ChannelInfo[]): Map<number, ChannelInfo> {
  return new Map(
    [
      channel(0, null, "Tiffoyn"),
      channel(1, 0, "Lobby", 0),
      channel(2, 0, "Gaming", 1),
      channel(3, 2, "Raid", 0),
      channel(4, 3, "Tanks"),
      channel(5, 3, "Healers"),
      channel(6, 2, "Casual", 1),
      channel(7, 0, "Music", 2),
      channel(8, 0, "AFK", 3),
      ...extra,
    ].map((c) => [c.id, c]),
  );
}

describe("dropZone", () => {
  it("places in the top and bottom quarter and moves into the middle", () => {
    expect(dropZone(0, 40)).toBe("above");
    expect(dropZone(9, 40)).toBe("above");
    expect(dropZone(10, 40)).toBe("into");
    expect(dropZone(30, 40)).toBe("into");
    expect(dropZone(31, 40)).toBe("below");
  });
});

describe("isWithin", () => {
  it("finds a channel itself and everything below it", () => {
    const channels = tree();
    expect(isWithin(channels, 2, 2)).toBe(true);
    expect(isWithin(channels, 4, 2)).toBe(true);
    expect(isWithin(channels, 7, 2)).toBe(false);
  });

  it("stops on a loop in a broken tree", () => {
    const channels = new Map([
      [1, channel(1, 2, "A")],
      [2, channel(2, 1, "B")],
    ]);
    expect(isWithin(channels, 1, 9)).toBe(false);
  });
});

describe("planChannelDrop", () => {
  it("moves a channel into another one, sub-channels included", () => {
    expect(planChannelDrop(tree(), 3, 7, "into")).toEqual({
      kind: "move",
      channelId: 3,
      fromParentId: 2,
      toParentId: 7,
      zone: "into",
      steps: [{ channelId: 3, parentId: 7, position: null }],
    });
  });

  it("refuses the channel itself and its own sub-channels", () => {
    expect(planChannelDrop(tree(), 2, 2, "into")).toEqual({ kind: "refused", reason: "self" });
    expect(planChannelDrop(tree(), 2, 4, "into")).toEqual({
      kind: "refused",
      reason: "subChannel",
    });
    expect(planChannelDrop(tree(), 2, 3, "above")).toEqual({
      kind: "refused",
      reason: "subChannel",
    });
  });

  it("does nothing where nothing would change", () => {
    // Already its parent, its own spot, or right after the sibling it already follows.
    expect(planChannelDrop(tree(), 3, 2, "into")).toEqual({ kind: "none" });
    expect(planChannelDrop(tree(), 3, 3, "above")).toEqual({ kind: "none" });
    expect(planChannelDrop(tree(), 8, 7, "below")).toEqual({ kind: "none" });
    // The root cannot be dragged.
    expect(planChannelDrop(tree(), 0, 7, "into")).toEqual({ kind: "none" });
  });

  it("does not move channels into a temporary channel", () => {
    const channels = tree({ ...channel(9, 0, "Temp", 4), temporary: true });
    expect(planChannelDrop(channels, 8, 9, "into")).toEqual({ kind: "none" });
  });

  it("drops onto the root's row into the root", () => {
    const plan = planChannelDrop(tree(), 3, 0, "above");
    expect(plan).toMatchObject({ kind: "move", toParentId: 0, zone: "into" });
  });

  it("reorders with only the dragged channel's position when one fits", () => {
    // AFK (3) above Music (2): position 2 sorts by name before Music.
    expect(planChannelDrop(tree(), 8, 7, "above")).toEqual({
      kind: "move",
      channelId: 8,
      fromParentId: 0,
      toParentId: 0,
      zone: "above",
      steps: [{ channelId: 8, parentId: null, position: 2 }],
    });
    // Lobby (0) below AFK (3): position 3 sorts by name after AFK.
    expect(planChannelDrop(tree(), 1, 8, "below")).toMatchObject({
      steps: [{ channelId: 1, parentId: null, position: 3 }],
    });
  });

  it("renumbers the siblings when no single position fits", () => {
    // All at 0, sorted by name: Alpha, Bravo, Charlie. Charlie above Bravo needs Bravo to move.
    const channels = new Map(
      [
        channel(0, null, "Root"),
        channel(1, 0, "Alpha"),
        channel(2, 0, "Bravo"),
        channel(3, 0, "Charlie"),
      ].map((c) => [c.id, c]),
    );
    expect(planChannelDrop(channels, 3, 2, "above")).toMatchObject({
      steps: [
        { channelId: 3, parentId: null, position: 1 },
        { channelId: 2, parentId: null, position: 2 },
      ],
    });
  });

  it("places a channel under a new parent when dropped between that parent's children", () => {
    // AFK above Casual (Gaming's second child, position 1): new parent Gaming, position 1 fits.
    expect(planChannelDrop(tree(), 8, 6, "above")).toEqual({
      kind: "move",
      channelId: 8,
      fromParentId: 0,
      toParentId: 2,
      zone: "above",
      steps: [{ channelId: 8, parentId: 2, position: 1 }],
    });
  });
});

describe("channelNamePath", () => {
  it("lists the names below the root", () => {
    expect(channelNamePath(tree(), 4)).toEqual(["Gaming", "Raid", "Tanks"]);
    expect(channelNamePath(tree(), 0)).toEqual([]);
  });
});
