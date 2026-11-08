import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const KIT_DIR = __dirname;
const ROOT = join(__dirname, "..", "..", "..");
const css = readFileSync(join(KIT_DIR, "kit.css"), "utf8");
const code = css.replace(/\/\*[\s\S]*?\*\//g, "");

// The token set of spec 13 FR-13.9: the only custom properties the kit may read.
const TOKENS = new Set([
  ...[
    "text",
    "text-muted",
    "accent",
    "danger",
    "warning",
    "success",
    "surface",
    "surface-raised",
    "border",
    "text-secondary",
    "accent-text",
    "on-accent",
    "primary",
    "on-primary",
    "hover",
    "selected",
    "border-strong",
    "focus",
    "backdrop",
  ].map((name) => `--mc-color-${name}`),
  "--mc-font-family",
  "--mc-font-family-display",
  "--mc-font-family-mono",
  ...["xs", "sm", "md", "lg", "xl"].map((size) => `--mc-font-size-${size}`),
  "--mc-font-weight-normal",
  "--mc-font-weight-bold",
  "--mc-line-height",
  ...["1", "2", "3", "4", "5", "6"].map((step) => `--mc-space-${step}`),
  ...["sm", "md", "lg", "full"].map((size) => `--mc-radius-${size}`),
  "--mc-focus-ring-width",
  "--mc-shadow-raised",
  "--mc-shadow-primary-glow",
  "--mc-duration-fast",
  "--mc-duration-normal",
  "--mc-ease-out",
]);

interface Rule {
  selector: string;
  body: string;
  atRules: string[];
}

/** Style rules with the at-rules they sit in; enough for this flat, hand-written file. */
function rules(text: string): Rule[] {
  const found: Rule[] = [];
  const stack: string[] = [];
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "{") {
      const prelude = text.slice(start, i).trim();
      if (prelude.startsWith("@") && !prelude.startsWith("@font-face")) {
        stack.push(prelude);
        start = i + 1;
        continue;
      }
      const end = text.indexOf("}", i);
      found.push({ selector: prelude, body: text.slice(i + 1, end), atRules: [...stack] });
      i = end;
      start = i + 1;
    } else if (ch === "}") {
      stack.pop();
      start = i + 1;
    }
  }
  return found;
}

/** Splits a selector list at top-level commas. */
function selectorList(selector: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const ch of selector) {
    if (ch === "(") depth++;
    if (ch === ")") depth--;
    if (ch === "," && depth === 0) {
      parts.push(current.trim());
      current = "";
    } else current += ch;
  }
  parts.push(current.trim());
  return parts;
}

const all = rules(code);
const styleRules = all.filter((rule) => !rule.selector.startsWith("@font-face"));
const forced = (rule: Rule) => rule.atRules.some((at) => at.includes("forced-colors: active"));

describe("plugin UI kit", () => {
  it("parses into style rules", () => {
    expect(styleRules.length).toBeGreaterThan(30);
  });

  it("wraps every selector in :where() so pages always win (FR-13.20)", () => {
    for (const rule of styleRules) {
      for (const selector of selectorList(rule.selector)) {
        // Allowed outside :where(): pseudo-elements and the state pseudo-classes after it.
        const outside = selector.replace(/:where\((?:[^()]|\((?:[^()]|\([^()]*\))*\))*\)/g, "");
        expect(outside, selector).toMatch(/^(\s|::?[\w-]+(\([^()]*\))?)*$/);
      }
    }
  });

  it("only touches kit classes or plain controls inside .mc-kit (FR-13.20)", () => {
    for (const rule of styleRules) {
      for (const selector of selectorList(rule.selector)) {
        expect(selector, selector).toMatch(/\.mc-/);
        expect(selector, selector).not.toMatch(/(^|[\s(,])(html|body|\*)([\s:.[)]|$)/);
      }
    }
  });

  it("matches the app's glow, danger and dialog look (lead decision, spec 13 §12)", () => {
    const body = (needle: string): string =>
      rules(css)
        .filter((r) => r.atRules.length === 0 && r.selector.includes(needle))
        .map((r) => r.body)
        .join("\n");
    expect(body(".mc-button--primary")).toContain("box-shadow: var(--mc-shadow-primary-glow)");
    const danger = body(".mc-button--danger");
    expect(danger).toContain("border-color: var(--mc-color-danger)");
    expect(danger).toContain("color: var(--mc-color-danger)");
    expect(body(".mc-dialog")).toContain("background: var(--mc-color-surface);");
    expect(body(".mc-kit")).toContain("font-family: var(--mc-font-family)");
  });

  it("uses !important only to restore system colours under forced colours", () => {
    for (const rule of styleRules) {
      if (!forced(rule)) expect(rule.body, rule.selector).not.toContain("!important");
    }
  });

  it("reads only the --mc- tokens of FR-13.9 and declares no custom properties", () => {
    const used = [...code.matchAll(/var\((--[\w-]+)/g)].map((m) => m[1] ?? "");
    expect(used.length).toBeGreaterThan(0);
    expect(used.filter((name) => !TOKENS.has(name))).toEqual([]);
    expect(code).not.toMatch(/(^|[\s;{])--[\w-]+\s*:/);
  });

  it("has no literal colours outside forced colours (FR-13.21)", () => {
    for (const rule of styleRules) {
      if (forced(rule)) continue;
      const body = rule.body.replace(/url\("data:[^"]*"\)/g, "");
      expect(body, rule.selector).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(|oklch\(/i);
    }
  });

  it("sizes in rem or em only (FR-13.27)", () => {
    const body = code.replace(/url\("data:[^"]*"\)/g, "");
    expect(body).not.toMatch(/\d(px|pt|vh|vw)\b/);
  });

  it("stays below 30 KiB", () => {
    expect(Buffer.byteLength(css)).toBeLessThanOrEqual(30 * 1024);
  });

  it("links only its bundled fonts and inline images, nothing from the network", () => {
    const urls = [...code.matchAll(/url\("([^"]*)"\)/g)].map((m) => m[1] ?? "");
    const files = urls.filter((url) => !url.startsWith("data:"));
    expect(files.sort()).toEqual([
      "fonts/bricolage-grotesque-latin-wght-normal.woff2",
      "fonts/instrument-sans-latin-wght-normal.woff2",
    ]);
  });

  it("ships the same font files the app bundles (FR-13.18)", () => {
    for (const [pkg, file] of [
      ["instrument-sans", "instrument-sans-latin-wght-normal.woff2"],
      ["bricolage-grotesque", "bricolage-grotesque-latin-wght-normal.woff2"],
    ] as const) {
      const app = readFileSync(
        join(ROOT, "node_modules", "@fontsource-variable", pkg, "files", file),
      );
      expect(readFileSync(join(KIT_DIR, "fonts", file)).equals(app), file).toBe(true);
    }
    expect(readFileSync(join(KIT_DIR, "fonts", "OFL.txt"), "utf8")).toContain(
      "SIL OPEN FONT LICENSE Version 1.1",
    );
  });
});
