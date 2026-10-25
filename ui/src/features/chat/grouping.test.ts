import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../../api";
import { daysAgo, GROUP_GAP_MS, messageLayout } from "./grouping";

function message(sender: string, sentAt: Date, patch: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: `${sender}-${String(sentAt.getTime())}`,
    conversation: { kind: "channel", channelId: 1 },
    senderName: sender,
    senderCertHash: `${sender}-hash`,
    senderSession: null,
    own: false,
    sentAt: sentAt.toISOString(),
    body: "hi",
    dismissedPreviews: [],
    syncedFrom: null,
    ...patch,
  };
}

/** Local time on 2026-10-24 (month index 9). */
function at(hours: number, minutes: number, day = 24): Date {
  return new Date(2026, 9, day, hours, minutes);
}

describe("messageLayout", () => {
  it("groups one sender's messages within the time window", () => {
    const layout = messageLayout([
      message("Maya", at(10, 0)),
      message("Maya", at(10, 4)),
      message("Maya", at(10, 8)),
    ]);
    expect(layout.map((l) => l.start)).toEqual([true, false, false]);
    expect(layout.map((l) => l.newDay)).toEqual([true, false, false]);
  });

  it("starts a new group after a gap longer than the window", () => {
    const first = at(10, 0);
    const exactly = new Date(first.getTime() + GROUP_GAP_MS);
    const later = new Date(exactly.getTime() + GROUP_GAP_MS + 1000);
    const layout = messageLayout([
      message("Maya", first),
      message("Maya", exactly),
      message("Maya", later),
    ]);
    expect(layout.map((l) => l.start)).toEqual([true, false, true]);
  });

  it("starts a new group when the sender changes", () => {
    const layout = messageLayout([
      message("Maya", at(10, 0)),
      message("Jonas", at(10, 1)),
      message("Jonas", at(10, 2), { own: true }),
      message("Me", at(10, 3), { own: true }),
    ]);
    expect(layout.map((l) => l.start)).toEqual([true, true, true, false]);
  });

  it("starts a new group and a new day at midnight, even within the window", () => {
    const layout = messageLayout([message("Maya", at(23, 58, 23)), message("Maya", at(0, 1, 24))]);
    expect(layout).toEqual([
      { start: true, newDay: true },
      { start: true, newDay: true },
    ]);
  });
});

describe("daysAgo", () => {
  it("counts local calendar days, not 24-hour spans", () => {
    expect(daysAgo(at(0, 5), at(23, 50))).toBe(0);
    expect(daysAgo(at(23, 50, 23), at(0, 5))).toBe(1);
    expect(daysAgo(at(12, 0, 20), at(12, 0))).toBe(4);
  });
});
