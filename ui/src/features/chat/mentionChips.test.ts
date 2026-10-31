import { describe, expect, it } from "vitest";
import { linkMentions, mentionPeople, type MentionPerson } from "./mentionChips";

const people: MentionPerson[] = mentionPeople([
  { session: 1, name: "Anna" },
  { session: 2, name: "Anna Maria" },
  { session: 3, name: "José" },
] as Parameters<typeof mentionPeople>[0]);
const label = (name: string) => `${name}, mention`;

function chips(html: string): HTMLElement[] {
  const doc = new DOMParser().parseFromString(linkMentions(html, people, label), "text/html");
  return [...doc.querySelectorAll<HTMLElement>(".chat-mention")];
}

describe("linkMentions", () => {
  it("wraps people on the server, ignoring case and diacritics", () => {
    const found = chips("<p>hi @anna and @JOSE!</p>");
    expect(found.map((c) => c.textContent)).toEqual(["@anna", "@JOSE"]);
    expect(found[1]?.dataset["mentionSession"]).toBe("3");
    expect(found[0]?.getAttribute("aria-label")).toBe("Anna, mention");
  });

  it("prefers the longest name, spaces included", () => {
    const found = chips("<p>@Anna Maria and @Anna</p>");
    expect(found.map((c) => c.dataset["mentionSession"])).toEqual(["2", "1"]);
    expect(found[0]?.textContent).toBe("@Anna Maria");
  });

  it("leaves people who are not on the server as plain text", () => {
    expect(chips("<p>@Nobody and mail anna@example.com</p>")).toHaveLength(0);
    expect(linkMentions("<p>@Nobody</p>", people, label)).toBe("<p>@Nobody</p>");
  });

  it("skips code and links", () => {
    expect(chips('<pre><code>@Anna</code></pre><a href="https://x.test">@Anna</a>')).toHaveLength(
      0,
    );
  });

  it("highlights @all and @channel as plain group chips, also with nobody to link (#70)", () => {
    const html = linkMentions(
      "<p>@ALL and @channel, not @allison or me@all.example</p>",
      [],
      label,
    );
    const doc = new DOMParser().parseFromString(html, "text/html");
    const groups = [...doc.querySelectorAll<HTMLElement>(".chat-mention-group")];
    expect(groups.map((c) => c.textContent)).toEqual(["@ALL", "@channel"]);
    expect(groups[0]?.getAttribute("role")).toBeNull();
    expect(doc.body.textContent).toBe("@ALL and @channel, not @allison or me@all.example");
  });

  it("never turns message text into markup", () => {
    const html = linkMentions("<p>@Anna &lt;img src=x onerror=1&gt;</p>", people, label);
    expect(new DOMParser().parseFromString(html, "text/html").querySelector("img")).toBeNull();
  });
});
