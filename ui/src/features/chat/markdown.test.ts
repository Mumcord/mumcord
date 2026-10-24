import { describe, expect, it } from "vitest";
import { markdownToHtml, plainTextToHtml } from "./markdown";

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
