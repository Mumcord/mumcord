import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(__dirname, "..");

// Custom properties that are set only at runtime from a computed name, so no literal
// declaration exists for the scan to find. Keep this list empty unless a reason is given here.
const RUNTIME_SET = new Set<string>([]);

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.(css|tsx?)$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

describe("CSS custom properties", () => {
  it("every var(--name) refers to a property that is defined somewhere", () => {
    const defined = new Set(RUNTIME_SET);
    const used = new Map<string, string>();
    for (const file of sourceFiles(SRC)) {
      const text = readFileSync(file, "utf8");
      // CSS declarations, and quoted names handed to style.setProperty / inline style objects.
      for (const m of text.matchAll(/(?:^|[\s;{"'`])(--[\w-]+)\s*:/g)) defined.add(m[1] ?? "");
      for (const m of text.matchAll(/["'`](--[\w-]+)["'`]/g)) defined.add(m[1] ?? "");
      for (const m of text.matchAll(/var\((--[\w-]+)/g)) {
        const name = m[1] ?? "";
        // A trailing dash means the name is built at runtime (`--color-avatar-${n}`).
        if (!name.endsWith("-")) used.set(name, file);
      }
    }
    // Names built from a template prefix (`--color-${token}`) are covered by tokens.css.
    const undefinedNames = [...used].filter(([name]) => !defined.has(name));
    expect(undefinedNames).toEqual([]);
  });
});
