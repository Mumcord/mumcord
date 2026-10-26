import { describe, expect, it } from "vitest";
import type { Friend } from "../../api";
import { collectEntries } from "./directEntries";
import type { DirectChat, FollowState } from "./followStore";

const anna: DirectChat = {
  conversation: { kind: "private", certHash: "annahash" },
  key: "p:annahash",
  name: "Anna",
};
const bert: DirectChat = {
  conversation: { kind: "private", certHash: "berthash" },
  key: "p:berthash",
  name: "Bert",
};

function state(chats: DirectChat[], unread: Record<string, number>): FollowState {
  return { followed: [], chats, unread: new Map(Object.entries(unread)) };
}

function friend(certHash: string, onlineOn: number | null): Friend {
  return {
    certHash,
    name: certHash,
    addedAt: "2026-01-01T00:00:00Z",
    lastSeenAt: null,
    online: onlineOn === null ? null : { serverId: onlineOn, session: 5, channelId: 0 },
  };
}

const states = new Map([
  [1, state([anna, bert], { "p:annahash": 2, "p:berthash": 1 })],
  [2, state([anna, bert], { "p:annahash": 3, "p:berthash": 4 })],
]);

describe("direct message entries across servers", () => {
  it("shows a friend's chats once, on the server where they are online, with unread added up", () => {
    const entries = collectEntries(states, [1, 2], [friend("annahash", 2)]);
    const annaEntries = entries.filter((e) => e.chat.key === anna.key);
    expect(annaEntries).toEqual([{ serverId: 2, chat: anna, unread: 5 }]);
  });

  it("keeps an offline friend on the first server in the saved order", () => {
    const entries = collectEntries(states, [2, 1], [friend("annahash", null)]);
    expect(entries.filter((e) => e.chat.key === anna.key)).toEqual([
      { serverId: 2, chat: anna, unread: 5 },
    ]);
  });

  it("lists chats with people who are not friends separately per server", () => {
    const entries = collectEntries(states, [1, 2], [friend("annahash", 1)]);
    expect(entries.filter((e) => e.chat.key === bert.key)).toEqual([
      { serverId: 1, chat: bert, unread: 1 },
      { serverId: 2, chat: bert, unread: 4 },
    ]);
  });
});
