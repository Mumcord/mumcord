import { describe, expect, it } from "vitest";
import { highlightCode } from "./highlight";
import { markdownToHtml } from "./markdown";
import { sanitizeHtml } from "./sanitize";

/** What a received message goes through before it is shown. */
const render = (html: string) => highlightCode(sanitizeHtml(html));

function parse(html: string): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = html;
  return container;
}

const JS = "const answer = 42;\nfunction greet(name) {\n  return `hi ${name}`;\n}";

describe("highlightCode", () => {
  it("highlights a fenced block with a language tag", () => {
    const sent = markdownToHtml("```js\n" + JS + "\n```");
    expect(sent).toMatch(/^<pre><code>/);
    const code = parse(render(sent)).querySelector("pre > code");
    expect(code?.classList.contains("hljs")).toBe(true);
    expect(code?.querySelector(".hljs-keyword")?.textContent).toBe("const");
    expect(code?.textContent).toBe(JS + "\n");
  });

  it("highlights a fenced block without a language tag", () => {
    const code = parse(render(markdownToHtml("```\n" + JS + "\n```"))).querySelector("code");
    expect(code?.querySelector(".hljs-keyword")).not.toBeNull();
  });

  it("highlights inline code that looks like code", () => {
    const html = render(markdownToHtml("try `const x = await fetch(url);` here"));
    const code = parse(html).querySelector("code");
    expect(code?.querySelector(".hljs-keyword")?.textContent).toBe("const");
    expect(html.startsWith("try <code")).toBe(true);
  });

  it("leaves a plain word in inline code alone", () => {
    expect(render("<code>hello</code>")).toBe("<code>hello</code>");
  });

  it("leaves messages without code untouched", () => {
    expect(highlightCode("<b>hi</b>")).toBe("<b>hi</b>");
  });

  it("keeps malicious content in code as inert text", () => {
    const evil = '<img src=x onerror="alert(1)"><script>alert(2)</script>';
    const html = render(markdownToHtml("```html\n" + evil + "\n```"));
    const container = parse(html);
    expect(container.querySelector("img, script, [onerror]")).toBeNull();
    expect(container.querySelector("code")?.textContent).toBe(evil + "\n");
  });

  it("only lets the highlighter's own spans through", () => {
    // Escaped markup in received code must stay text after highlighting.
    const html = render('<pre><code>&lt;span class="x" onclick="y"&gt;a&lt;/span&gt;</code></pre>');
    const container = parse(html);
    expect(container.querySelector("[onclick]")).toBeNull();
    for (const span of container.querySelectorAll("span")) {
      for (const name of span.classList) expect(name).toMatch(/^(hljs-|[a-z]+_)/);
    }
  });
});
