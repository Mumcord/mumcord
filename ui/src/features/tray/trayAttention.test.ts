import { describe, expect, it } from "vitest";
import type { DirectChat, FollowState } from "../follow/followStore";
import { MAX_RECENT_CHATS, trayAttention } from "./trayAttention";

function chat(name: string): DirectChat {
  return { conversation: { kind: "private", certHash: name }, key: `p:${name}`, name };
}

function state(chats: DirectChat[], unread: Record<string, number>): FollowState {
  return { followed: [], chats, unread: new Map(Object.entries(unread)) };
}

const nameOf = (entry: { chat: DirectChat }) => entry.chat.name ?? "?";

describe("trayAttention", () => {
  it("adds up unread and mentions, skipping servers with muted notifications", () => {
    const states = new Map([
      [1, state([], { "c:2": 3, "p:anna": 1 })],
      [2, state([], { "c:5": 4 })],
    ]);
    const mentions = new Map([
      [1, new Map([["c:2", 2]])],
      [2, new Map([["c:5", 1]])],
    ]);
    const result = trayAttention({
      states,
      mentions,
      servers: [
        { id: 1, notificationsMuted: false },
        { id: 2, notificationsMuted: true },
      ],
      friends: [],
      nameOf,
    });
    expect(result.unread).toBe(4);
    expect(result.mentions).toBe(2);
  });

  it("lists the newest chats first, unread ones before the rest, up to the limit", () => {
    const names = ["a", "b", "c", "d", "e", "f", "g"];
    const states = new Map([[1, state(names.map(chat), { "p:b": 2 })]]);
    const result = trayAttention({
      states,
      mentions: new Map(),
      servers: [{ id: 1, notificationsMuted: false }],
      friends: [],
      nameOf,
    });
    expect(result.chats.map((c) => c.name)).toEqual(["b", "g", "f", "e", "d"]);
    expect(result.chats).toHaveLength(MAX_RECENT_CHATS);
    expect(result.chats[0]).toEqual({
      serverId: 1,
      conversation: { kind: "private", certHash: "b" },
      name: "b",
      unread: 2,
    });
  });
});
