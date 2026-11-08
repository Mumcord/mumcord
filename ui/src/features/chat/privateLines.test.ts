import { beforeEach, describe, expect, it } from "vitest";
import type { Conversation } from "../../api";
import { addPrivateLine, privateLinesFor, resetPrivateLines } from "./privateLines";

const CHANNEL: Conversation = { kind: "channel", channelId: 1 };
const NOTE = "Only you see this line";

function add(text: string, conversation: Conversation = CHANNEL): void {
  addPrivateLine({ serverId: 1, conversation, text, note: NOTE });
}

describe("the lines only this admin sees (FR-10.61)", () => {
  beforeEach(() => {
    resetPrivateLines();
  });

  it("keeps them in the conversation they were written for", () => {
    add("You muted Anna on the server.");
    expect(privateLinesFor(1, CHANNEL).map((line) => line.message.body)).toEqual([
      "You muted Anna on the server.",
    ]);
    // Another channel of the same server, and another server, hold nothing.
    expect(privateLinesFor(1, { kind: "channel", channelId: 2 })).toEqual([]);
    expect(privateLinesFor(2, CHANNEL)).toEqual([]);
  });

  it("puts a line at its time, with the note beside it (mockup 05-result-line)", () => {
    add("first");
    add("second");
    const lines = privateLinesFor(1, CHANNEL);
    expect(lines.map((line) => line.message.body)).toEqual(["first", "second"]);
    expect(lines[0]?.note).toBe(NOTE);
    // Shaped like a message so the list can place it, but without a sender of its own.
    expect(lines[0]?.message.own).toBe(true);
    expect(lines[0]?.message.senderName).toBe("");
    expect(lines[0]?.message.senderSession).toBeNull();
    expect(lines.map((line) => Date.parse(line.message.sentAt))).toEqual(
      [
        ...[Date.parse(lines[0]?.message.sentAt ?? ""), Date.parse(lines[1]?.message.sentAt ?? "")],
      ].sort((a, b) => a - b),
    );
  });

  it("escapes the text, which the message list draws as HTML", () => {
    add('You muted <script>"Anna"</script>.');
    expect(privateLinesFor(1, CHANNEL)[0]?.message.body).toBe(
      'You muted &lt;script&gt;"Anna"&lt;/script&gt;.',
    );
  });

  it("keeps the last lines of a conversation and forgets the oldest", () => {
    for (let index = 0; index < 205; index += 1) add(`line ${String(index)}`);
    const lines = privateLinesFor(1, CHANNEL);
    expect(lines).toHaveLength(200);
    expect(lines[0]?.message.body).toBe("line 5");
    expect(lines[199]?.message.body).toBe("line 204");
  });

  it("starts empty again, so nothing of one test is left in the next (FR-10.63)", () => {
    add("You muted Anna on the server.");
    resetPrivateLines();
    expect(privateLinesFor(1, CHANNEL)).toEqual([]);
  });
});
