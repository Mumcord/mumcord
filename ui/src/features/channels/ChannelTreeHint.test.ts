import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("channel tree hint layout stability", () => {
  it("hides the unfocused hint via opacity only, never changing layout", () => {
    const css = readFileSync("ui/src/features/channels/channels.css", "utf8");
    const m = css.match(/\.channel-tree:not\(:focus-within\) \.channel-tree__hint\s*\{([^}]*)\}/);
    expect(m).not.toBeNull();
    const body = m?.[1] ?? "";
    expect(body).toMatch(/opacity:\s*0/);
    expect(body).not.toMatch(/display|position|width|height|margin|padding|visibility/);
  });
});
