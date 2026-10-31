import { describe, expect, it } from "vitest";
import type { ChannelInfo, User } from "../../api";
import type { ServerState } from "../channels/serverStore";
import { channelPresence } from "./channelPresence";

const lobby: ChannelInfo = {
  id: 1,
  parentId: 0,
  name: "Lobby",
  position: 0,
  permissions: { enter: true, textMessage: true, listen: true, registerSelf: false },
  description: null,
  hasDescription: false,
  maxUsers: null,
};

function user(session: number, name: string, change: Partial<User> = {}): User {
  return {
    session,
    channelId: 1,
    name,
    certHash: null,
    registered: false,
    selfMute: false,
    selfDeaf: false,
    serverMute: false,
    serverDeaf: false,
    suppressed: false,
    listeningChannelIds: [],
    hasAvatar: false,
    avatarHash: null,
    hasComment: false,
    volume: 100,
    localMute: false,
    isMumcord: false,
    mumcordFeatures: [],
    ...change,
  };
}

function state(users: User[], talking: number[]): ServerState {
  return {
    connection: { state: "disconnected", error: null },
    connected: true,
    serverId: 1,
    selfSession: 1,
    info: null,
    channels: new Map([[lobby.id, lobby]]),
    users: new Map(users.map((u) => [u.session, u])),
    talking: new Set(talking),
    readOnly: false,
    rejoinChannelId: null,
    syncChannels: 0,
  };
}

const people = [
  user(1, "Zoe"),
  user(2, "freya"),
  user(3, "Astrid"),
  user(4, "Leif", { channelId: 2 }),
  user(5, "Sigrid", { channelId: 2, listeningChannelIds: [1] }),
];

describe("channelPresence", () => {
  it("counts the people in the channel, listeners included, like the member heading", () => {
    expect(channelPresence(state(people, []), { kind: "channel", channelId: 1 })).toEqual({
      count: 4,
      talkers: [],
    });
  });

  it("names the talkers in the channel by name, without listeners or people elsewhere", () => {
    const presence = channelPresence(state(people, [1, 2, 4, 5]), {
      kind: "channel",
      channelId: 1,
    });
    expect(presence?.talkers).toEqual(["freya", "Zoe"]);
  });

  it("has nothing to say about a private chat or an unknown channel", () => {
    const s = state(people, [1]);
    expect(channelPresence(s, { kind: "private", certHash: "abc" })).toBeNull();
    expect(channelPresence(s, { kind: "channel", channelId: 9 })).toBeNull();
    expect(channelPresence(s, null)).toBeNull();
  });
});
