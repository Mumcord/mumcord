import { describe, expect, it } from "vitest";
import type { ChannelInfo, ServerInfo, User } from "../../api";
import {
  channelLink,
  channelPath,
  hasSavedPassword,
  permissionList,
  shortDescription,
  userCount,
} from "./channelModel";

function channel(id: number, parentId: number | null, name: string): ChannelInfo {
  return {
    id,
    parentId,
    name,
    position: 0,
    permissions: {
      enter: true,
      textMessage: true,
      listen: true,
      registerSelf: false,
      speak: true,
      whisper: true,
      write: false,
      moveUsers: false,
    },
    description: null,
    hasDescription: false,
    maxUsers: null,
    temporary: false,
    links: [],
    enterRestricted: false,
  };
}

const channels = new Map(
  [channel(0, null, "Root"), channel(1, 0, "Raid"), channel(2, 1, "Officers & Co")].map((c) => [
    c.id,
    c,
  ]),
);

function info(address: string | null, port: number | null): ServerInfo {
  return { serverAddress: address, serverPort: port } as ServerInfo;
}

describe("channel header model", () => {
  it("builds the path below the root", () => {
    expect(channelPath(channels, 2)).toEqual(["Raid", "Officers & Co"]);
    expect(channelPath(channels, 0)).toEqual([]);
    expect(channelPath(channels, 99)).toEqual([]);
  });

  it("survives a parent cycle from the server", () => {
    const looped = new Map([
      [1, { ...channel(1, 2, "A") }],
      [2, { ...channel(2, 1, "B") }],
    ]);
    expect(channelPath(looped, 1)).toEqual(["B", "A"]);
  });

  it("builds a mumble:// link without credentials and encodes names", () => {
    expect(channelLink(info("voice.example.org", 64738), ["Raid", "Officers & Co"])).toBe(
      "mumble://voice.example.org:64738/Raid/Officers%20%26%20Co?version=1.2.0",
    );
    expect(channelLink(info("::1", 64738), [])).toBe("mumble://[::1]:64738/?version=1.2.0");
    expect(channelLink(info(null, null), ["Raid"])).toBeNull();
  });

  it("turns the untrusted description into one plain line", () => {
    expect(shortDescription('<b>Raid</b><br>nights<script>alert("x")</script>')).toBe(
      "Raid nights",
    );
    expect(shortDescription("<p>Loot</p>\n<p>rules</p>")).toBe("Loot rules");
    expect(shortDescription(null)).toBe("");
  });

  it("counts the people in a channel", () => {
    const users = new Map([
      [1, { channelId: 2 } as User],
      [2, { channelId: 2 } as User],
      [3, { channelId: 1 } as User],
    ]);
    expect(userCount(users, 2)).toBe(2);
    expect(userCount(users, 0)).toBe(0);
  });

  it("finds a saved password only for a channel that still matches", () => {
    const saved = [
      { label: null, lastUsedAt: null, channels: [{ channelId: 2, path: ["Raid"], found: true }] },
      { label: null, lastUsedAt: null, channels: [{ channelId: 3, path: ["Old"], found: false }] },
    ];
    expect(hasSavedPassword(saved, 2)).toBe(true);
    expect(hasSavedPassword(saved, 3)).toBe(false);
    expect(hasSavedPassword([], 2)).toBe(false);
  });

  it("lists the own permissions in a fixed order", () => {
    const list = permissionList(channel(1, 0, "Raid").permissions);
    expect(list.map((p) => p.permission)).toEqual([
      "enter",
      "speak",
      "textMessage",
      "whisper",
      "listen",
      "write",
    ]);
    expect(list.find((p) => p.permission === "write")?.allowed).toBe(false);
  });
});
