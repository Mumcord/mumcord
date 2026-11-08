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

  it("keeps text colours the way Mumble writes them and nothing else (issue #666)", () => {
    expect(cleanCommentHtml('<span style="color:#30a46c">a</span>')).toBe(
      '<span style="color:#30a46c">a</span>',
    );
    expect(cleanCommentHtml('<font color="#30A46C">a</font>')).toBe(
      '<font color="#30a46c">a</font>',
    );
    expect(cleanCommentHtml('<span style="color: rgb(48, 164, 108);">a</span>')).toBe(
      '<span style="color:#30a46c">a</span>',
    );
    expect(cleanCommentHtml('<span style="color:#abc">a</span>')).toBe(
      '<span style="color:#aabbcc">a</span>',
    );
    for (const unsafe of [
      '<span style="color:red;position:fixed">a</span>',
      '<span style="background:url(http://h/x.png)">a</span>',
      '<span style="color:expression(alert(1))">a</span>',
      '<p style="color:#30a46c">a</p>',
      '<font color="javascript:alert(1)" face="x" size="9">a</font>',
    ]) {
      expect(cleanCommentHtml(unsafe)).not.toMatch(/style|color=|face|size|url|expression/u);
    }
  });

  it("does not change HTML it has already cleaned", () => {
    const html =
      '<p><b>a</b> <span style="color: rgb(1, 2, 3)">b</span> <font color="#ABC">c</font></p>' +
      '<a href="https://a.org">l</a><img src="data:image/png;base64,AAAA">';
    const once = cleanCommentHtml(html);
    expect(cleanCommentHtml(once)).toBe(once);
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
