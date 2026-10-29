import { describe, expect, it } from "vitest";
import { displaySrc, originalSrc } from "./displaySrc";

const GIF = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";

describe("displaySrc", () => {
  it("shows a data: URL through one blob: URL of the same bytes", async () => {
    const shown = displaySrc(GIF);
    expect(shown).toMatch(/^blob:/);
    expect(displaySrc(GIF)).toBe(shown);
    expect(originalSrc(shown)).toBe(GIF);
    const blob = await (await fetch(shown)).blob();
    expect(blob.type).toBe("image/gif");
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(
      Uint8Array.from(atob("R0lGODlhAQABAAAAACw="), (c) => c.charCodeAt(0)),
    );
  });

  it("leaves other addresses alone", () => {
    expect(displaySrc("https://example.com/a.gif")).toBe("https://example.com/a.gif");
    expect(originalSrc("https://example.com/a.gif")).toBe("https://example.com/a.gif");
  });
});
