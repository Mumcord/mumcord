import { describe, expect, it } from "vitest";
import { highlightHex } from "./hexDump";
import { highlightCode, loadGrammar } from "./highlight";
import { markdownToHtml } from "./markdown";
import { CODE_LANGUAGE, sanitizeHtml } from "./sanitize";

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
    expect(sent).toMatch(/^<pre data-mumcord-lang="js"><code>/);
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

/** Sends `source` as typed in the composer and renders it like a received message. */
function block(source: string): HTMLElement {
  const container = parse(render(markdownToHtml(source)));
  const found = container.querySelector<HTMLElement>(".chat-code");
  if (found === null) throw new Error("no code block");
  return found;
}

describe("named languages (issue #112)", () => {
  it("keeps the fence language as a marker that survives both sanitizers", () => {
    const sent = markdownToHtml("```sql\nSELECT 1;\n```");
    expect(sent).toBe('<pre data-mumcord-lang="sql"><code>SELECT 1;\n</code></pre>');
    const code = block("```sql\nSELECT 1;\n```");
    expect(code.dataset["language"]).toBe("sql");
    expect(code.dataset["languageLabel"]).toBe("SQL");
    expect(code.querySelector(".hljs-keyword")?.textContent).toBe("SELECT");
  });

  it("shows a not yet loaded language plain first, then highlighted", async () => {
    const source = "```swift\nlet x = 1\n```";
    const first = block(source);
    expect(first.dataset["language"]).toBe("swift");
    expect(first.querySelector("span")).toBeNull();
    await loadGrammar("swift");
    expect(block(source).querySelector(".hljs-keyword")).not.toBeNull();
  });

  it.each([
    ["sh", "bash", "Bash"],
    ["shell", "bash", "Bash"],
    ["zsh", "bash", "Bash"],
    ["py", "python", "Python"],
    ["rs", "rust", "Rust"],
    ["js", "javascript", "JavaScript"],
    ["ts", "typescript", "TypeScript"],
    ["c++", "cpp", "C++"],
    ["bat", "cmd", "CMD"],
    ["batch", "cmd", "CMD"],
    ["yml", "yaml", "YAML"],
    ["SQL", "sql", "SQL"],
  ])("resolves the alias %s to %s", (alias, language, label) => {
    const code = block("```" + alias + "\nx = 1\n```");
    expect(code.dataset["language"]).toBe(language);
    expect(code.dataset["languageLabel"]).toBe(label);
  });

  it.each([
    ["php", "<?php echo $name; ?>", "PHP"],
    ["ruby", "def greet(name)\n  puts name\nend", "Ruby"],
    ["kotlin", "fun main() { val x = 1 }", "Kotlin"],
    ["swift", 'func greet() -> String { return "hi" }', "Swift"],
    ["lua", "local function f() return nil end", "Lua"],
    ["powershell", "Get-ChildItem -Path C:\\ | Where-Object { $_.Length -gt 1 }", "PowerShell"],
    ["cmd", "@echo off\nif exist foo.txt echo yes", "CMD"],
    ["dockerfile", "FROM alpine:3\nRUN apk add curl", "Dockerfile"],
    ["toml", '[package]\nname = "mumcord"', "TOML"],
    ["markdown", "# Title\n\n**bold**", "Markdown"],
    ["makefile", "all: build\n\tcargo build", "Makefile"],
    ["nginx", "server { listen 80; }", "nginx"],
    ["rust", "fn main() { let x = 1; }", "Rust"],
    ["java", "public class A { int x; }", "Java"],
    ["c", "int main(void) { return 0; }", "C"],
  ])("highlights %s as exactly that language", async (language, source, label) => {
    const grammar = { cmd: "dos", toml: "ini" }[language] ?? language;
    await loadGrammar(grammar);
    const code = block("```" + language + "\n" + source + "\n```");
    expect(code.dataset["languageLabel"]).toBe(label);
    const inner = code.querySelector("code");
    expect(inner?.classList.contains("hljs")).toBe(true);
    expect(inner?.querySelector("[class^='hljs-']")).not.toBeNull();
    expect(inner?.textContent).toBe(source + "\n");
  });

  it("keeps plaintext plain, with its label", () => {
    const code = block("```plaintext\nconst x = 1; function f() { return x; }\n```");
    expect(code.dataset["language"]).toBe("plaintext");
    expect(code.querySelector("span")).toBeNull();
  });

  it("shows an unknown language as plain text with its name, without guessing", () => {
    const code = block("```klingon\nconst x = 1; function f() { return x; }\n```");
    expect(code.dataset["languageLabel"]).toBe("klingon");
    expect(code.dataset["language"]).toBeUndefined();
    expect(code.querySelector("span")).toBeNull();
  });

  it("still detects the language of blocks from other clients", () => {
    const code = parse(render("<pre><code>" + JS + "</code></pre>")).querySelector<HTMLElement>(
      ".chat-code",
    );
    expect(code?.dataset["languageLabel"]).toBeUndefined();
    expect(code?.dataset["language"]).toBe("javascript");
    expect(code?.querySelector(".hljs-keyword")).not.toBeNull();
  });

  it.each([
    ['"><img src=x onerror=alert(1)>', "attribute break-out"],
    ["sql onmouseover=alert(1)", "extra attribute"],
    ["javascript:alert(1)", "script URL"],
  ])("refuses a hostile fence language (%s, %s)", (language) => {
    const sent = markdownToHtml("```" + language + "\nSELECT 1;\n```");
    const container = parse(render(sent));
    expect(container.querySelector("img, [onerror], [onmouseover]")).toBeNull();
    const marker = /data-mumcord-lang="([^"]*)"/.exec(sent)?.[1];
    expect(marker === undefined || CODE_LANGUAGE.test(marker)).toBe(true);
  });

  it("drops a hostile marker received from the server", () => {
    const evil =
      '<pre data-mumcord-lang="&quot;&gt;&lt;img src=x onerror=alert(1)&gt;"><code>x</code></pre>' +
      '<div data-mumcord-lang="sql">y</div><code data-mumcord-lang="sql">z</code>';
    const container = parse(render(evil));
    expect(container.querySelector("img, [onerror]")).toBeNull();
    expect(container.querySelectorAll("[data-mumcord-lang]")).toHaveLength(0);
    expect(container.querySelector<HTMLElement>(".chat-code")?.dataset["languageLabel"]).toBe(
      undefined,
    );
  });

  it("keeps markup in a named block as inert text", () => {
    const evil = '<img src=x onerror="alert(1)"><script>alert(2)</script>';
    const container = parse(render(markdownToHtml("```html\n" + evil + "\n```")));
    expect(container.querySelector("img, script, [onerror]")).toBeNull();
    expect(container.querySelector("code")?.textContent).toBe(evil + "\n");
    expect(container.querySelector<HTMLElement>(".chat-code")?.dataset["languageLabel"]).toBe(
      "HTML",
    );
  });
});

describe("hex dumps", () => {
  const classes = (html: string, scope: string) =>
    [...parse(html).querySelectorAll(`.hljs-${scope}`)].map((span) => span.textContent);

  it("colors hexdump -C: offset, bytes and the ASCII column", () => {
    const dump = "00000000  48 65 6c 6c 6f 00 77 6f  72 6c 64 0a  |Hello.world.|";
    const html = highlightHex(dump);
    expect(parse(html).textContent).toBe(dump);
    expect(classes(html, "comment")).toEqual(["00000000", "00"]);
    expect(classes(html, "number")).toHaveLength(11);
    expect(classes(html, "string")).toEqual(["|Hello.world.|"]);
  });

  it("colors xxd output with grouped bytes", () => {
    const dump = "00000010: 4865 6c6c 6f0a  Hello.";
    const html = highlightHex(dump);
    expect(parse(html).textContent).toBe(dump);
    expect(classes(html, "comment")).toEqual(["00000010:"]);
    expect(classes(html, "number")).toEqual(["48", "65", "6c", "6c", "6f", "0a"]);
    expect(classes(html, "string")).toEqual(["Hello."]);
  });

  it.each(["48 65 6c 6c", "0x48, 0x65, 0x6c", "\\x48\\x65\\x6c", "48:65:6c", "48656c"])(
    "colors free-form hex %s",
    (dump) => {
      const html = highlightHex(dump);
      expect(parse(html).textContent).toBe(dump);
      expect(classes(html, "number").length).toBeGreaterThanOrEqual(3);
    },
  );

  it("leaves text it does not understand plain and never loses characters", () => {
    const dump = "not hex at all <b>&amp;\n\n  zz 4 abc";
    const html = highlightHex(dump);
    expect(parse(html).textContent).toBe(dump);
    expect(parse(html).querySelector("b")).toBeNull();
  });

  it("highlights a ```hex block end to end", () => {
    const code = block("```hex\n00000000  de ad be ef  |....|\n```");
    expect(code.dataset["languageLabel"]).toBe("Hex");
    expect(code.querySelector(".hljs-number")?.textContent).toBe("de");
  });
});
