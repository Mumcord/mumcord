import { describe, expect, it } from "vitest";
import { markdownToHtml, plainTextToHtml } from "./markdown";
import { htmlToText } from "./sanitize";

describe("markdownToHtml", () => {
  it("converts inline Markdown without a wrapping paragraph", () => {
    expect(markdownToHtml("hey **there** and *you* `x`")).toBe(
      "hey <strong>there</strong> and <em>you</em> <code>x</code>",
    );
  });

  it("turns line breaks into <br>", () => {
    expect(markdownToHtml("a\nb")).toBe("a<br>b");
  });

  it("keeps paragraphs for multi-paragraph text", () => {
    expect(markdownToHtml("a\n\nb")).toBe("<p>a</p><p>b</p>");
  });

  it("makes safe links", () => {
    expect(markdownToHtml("[site](https://x.test)")).toBe(
      '<a href="https://x.test" rel="noopener noreferrer">site</a>',
    );
    expect(markdownToHtml("[bad](javascript:alert(1))")).not.toContain("javascript");
  });

  it("shows raw HTML as text", () => {
    const html = markdownToHtml('<img src=x onerror="alert(1)"> <b>hi</b>');
    expect(html).not.toContain("<img");
    expect(html).not.toContain("<b>");
    expect(html).toContain("&lt;b&gt;hi");
  });

  it("returns an empty string for blank input", () => {
    expect(markdownToHtml("   \n ")).toBe("");
  });

  it("escapes plain text for servers without HTML", () => {
    expect(plainTextToHtml(" <b>&</b> ")).toBe("&lt;b&gt;&amp;&lt;/b&gt;");
  });
});

describe("text emoticons and lone angle brackets (F22)", () => {
  it.each([">_>", ">.<", ">hello", "<_<", ">:(", "a > b"])("shows %s as typed", (text) => {
    const html = markdownToHtml(text);
    expect(html).not.toContain("<blockquote");
    expect(html).not.toContain("<em>");
    expect(htmlToText(html)).toBe(text);
  });
  it("turns lines starting with '> ' into one quote block (AC-2.49)", () => {
    expect(markdownToHtml("> one\n> two")).toBe("<blockquote><p>one<br>two</p></blockquote>");
    const mixed = markdownToHtml("> quoted\nreply");
    expect(mixed).toContain("<blockquote>");
    expect(mixed).toContain("reply");
    expect(markdownToHtml("> a\n>_>")).not.toBe(markdownToHtml("> a\n> _>"));
  });
  it("keeps a lone > on a later line as text", () => {
    const html = markdownToHtml("a\n>_>");
    expect(html).not.toContain("<blockquote");
    expect(html).toContain("&gt;_&gt;");
  });
  it("keeps Markdown italic with asterisks", () => {
    expect(markdownToHtml("*x*")).toBe("<em>x</em>");
  });
});
