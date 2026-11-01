import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("settings.css", () => {
  it("declares the compact input width after the base input width", () => {
    const css = readFileSync(
      resolve(process.cwd(), "ui/src/features/settings/settings.css"),
      "utf8",
    );
    const base = css.indexOf("\n.settings-input {");
    const compact = css.indexOf("\n.settings-input--compact {");
    expect(base).toBeGreaterThan(-1);
    // Equal specificity: the later rule wins, so compact must come last.
    expect(compact).toBeGreaterThan(base);
  });
});
