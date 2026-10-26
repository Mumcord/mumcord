import { describe, expect, it } from "vitest";
import { pinnedIndex, type PinItem } from "./pinnedAuthor";

const item = (top: number, bottom: number, headerBottom: number | null): PinItem => ({
  top,
  bottom,
  headerBottom,
});

describe("pinnedIndex", () => {
  it("shows the message whose header scrolled out while its body is visible", () => {
    const items = [item(-300, -50, -270), item(-40, 400, -10), item(410, 500, 430)];
    expect(pinnedIndex(items, 0, 32)).toBe(1);
  });

  it("is hidden while the header is still visible", () => {
    expect(pinnedIndex([item(-10, 300, 20)], 0, 32)).toBeNull();
  });

  it("is hidden once the message ends under the banner", () => {
    expect(pinnedIndex([item(-300, 20, -270), item(30, 100, 50)], 0, 32)).toBeNull();
  });

  it("shows the run author while a grouped follow-up is topmost", () => {
    const items = [item(-300, -50, -270), item(-40, 400, null), item(410, 500, 430)];
    expect(pinnedIndex(items, 0, 32)).toBe(0);
  });

  it("is hidden when a grouped follow-up's run header is still visible", () => {
    expect(pinnedIndex([item(-100, 50, 10), item(60, 300, null)], 0, 32)).toBeNull();
  });

  it("is hidden when the run ends under the banner", () => {
    const items = [item(-300, -50, -270), item(-40, 20, null), item(30, 100, 50)];
    expect(pinnedIndex(items, 0, 32)).toBeNull();
  });

  it("is hidden when the run's first message is not rendered", () => {
    expect(pinnedIndex([null, item(-100, 300, null)], 0, 32)).toBeNull();
  });

  it("skips unrendered items and handles an empty list", () => {
    expect(pinnedIndex([null, item(-100, 300, -70)], 0, 32)).toBe(1);
    expect(pinnedIndex([], 0, 32)).toBeNull();
  });
});
