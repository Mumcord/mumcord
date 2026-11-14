import { describe, expect, it, vi } from "vitest";
import { firstVisibleIndex, pinnedIndex } from "./pinnedAuthor";

function item(top: number, bottom: number, headerBottom: number | null): HTMLElement {
  const row = document.createElement("li");
  vi.spyOn(row, "getBoundingClientRect").mockReturnValue(new DOMRect(0, top, 400, bottom - top));
  if (headerBottom !== null) {
    const header = document.createElement("div");
    header.className = "chat-message__header";
    vi.spyOn(header, "getBoundingClientRect").mockReturnValue(
      new DOMRect(0, top, 400, headerBottom - top),
    );
    row.append(header);
  }
  return row;
}

describe("firstVisibleIndex", () => {
  it("finds variable-height rows and excludes one ending exactly at the viewport edge", () => {
    const items = [item(-300, -50, -270), item(-40, 0, null), item(10, 400, 30)];
    expect(firstVisibleIndex(items, 0)).toBe(2);
    expect(firstVisibleIndex(items, -60)).toBe(0);
    expect(firstVisibleIndex(items, 400)).toBe(-1);
    expect(firstVisibleIndex([], 0)).toBe(-1);
  });

  it("falls back safely when a ref is missing in the search", () => {
    expect(firstVisibleIndex([item(-100, 20, -80), null, item(30, 100, 50)], 0)).toBe(0);
    expect(firstVisibleIndex([null, null], 0)).toBe(-1);
    const pendingRefs = new Array<HTMLElement | null>(3);
    pendingRefs[2] = item(30, 100, 50);
    expect(firstVisibleIndex(pendingRefs, 0)).toBe(2);
  });

  it("needs logarithmic geometry reads in a long list", () => {
    const items = Array.from({ length: 1024 }, (_, index) =>
      item(index * 100, (index + 1) * 100, null),
    );
    expect(firstVisibleIndex(items, 100_050)).toBe(1000);
    const reads = items.reduce(
      (total, row) => total + vi.spyOn(row, "getBoundingClientRect").mock.calls.length,
      0,
    );
    expect(reads).toBeLessThanOrEqual(11);
  });
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

  it("does not measure every grouped follow-up", () => {
    const items = Array.from({ length: 1024 }, (_, index) =>
      item(index * 100, (index + 1) * 100, index === 0 ? 20 : null),
    );
    expect(pinnedIndex(items, 100_050, 32)).toBe(0);
    const reads = items.reduce(
      (total, row) => total + vi.spyOn(row, "getBoundingClientRect").mock.calls.length,
      0,
    );
    expect(reads).toBeLessThanOrEqual(12);
  });

  it("skips unrendered items and handles an empty list", () => {
    expect(pinnedIndex([null, item(-100, 300, -70)], 0, 32)).toBe(1);
    expect(pinnedIndex([], 0, 32)).toBeNull();
  });
});
