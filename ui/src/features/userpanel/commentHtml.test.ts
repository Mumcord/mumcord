import { describe, expect, it } from "vitest";
import { cleanCommentHtml, commentToSend, normalizeLink } from "./commentHtml";

describe("comment html", () => {
  it("keeps the formatting Mumble clients write", () => {
    const html = "<p><b>a</b> <i>b</i> <u>c</u></p><ul><li>one</li></ul><ol><li>two</li></ol>";
    expect(cleanCommentHtml(html)).toBe(html);
  });

  it("drops scripts, handlers and unsafe links", () => {
    const clean = cleanCommentHtml(
      '<a href="javascript:alert(1)" onclick="x()">l</a><script>x()</script><img src="http://h/x.png">',
    );
    expect(clean).not.toMatch(/javascript|onclick|script|img/u);
  });

  it("sends nothing for a comment without visible content", () => {
    expect(commentToSend("<p><b> </b></p><br>")).toBe("");
    expect(commentToSend("<b>x</b>")).toBe("<b>x</b>");
  });

  it("accepts web and mail links only", () => {
    expect(normalizeLink("example.org")).toBe("https://example.org");
    expect(normalizeLink("mailto:a@b.c")).toBe("mailto:a@b.c");
    expect(normalizeLink("javascript:alert(1)")).toBeNull();
    expect(normalizeLink("  ")).toBeNull();
  });
});
