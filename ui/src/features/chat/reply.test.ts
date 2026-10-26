import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../../api";
import {
  findOriginal,
  parseReply,
  quoteFor,
  quoteHtml,
  repliesToOwn,
  snippet,
  SNIPPET_CHARS,
} from "./reply";
import { sanitizeHtml } from "./sanitize";

function message(id: string, sender: string, body: string): ChatMessage {
  return {
    id,
    conversation: { kind: "channel", channelId: 1 },
    senderName: sender,
    senderCertHash: null,
    senderSession: null,
    own: false,
    sentAt: "2026-06-15T10:00:00.000Z",
    body,
    dismissedPreviews: [],
    syncedFrom: null,
  };
}

describe("reply quotes", () => {
  it("builds a readable quote for HTML and plain-text servers", () => {
    const quote = { name: "Maya <3", snippet: "a & b" };
    expect(quoteHtml(quote, true)).toBe("<blockquote><b>Maya &lt;3</b>: a &amp; b</blockquote>");
    expect(quoteHtml(quote, false)).toBe("&gt; Maya &lt;3: a &amp; b<br>");
  });

  it("round-trips its own HTML quote through sanitizing", () => {
    const html = sanitizeHtml(
      `${quoteHtml({ name: "Maya", snippet: "hi there" }, true)}<b>yes</b>`,
    );
    expect(parseReply(html)).toEqual({
      quote: { name: "Maya", snippet: "hi there" },
      rest: "<b>yes</b>",
    });
  });

  it("recognises the plain-text quote, also after the server turned it into a newline", () => {
    expect(parseReply(sanitizeHtml("&gt; Maya: hi<br>yes"))?.quote).toEqual({
      name: "Maya",
      snippet: "hi",
    });
    const fromServer = parseReply(sanitizeHtml("> Maya: hi there\nyes"));
    expect(fromServer?.quote).toEqual({ name: "Maya", snippet: "hi there" });
    expect(fromServer?.rest).toBe("yes");
  });

  it("does not take ordinary quotes or messages for replies", () => {
    expect(
      parseReply(sanitizeHtml("<blockquote><p><strong>Maya</strong>: hi</p></blockquote>")),
    ).toBeNull();
    expect(parseReply(sanitizeHtml("> just a quote"))).toBeNull();
    expect(parseReply("hello")).toBeNull();
  });

  it("keeps quoted HTML and scripts as plain text and caps the length", () => {
    const evil = `<blockquote><b>Eve</b>: &lt;script&gt;alert(1)&lt;/script&gt;${"x".repeat(500)}</blockquote>ok`;
    const reply = parseReply(sanitizeHtml(evil));
    expect(reply?.quote.snippet.startsWith("<script>alert(1)</script>")).toBe(true);
    expect(reply?.quote.snippet.length).toBe(SNIPPET_CHARS + 1);
    expect(reply?.rest).toBe("ok");
  });

  it("quotes the first characters of the text, without the original's own quote", () => {
    expect(snippet("a".repeat(150))).toBe(`${"a".repeat(SNIPPET_CHARS)}…`);
    const nested = message(
      "1",
      "Jonas",
      "<blockquote><b>Maya</b>: hi</blockquote>sure <i>thing</i>",
    );
    expect(quoteFor(nested, "Jonas", "Image")).toEqual({ name: "Jonas", snippet: "sure thing" });
    expect(
      quoteFor(message("2", "Maya", '<img src="data:image/png;base64,AAAA">'), "Maya", "Image"),
    ).toEqual({
      name: "Maya",
      snippet: "Image",
    });
  });

  it("finds the latest matching original before the reply", () => {
    const long = "b".repeat(150);
    const messages = [
      message("1", "Maya", "hi there"),
      message("2", "Jonas", "hi there"),
      message("3", "Maya", "hi there"),
      message("4", "Maya", long),
      message("5", "Jonas", "reply"),
    ];
    const nameAt = (index: number) => messages[index]?.senderName ?? "";
    const find = (name: string, text: string) =>
      findOriginal(messages, 4, { name, snippet: text }, nameAt, "Image");
    expect(find("Maya", "hi there")).toBe(2);
    expect(find("Jonas", "hi there")).toBe(1);
    expect(find("Maya", snippet(long))).toBe(3);
    expect(find("Maya", "gone")).toBeNull();
    // Only messages before the reply count.
    expect(findOriginal(messages, 1, { name: "Maya", snippet: "hi there" }, nameAt, "Image")).toBe(
      0,
    );
  });
});

describe("lone > is no reply quote (F22)", () => {
  it.each([
    ">_>",
    "&gt;_&gt;",
    ">.<",
    "&gt;.&lt;",
    "> text",
    "&gt; text",
    "> text<br>more",
    "&gt;:(",
    "a > b",
  ])("%s", (raw) => {
    expect(parseReply(sanitizeHtml(raw))).toBeNull();
  });
});

describe("a reply to an own message is a mention (FR-2.67)", () => {
  const own = (id: string, body: string): ChatMessage => ({
    ...message(id, "Me", body),
    own: true,
  });
  const reply = (quoted: string, text: string): ChatMessage =>
    message("r", "Bot", `${quoteHtml({ name: quoted, snippet: text }, true)}sure`);

  it("matches a quote of an own message in the loaded history", () => {
    expect(repliesToOwn(reply("Me", "lunch at noon?"), "Me", [own("1", "lunch at noon?")])).toBe(
      true,
    );
  });

  it("matches by the name alone when the original is not loaded", () => {
    expect(repliesToOwn(reply("Me", "old news"), "Me", [])).toBe(true);
    expect(repliesToOwn(message("p", "Bot", "&gt; Me: old news<br>yes"), "Me", [])).toBe(true);
  });

  it("does not match a quote of someone else", () => {
    expect(repliesToOwn(reply("Maya", "lunch at noon?"), "Me", [own("1", "lunch at noon?")])).toBe(
      false,
    );
    expect(repliesToOwn(message("x", "Bot", "Me: hi"), "Me", [])).toBe(false);
  });

  it("does not match a loaded original someone else sent under the user's name", () => {
    expect(repliesToOwn(reply("Me", "hello"), "Me", [message("1", "Me", "hello")])).toBe(false);
  });

  it("never counts the user's own reply", () => {
    const ownReply = { ...reply("Me", "lunch at noon?"), own: true };
    expect(repliesToOwn(ownReply, "Me", [own("1", "lunch at noon?")])).toBe(false);
  });
});
