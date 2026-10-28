import { describe, expect, it } from "vitest";
import { cardToHtml, composeCards, extractCards, splitCardSource, type MessageCard } from "./cards";
import { markdownToHtml } from "./markdown";
import { sanitizeHtml } from "./sanitize";

const FULL =
  '[card title="CPU high on db-1" color=warning icon=chart image="https://grafana.test/r.png" url="https://grafana.test/d/db-1"]\n' +
  "Load **97 %** for 5 min\n" +
  "[/card]";

function card(partial: Partial<MessageCard>): MessageCard {
  return { title: null, color: null, icon: null, image: null, url: null, body: "", ...partial };
}

/** What a receiving Mumcord sees: the composed HTML through the chat sanitizer. */
function receive(source: string) {
  const html = composeCards(source, markdownToHtml);
  if (html === null) throw new Error("no card");
  return extractCards(sanitizeHtml(html));
}

describe("splitCardSource", () => {
  it("parses all fields", () => {
    const parts = splitCardSource(FULL);
    expect(parts).toEqual([
      {
        kind: "card",
        card: card({
          title: "CPU high on db-1",
          color: "warning",
          icon: "chart",
          image: "https://grafana.test/r.png",
          url: "https://grafana.test/d/db-1",
          body: "Load **97 %** for 5 min",
        }),
      },
    ]);
  });

  it("keeps text around cards and supports several cards", () => {
    const parts = splitCardSource(
      'before [card title=A]a[/card] mid [card title="B"][/card] after',
    );
    expect(parts.map((p) => (p.kind === "card" ? `card:${p.card.title ?? ""}` : p.text))).toEqual([
      "before ",
      "card:A",
      " mid ",
      "card:B",
      " after",
    ]);
  });

  it("ignores unknown attributes and drops invalid values", () => {
    const [part] = splitCardSource(
      '[card title=x color=purple icon="https://evil.test/i.png" size=9 image="javascript:alert(1)" url="data:text/html,x"]t[/card]',
    );
    expect(part).toEqual({ kind: "card", card: card({ title: "x", body: "t" }) });
  });

  it("accepts http and https links only", () => {
    const [ok] = splitCardSource("[card url=http://a.test/x image=https://b.test/i.png]t[/card]");
    expect(ok).toMatchObject({ card: { url: "http://a.test/x", image: "https://b.test/i.png" } });
    const [bad] = splitCardSource('[card url="ftp://a.test" image="https://"]t[/card]');
    expect(bad).toMatchObject({ card: { url: null, image: null } });
  });

  it("leaves broken or empty cards as text", () => {
    for (const source of [
      '[card title="unterminated]text[/card]',
      "[card title=x]no end",
      "[card]   [/card]",
      "[card color=error][/card]",
      "[card title]x[/card]",
    ]) {
      expect(splitCardSource(source)).toEqual([{ kind: "text", text: source }]);
    }
  });

  it("accepts a card with only a text", () => {
    expect(splitCardSource("[card]just text[/card]")).toEqual([
      { kind: "card", card: card({ body: "just text" }) },
    ]);
  });

  it("is case insensitive for the tag and the enum values", () => {
    expect(splitCardSource("[CARD Color=ERROR Icon=Bot]x[/CARD]")).toEqual([
      { kind: "card", card: card({ color: "error", icon: "bot", body: "x" }) },
    ]);
  });
});

describe("cardToHtml (fallback for official clients)", () => {
  it("shows the status as text, the title bold, then text, image and link", () => {
    const html = cardToHtml(
      card({
        title: "Disk <85 %>",
        color: "warning",
        image: "https://g.test/i.png",
        url: "https://g.test/d",
        body: "About <b>3 days</b>",
      }),
    );
    expect(html).toBe(
      '<div data-mumcord-card="v=1&amp;title=Disk+%3C85+%25%3E&amp;color=warning&amp;image=https%3A%2F%2Fg.test%2Fi.png&amp;url=https%3A%2F%2Fg.test%2Fd">' +
        "<b>[Warning] Disk &lt;85 %&gt;</b>" +
        '<div data-mumcord-card="body">About <b>3 days</b></div>' +
        'Image: <a href="https://g.test/i.png">g.test/i.png</a><br>' +
        '<a href="https://g.test/d">https://g.test/d</a></div>',
    );
  });

  it("omits the parts a card does not have", () => {
    expect(cardToHtml(card({ title: "T" }))).toBe(
      '<div data-mumcord-card="v=1&amp;title=T"><b>T</b></div>',
    );
  });
});

describe("composeCards", () => {
  it("returns null without a card", () => {
    expect(composeCards("hello **x**", markdownToHtml)).toBeNull();
    expect(composeCards("[card]", markdownToHtml)).toBeNull();
  });

  it("converts the Markdown inside and around cards", () => {
    const html = composeCards("**hi** [card title=T]a *b*[/card]", markdownToHtml);
    expect(html).toBe(
      "<strong>hi</strong>" +
        '<div data-mumcord-card="v=1&amp;title=T"><b>T</b><div data-mumcord-card="body">a <em>b</em></div></div>',
    );
  });

  it("escapes HTML typed into the card text", () => {
    const html = composeCards(
      '[card title=T]<img src=x onerror="alert(1)">[/card]',
      markdownToHtml,
    );
    expect(html).not.toContain("<img");
    expect(html).toContain("&lt;img");
  });
});

describe("extractCards", () => {
  it("round-trips BB code → wire HTML → model", () => {
    const { rest, cards } = receive(FULL);
    expect(rest).toBe("");
    expect(cards).toEqual([
      card({
        title: "CPU high on db-1",
        color: "warning",
        icon: "chart",
        image: "https://grafana.test/r.png",
        url: "https://grafana.test/d/db-1",
        body: "Load <strong>97 %</strong> for 5 min",
      }),
    ]);
  });

  it("keeps text around cards and the order of several cards", () => {
    const { rest, cards } = receive("On it: [card title=A]a[/card][card title=B]b[/card]");
    expect(rest).toBe("On it:");
    expect(cards.map((c) => c.title)).toEqual(["A", "B"]);
  });

  it("leaves messages without cards untouched", () => {
    const html = "<b>hi</b> [not a card]";
    expect(extractCards(html)).toEqual({ rest: html, cards: [] });
  });

  it("parses the marker strictly and ignores invalid values", () => {
    const html =
      '<div data-mumcord-card="title=X&amp;color=pink&amp;icon=%3Cimg%3E&amp;url=javascript%3Aalert(1)&amp;image=data%3Aimage%2Fpng%3Bbase64%2Cx&amp;future=1"><b>X</b></div>';
    expect(extractCards(sanitizeHtml(html)).cards).toEqual([card({ title: "X" })]);
  });

  it("keeps a marked card without title and text as normal HTML", () => {
    const html = '<div data-mumcord-card="color=error"><b>[Error]</b></div>';
    expect(extractCards(sanitizeHtml(html))).toEqual({ rest: sanitizeHtml(html), cards: [] });
  });

  it("does not let script in the marker or the body through", () => {
    const html =
      '<div data-mumcord-card="title=%3Cscript%3Ealert(1)%3C%2Fscript%3E"><div data-mumcord-card="body"><img src="x" onerror="alert(1)"><script>alert(2)</script>ok</div></div>';
    const { cards } = extractCards(sanitizeHtml(html));
    expect(cards).toEqual([card({ title: "<script>alert(1)</script>", body: "ok" })]);
  });

  it("renders typed BB code that arrives as text, e.g. from an official client", () => {
    const html = sanitizeHtml(
      'Alert:<br>[card title="Disk &amp; more" color=error url="https://a.test/?x=1&amp;y=2"]<br>About <b>3</b> days<br>[/card]',
    );
    const { rest, cards } = extractCards(html);
    expect(rest).toBe("Alert:");
    expect(cards).toEqual([
      card({
        title: "Disk & more",
        color: "error",
        url: "https://a.test/?x=1&y=2",
        body: "About <b>3</b> days",
      }),
    ]);
  });

  it("sanitizes typed BB code bodies and the remaining text", () => {
    const html = '[card title=T]<a href="javascript:alert(1)">x</a>[/card]<b>after</b>';
    const { rest, cards } = extractCards(sanitizeHtml(html));
    expect(cards[0]?.body).toBe('<a rel="noopener noreferrer">x</a>');
    expect(rest).toBe("<b>after</b>");
  });

  it("leaves broken typed BB code as text", () => {
    const html = "[card title=x]no end";
    expect(extractCards(html)).toEqual({ rest: html, cards: [] });
  });
});
