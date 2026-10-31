import { describe, expect, it } from "vitest";
import type { MessageRequest, PmRequests } from "../../api";
import {
  MESSAGE_ITEM_TTL_MS,
  buildItems,
  filterCounts,
  prune,
  requestItemId,
  unreadCount,
  visibleItems,
  type CentreState,
  type StoredItem,
} from "./centreModel";

const NOW = Date.parse("2026-10-31T12:00:00Z");

function mention(id: string, at: number, read = false): StoredItem {
  return {
    kind: "mention",
    id,
    serverId: 1,
    at,
    read,
    conversation: { kind: "channel", channelId: 1 },
    messageId: id,
    sender: "Anna",
    excerpt: "hi",
    group: false,
  };
}

const reply: StoredItem = {
  kind: "reply",
  id: "r",
  serverId: 1,
  at: NOW - 2000,
  read: false,
  conversation: { kind: "channel", channelId: 1 },
  messageId: "r",
  sender: "Maya",
  excerpt: "sure",
};
const invite: StoredItem = {
  kind: "invite",
  invite: "watchParty",
  id: "w",
  serverId: 1,
  at: NOW - 3000,
  read: false,
  sender: "Ben",
  channelId: 1,
};
const connection: StoredItem = {
  kind: "connection",
  id: "c",
  serverId: 2,
  at: NOW - 4000,
  read: true,
  text: "gone",
};

function request(id: number, receivedAt: string): MessageRequest {
  return {
    id,
    session: 5,
    name: "Priya",
    certHash: null,
    chatRequest: true,
    note: null,
    messages: [],
    receivedAt,
  };
}

function state(items: StoredItem[], read: string[] = [], dismissed: string[] = []): CentreState {
  return { items, readRequests: new Set(read), dismissedRequests: new Set(dismissed) };
}

describe("notification centre model", () => {
  const requests = new Map<number, PmRequests>([
    [
      1,
      {
        incoming: [
          request(7, new Date(NOW - 1000).toISOString()),
          request(8, new Date(NOW).toISOString()),
        ],
        sent: [],
      },
    ],
  ]);

  it("lists stored entries and open requests newest first", () => {
    const items = buildItems(state([invite, mention("m", NOW - 500), reply, connection]), requests);
    expect(items.map((item) => item.id)).toEqual([
      requestItemId(1, 8),
      "m",
      requestItemId(1, 7),
      "r",
      "w",
      "c",
    ]);
  });

  it("counts entries per filter; connection events only count under All", () => {
    const items = buildItems(state([invite, mention("m", NOW), reply, connection]), requests);
    expect(filterCounts(items)).toEqual({ all: 6, mentions: 2, requests: 2, invites: 1 });
    expect(visibleItems(items, "mentions").map((item) => item.kind)).toEqual(["mention", "reply"]);
    expect(visibleItems(items, "requests")).toHaveLength(2);
  });

  it("counts unread entries for the badge, including unread requests", () => {
    const items = buildItems(state([invite, mention("m", NOW, true), connection]), requests);
    expect(unreadCount(items)).toBe(3);
    const read = buildItems(state([invite], [requestItemId(1, 7)]), requests);
    expect(unreadCount(read)).toBe(2);
  });

  it("leaves dismissed requests out", () => {
    const items = buildItems(state([], [], [requestItemId(1, 8)]), requests);
    expect(items.map((item) => item.id)).toEqual([requestItemId(1, 7)]);
  });

  it("prunes mentions and replies after seven days, keeps the rest", () => {
    const old = NOW - MESSAGE_ITEM_TTL_MS - 1;
    const items = [
      mention("old", old),
      { ...reply, at: old },
      { ...invite, at: old },
      mention("new", NOW),
    ];
    expect(prune(items, NOW).map((item) => item.id)).toEqual(["w", "new"]);
    const fresh = [mention("new", NOW)];
    expect(prune(fresh, NOW)).toBe(fresh);
  });
});
