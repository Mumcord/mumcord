// @vitest-environment node
/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { contrastRatio } from "./contrast";

// Read from disk: vitest does not process CSS, so a `?raw` import would be empty.
const tokensCss = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");

/** Reads `--color-*: #hex` declarations from the block whose selector list contains `selector`. */
function colorTokens(selector: string): Map<string, string> {
  const blocks = tokensCss.matchAll(/([^{}]+)\{([^}]*)\}/g);
  for (const [, selectors = "", body = ""] of blocks) {
    if (!selectors.split(",").some((part) => part.trim() === selector)) continue;
    const tokens = new Map<string, string>();
    for (const [, name = "", value = ""] of body.matchAll(
      /--(color-[\w-]+):\s*(#[0-9a-f]{6});/gi,
    )) {
      tokens.set(name, value);
    }
    return tokens;
  }
  throw new Error(`No token block for ${selector}`);
}

const BACKGROUNDS = [
  "bg-servers",
  "bg-sidebar",
  "bg-chat",
  "bg-elevated",
  "bg-hover",
  "bg-selected",
];
const TEXT = [
  "text",
  "text-secondary",
  "text-muted",
  "accent-text",
  "success",
  "warning",
  "danger",
];
// WCAG 1.4.11: focus indicators and control boundaries need 3:1 against adjacent colours.
const NON_TEXT = ["focus", "border-strong", "accent", "talking"];

describe.each([':root[data-theme="dark"]', ':root[data-theme="light"]'])(
  "%s tokens",
  (selector) => {
    const tokens = colorTokens(selector);
    const color = (name: string): string => {
      const value = tokens.get(`color-${name}`);
      if (!value) throw new Error(`Missing --color-${name} in ${selector}`);
      return value;
    };

    it.each(BACKGROUNDS.flatMap((bg) => TEXT.map((fg) => [fg, bg])))(
      "%s on %s has at least 4.5:1",
      (fg, bg) => {
        expect(contrastRatio(color(fg), color(bg))).toBeGreaterThanOrEqual(4.5);
      },
    );

    it.each(BACKGROUNDS.flatMap((bg) => NON_TEXT.map((fg) => [fg, bg])))(
      "%s on %s has at least 3:1",
      (fg, bg) => {
        expect(contrastRatio(color(fg), color(bg))).toBeGreaterThanOrEqual(3);
      },
    );

    it("text on the accent colour has at least 4.5:1", () => {
      expect(contrastRatio(color("on-accent"), color("accent"))).toBeGreaterThanOrEqual(4.5);
    });

    // Primary buttons, the connected server and badges sit on the gradient, so every stop counts.
    it.each(["accent-stop-1", "accent-stop-2", "accent-stop-3"])(
      "text on gradient %s has at least 4.5:1",
      (stop) => {
        expect(contrastRatio(color("on-accent"), color(stop))).toBeGreaterThanOrEqual(4.5);
      },
    );

    it("defines the same colour tokens as the other theme", () => {
      const other = selector.includes("dark")
        ? colorTokens(':root[data-theme="light"]')
        : colorTokens(':root[data-theme="dark"]');
      expect([...tokens.keys()].sort()).toEqual([...other.keys()].sort());
    });
  },
);

describe("contrastRatio", () => {
  it("matches the WCAG reference values", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrastRatio("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
  });
});
