import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";
import type { Friend } from "../../api";
import { presenceText, type PresenceLookup } from "./presence";

const templates: Record<string, string> = {
  online: "Online",
  onlineIn: "Online in {channel}",
  onlineOnServer: "Online on {server}",
  inChannelOnServer: "In {channel} · {server}",
};
const t = ((key: string, values: Record<string, string> = {}) =>
  (templates[key] ?? key).replace(
    /\{(\w+)\}/g,
    (_, name: string) => values[name] ?? "",
  )) as unknown as TFunction<"friends">;

const friend = (serverId: number, channelId: number): Friend => ({
  certHash: "h",
  name: "FriendyB",
  addedAt: "2026-01-01T00:00:00Z",
  lastSeenAt: null,
  online: { serverId, session: 7, channelId },
});

const lookup: PresenceLookup = {
  shownServerId: 1,
  serverName: (id) => (id === 2 ? "Bravo" : undefined),
  channelName: (id, channel) => (id === 2 && channel === 12 ? "Raid" : undefined),
};

describe("presenceText", () => {
  it("names the channel and server for a friend on another connected server", () => {
    expect(presenceText(t, friend(2, 12), "en", lookup)).toBe("In Raid · Bravo");
  });

  it("falls back to the server name while its channels are unknown", () => {
    expect(presenceText(t, friend(2, 99), "en", lookup)).toBe("Online on Bravo");
  });
});
