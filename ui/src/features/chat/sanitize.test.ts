import { describe, expect, it } from "vitest";
import { htmlToText, sanitizeHtml } from "./sanitize";

const PNG = "data:image/png;base64,iVBORw0KGgo=";

describe("sanitizeHtml", () => {
  it("keeps basic formatting", () => {
    const html =
      "<b>bold</b> <i>it</i> <code>x</code><br><ul><li>a</li></ul><blockquote>q</blockquote>";
    expect(sanitizeHtml(html)).toBe(html.replace("<br>", "<br>"));
  });

  it("removes scripts and event handlers", () => {
    expect(sanitizeHtml("<script>alert(1)</script>hi")).toBe("hi");
    const img = sanitizeHtml(`<img src="${PNG}" onerror="alert(1)">`);
    expect(img).toContain("<img");
    expect(img).not.toContain("onerror");
    expect(sanitizeHtml('<b onclick="x()">a</b>')).toBe("<b>a</b>");
  });

  it("drops style, forms, iframes, svg and target", () => {
    expect(sanitizeHtml('<span style="color:red">a</span>')).toBe("<span>a</span>");
    expect(sanitizeHtml('<form><input name="x"></form>ok')).toBe("ok");
    expect(sanitizeHtml('<iframe src="https://x.test"></iframe>ok')).toBe("ok");
    expect(sanitizeHtml("<svg><circle /></svg>ok")).toBe("ok");
    const link = sanitizeHtml('<a href="https://x.test" target="_blank">x</a>');
    expect(link).not.toContain("target");
  });

  it("allows only inline data images", () => {
    expect(sanitizeHtml('<img src="https://tracker.test/a.png">')).toBe("");
    expect(sanitizeHtml('<img src="data:image/svg+xml;base64,PHN2Zz4=">')).toBe("");
    expect(sanitizeHtml('<img src="data:text/html;base64,PGI+">')).toBe("");
    expect(sanitizeHtml(`<img src="${PNG}" alt="cat">`)).toBe(`<img src="${PNG}" alt="cat">`);
  });

  it("allows only http(s) and mailto links and adds rel", () => {
    expect(sanitizeHtml('<a href="https://x.test">x</a>')).toBe(
      '<a href="https://x.test" rel="noopener noreferrer">x</a>',
    );
    expect(sanitizeHtml('<a href="mailto:a@b.test">m</a>')).toContain('href="mailto:a@b.test"');
    for (const bad of ["javascript:alert(1)", "data:text/html,<b>x</b>", "file:///etc/passwd"]) {
      const out = sanitizeHtml(`<a href="${bad}">x</a>`);
      expect(out).not.toContain("href");
    }
    expect(sanitizeHtml('<a href="https://x.test" rel="opener">x</a>')).toContain(
      'rel="noopener noreferrer"',
    );
  });

  it("extracts readable text", () => {
    expect(htmlToText("<b>Hi</b><br>there <script>x</script>")).toBe("Hithere");
  });
});
