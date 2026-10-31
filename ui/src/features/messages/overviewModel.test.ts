import { describe, expect, it } from "vitest";
import type { Conversation, PmConversationPrefs } from "../../api";
import type { LastMessage } from "./conversationsStore";
import { buildRows, matches, unreadConversations, type RowsInput } from "./overviewModel";
import { timeLabel } from "./timeLabel";

const anna: Conversation = { kind: "private", certHash: "anna" };
const ben: Conversation = { kind: "private", certHash: "ben" };
const carl: Conversation = { kind: "private", certHash: "carl" };

function last(conversation: Conversation, at: number, text = "hi"): LastMessage {
  return { conversation, peerName: null, text, image: false, own: false, at };
}

function input(over: Partial<RowsInput>): RowsInput {
  return {
    chats: [],
    last: new Map(),
    prefs: [],
    friends: new Set(),
    liveName: () => undefined,
    serverName: (id) => (id === 1 ? "Retro LAN" : "Night Guild"),
    unknownName: "Unknown",
    now: 1_000,
    ...over,
  };
}

function prefs(certHash: string, change: Partial<PmConversationPrefs>): PmConversationPrefs {
  return { serverId: 1, certHash, pinned: false, mutedUntil: null, mutedForever: false, ...change };
}

describe("overview rows", () => {
  it("merges every server into one list, pinned first, then newest first", () => {
    const rows = buildRows(
      input({
        chats: [{ serverId: 2, conversation: carl, name: "Carl", unread: 2 }],
        last: new Map([
          [
            1,
            new Map([
              ["private:anna", { ...last(anna, 10), peerName: "Anna" }],
              ["private:ben", { ...last(ben, 30), peerName: "Ben" }],
            ]),
          ],
          [2, new Map([["private:carl", last(carl, 20)]])],
        ]),
        prefs: [prefs("anna", { pinned: true })],
      }),
    );
    expect(rows.map((r) => [r.name, r.serverName, r.pinned, r.unread])).toEqual([
      ["Anna", "Retro LAN", true, 0],
      ["Ben", "Retro LAN", false, 0],
      ["Carl", "Night Guild", false, 2],
    ]);
    expect(unreadConversations(rows)).toBe(1);
  });

  it("shows a friend once, on the server where they are online, with the counts added up", () => {
    const rows = buildRows(
      input({
        chats: [
          { serverId: 1, conversation: anna, name: "Anna", unread: 1 },
          { serverId: 2, conversation: anna, name: "Anna", unread: 2 },
        ],
        friends: new Set(["anna"]),
        liveName: (serverId) => (serverId === 2 ? "Anna (live)" : undefined),
      }),
    );
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ serverId: 2, name: "Anna (live)", online: true, unread: 3 });
  });

  it("counts a mute only until it ends, and forever until turned back on", () => {
    const at = (iso: string) =>
      buildRows(
        input({
          chats: [{ serverId: 1, conversation: anna, name: "Anna", unread: 1 }],
          prefs: [prefs("anna", { mutedUntil: iso })],
          now: Date.parse("2026-10-31T12:00:00Z"),
        }),
      )[0]?.muted;
    expect(at("2026-10-31T13:00:00Z")).toBe(true);
    expect(at("2026-10-31T11:00:00Z")).toBe(false);
    const forever = buildRows(
      input({
        chats: [{ serverId: 1, conversation: anna, name: "Anna", unread: 1 }],
        prefs: [prefs("anna", { mutedForever: true })],
      }),
    );
    expect(forever[0]?.muted).toBe(true);
  });

  it("never pins a chat without a certificate", () => {
    const session: Conversation = { kind: "privateSession", session: 7 };
    const rows = buildRows(
      input({ chats: [{ serverId: 1, conversation: session, name: null, unread: 0 }] }),
    );
    expect(rows[0]).toMatchObject({ certHash: null, pinned: false, name: "Unknown" });
  });

  it("searches names, servers and previews, ignoring case", () => {
    const [row] = buildRows(
      input({
        chats: [{ serverId: 1, conversation: anna, name: "Anna", unread: 0 }],
        last: new Map([[1, new Map([["private:anna", last(anna, 5, "LAN party Saturday")]])]]),
      }),
    );
    if (!row) throw new Error("no row");
    expect(matches(row, "anna")).toBe(true);
    expect(matches(row, "retro")).toBe(true);
    expect(matches(row, "PARTY")).toBe(true);
    expect(matches(row, "bob")).toBe(false);
  });
});

describe("time label", () => {
  const now = new Date(2026, 9, 31, 20, 0).getTime();
  it("shows the time today, Yesterday, the weekday this week and the date before", () => {
    expect(timeLabel(new Date(2026, 9, 31, 8, 5).getTime(), now, "en-GB", "Yesterday")).toBe(
      "08:05",
    );
    expect(timeLabel(new Date(2026, 9, 30, 23, 0).getTime(), now, "en-GB", "Yesterday")).toBe(
      "Yesterday",
    );
    expect(timeLabel(new Date(2026, 9, 26, 12, 0).getTime(), now, "en-GB", "Yesterday")).toBe(
      "Mon",
    );
    expect(timeLabel(new Date(2026, 8, 3, 12, 0).getTime(), now, "en-GB", "Yesterday")).toBe(
      "3 Sept",
    );
    expect(timeLabel(new Date(2025, 8, 3, 12, 0).getTime(), now, "en-GB", "Yesterday")).toBe(
      "3 Sept 2025",
    );
  });
});
