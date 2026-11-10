/** The keys of the camera tiles (spec 08 §5.5). */
import { fireEvent, render } from "@testing-library/react";
import { createElement } from "react";
import { describe, expect, it, vi } from "vitest";
import { gridMove, tileCommand, useGridKeys } from "./useGridKeys";

const plain = { shiftKey: false, ctrlKey: false, altKey: false, metaKey: false };

describe("gridMove", () => {
  it("moves through a 2 × 2 grid and stops at its edges", () => {
    expect(gridMove("ArrowRight", 0, 4, 2)).toBe(1);
    expect(gridMove("ArrowRight", 3, 4, 2)).toBe(3);
    expect(gridMove("ArrowLeft", 0, 4, 2)).toBe(0);
    expect(gridMove("ArrowDown", 1, 4, 2)).toBe(3);
    expect(gridMove("ArrowDown", 2, 4, 2)).toBe(2);
    expect(gridMove("ArrowUp", 3, 4, 2)).toBe(1);
    expect(gridMove("ArrowUp", 1, 4, 2)).toBe(1);
  });

  it("stays in an incomplete last row", () => {
    // Five tiles in three columns: no tile below the third.
    expect(gridMove("ArrowDown", 2, 5, 3)).toBe(2);
    expect(gridMove("ArrowDown", 1, 5, 3)).toBe(4);
  });

  it("goes to the first and last tile with Home and End", () => {
    expect(gridMove("Home", 3, 9, 3)).toBe(0);
    expect(gridMove("End", 0, 9, 3)).toBe(8);
  });

  it("ignores other keys and empty grids", () => {
    expect(gridMove("a", 0, 4, 2)).toBeNull();
    expect(gridMove("ArrowRight", 0, 0, 1)).toBeNull();
  });
});

describe("tileCommand", () => {
  it("maps the tile keys", () => {
    expect(tileCommand({ ...plain, key: "Enter" })).toBe("menu");
    expect(tileCommand({ ...plain, key: "F10", shiftKey: true })).toBe("menu");
    expect(tileCommand({ ...plain, key: "ContextMenu" })).toBe("menu");
    expect(tileCommand({ ...plain, key: "p" })).toBe("pin");
    expect(tileCommand({ ...plain, key: "F" })).toBe("large");
    expect(tileCommand({ ...plain, key: "h" })).toBe("hide");
    expect(tileCommand({ ...plain, key: "Escape" })).toBe("escape");
  });

  it("leaves modified keys and plain F10 alone", () => {
    expect(tileCommand({ ...plain, key: "F10" })).toBeNull();
    expect(tileCommand({ ...plain, key: "p", ctrlKey: true })).toBeNull();
    expect(tileCommand({ ...plain, key: "P", shiftKey: true })).toBeNull();
    expect(tileCommand({ ...plain, key: "h", metaKey: true })).toBeNull();
  });
});

describe("useGridKeys", () => {
  function Tiles() {
    const keys = useGridKeys([1], 1, () => true);
    return createElement(
      "div",
      { onKeyDown: keys.onKeyDown, onFocus: keys.onFocus },
      createElement("div", { "data-tile": "1", tabIndex: keys.tabIndexOf(1) }),
    );
  }

  it("does not look for the focus after the tiles are gone", async () => {
    const { container, unmount } = render(createElement(Tiles));
    const tile = container.querySelector<HTMLElement>("[data-tile]");
    if (tile === null) throw new Error("no tile");
    const contains = vi.spyOn(tile.parentElement as HTMLElement, "contains");
    fireEvent.keyDown(tile, { key: "f" });
    unmount();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(contains).not.toHaveBeenCalled();
  });
});
