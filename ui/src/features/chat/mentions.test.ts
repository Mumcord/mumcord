import { describe, expect, it } from "vitest";
import type { User } from "../../api";
import {
  bodyMentions,
  findMentionQuery,
  groupMentioned,
  insertMention,
  mentionsName,
  rankGroupMentions,
  rankMentionCandidates,
} from "./mentions";

function user(session: number, name: string, channelId: number): User {
  return { session, name, channelId } as User;
}

describe("findMentionQuery", () => {
  it("finds the query behind an @ at the start or after whitespace", () => {
    expect(findMentionQuery("@Bo", 3)).toEqual({ start: 0, query: "Bo" });
    expect(findMentionQuery("hi @", 4)).toEqual({ start: 3, query: "" });
    expect(findMentionQuery("hi @Ann Le", 10)).toEqual({ start: 3, query: "Ann Le" });
  });

  it("ignores emails, lone @ followed by space and queries across lines", () => {
    expect(findMentionQuery("mail me@host", 12)).toBeNull();
    expect(findMentionQuery("at @ home", 9)).toBeNull();
    expect(findMentionQuery("@Bo\nnext", 8)).toBeNull();
    expect(findMentionQuery("no at sign", 5)).toBeNull();
  });
});

describe("rankMentionCandidates", () => {
  const users = [
    user(1, "Zoe", 1),
    user(2, "Jose", 2),
    user(3, "jonas", 1),
    user(4, "Joanna", 2),
    user(5, "Mia", 1),
  ];

  it("matches prefixes ignoring case and diacritics, current channel first", () => {
    const found = rankMentionCandidates(users, "jo", 1, 1).map((u) => u.name);
    expect(found).toEqual(["jonas", "Joanna", "Jose"]);
    expect(rankMentionCandidates([user(9, "José", 1)], "jos", 1, null)).toHaveLength(1);
  });

  it("leaves out the user and caps the list", () => {
    expect(rankMentionCandidates(users, "z", 1, 1)).toEqual([]);
    const many = Array.from({ length: 20 }, (_, i) => user(i + 10, `P${String(i)}`, 1));
    expect(rankMentionCandidates(many, "", 1, null)).toHaveLength(8);
  });

  it("matches names with spaces", () => {
    expect(rankMentionCandidates([user(7, "Ann Lee", 1)], "ann l", 1, null)).toHaveLength(1);
  });
});

describe("insertMention", () => {
  it("replaces @query with @Name and a space, caret behind it", () => {
    expect(insertMention("hi @Bo", 3, 6, "Bob")).toEqual({ text: "hi @Bob ", caret: 8 });
  });

  it("keeps names with spaces whole and does not double a following space", () => {
    expect(insertMention("@An rest", 0, 3, "Ann Lee")).toEqual({
      text: "@Ann Lee rest",
      caret: 9,
    });
  });
});

describe("mentionsName", () => {
  it("matches whole words, case and diacritics ignored", () => {
    expect(mentionsName("hey @zoe!", "Zoe")).toBe(true);
    expect(mentionsName("@Zoe", "Zoe")).toBe(true);
    expect(mentionsName("ok @José, look", "jose")).toBe(true);
    expect(mentionsName("see @Ann Lee now", "Ann Lee")).toBe(true);
    expect(mentionsName("done @Zoe.", "Zoe")).toBe(true);
  });

  it("has no false positives inside words, emails or longer names", () => {
    expect(mentionsName("mail zoe@zoe.com", "Zoe")).toBe(false);
    expect(mentionsName("@Zoey", "Zoe")).toBe(false);
    expect(mentionsName("@Zoe.com", "Zoe")).toBe(false);
    expect(mentionsName("Zoe without at", "Zoe")).toBe(false);
    expect(mentionsName("x@Zoe", "Zoe")).toBe(false);
    expect(mentionsName("@", "")).toBe(false);
  });
});

describe("bodyMentions", () => {
  it("reads the text of HTML, not its markup", () => {
    expect(bodyMentions("<p>hi <b>@Zoe</b></p>", ["Zoe"])).toBe(true);
    expect(bodyMentions('<a href="https://x/@Zoe">link</a>', ["Zoe"])).toBe(false);
    expect(bodyMentions("<p>@Old</p>", ["Zoe", "Old"])).toBe(true);
  });
});

describe("rankGroupMentions", () => {
  it("offers @channel before @all and filters by prefix", () => {
    expect(rankGroupMentions("")).toEqual(["channel", "all"]);
    expect(rankGroupMentions("A")).toEqual(["all"]);
    expect(rankGroupMentions("ch")).toEqual(["channel"]);
    expect(rankGroupMentions("bo")).toEqual([]);
  });
});

describe("groupMentioned", () => {
  const lobby = { kind: "channel", channelId: 1 } as const;
  const here = { ownChannel: 1, muted: false };

  it("counts @all and @channel in the user's own channel", () => {
    expect(groupMentioned("<p>@all restart at 22:00</p>", lobby, here)).toBe(true);
    expect(groupMentioned("@channel raid in 10", lobby, here)).toBe(true);
    expect(groupMentioned("@ALL, heads up", lobby, here)).toBe(true);
  });

  it("counts @channel only for people in the channel, @all also for listeners", () => {
    const listening = { ownChannel: 2, muted: false };
    expect(groupMentioned("@channel raid", lobby, listening)).toBe(false);
    expect(groupMentioned("@all restart", lobby, listening)).toBe(true);
  });

  it("ignores lookalikes, private chats and servers where it is turned off", () => {
    expect(groupMentioned("mail me@all.example", lobby, here)).toBe(false);
    expect(groupMentioned("@allison hi", lobby, here)).toBe(false);
    expect(groupMentioned("@channels", lobby, here)).toBe(false);
    expect(groupMentioned("@all hi", { kind: "privateSession", session: 3 }, here)).toBe(false);
    expect(groupMentioned("@all hi", lobby, { ownChannel: 1, muted: true })).toBe(false);
  });
});
