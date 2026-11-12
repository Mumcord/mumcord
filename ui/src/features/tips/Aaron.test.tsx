import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Aaron } from "./Aaron";

afterEach(() => {
  vi.restoreAllMocks();
});

/** Every id an element of the given Aaron defines (gradients, clips, masks). */
function definedIds(svg: Element): string[] {
  return [...svg.querySelectorAll("[id]")].map((element) => element.id);
}

describe("Aaron", () => {
  it("is decorative and hidden from screen readers", () => {
    render(<Aaron pose="idle" talking={false} className="extra" />);
    const svg = screen.getByTestId("aaron");
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("focusable", "false");
    expect(svg).toHaveClass("aaron", "extra");
    expect(svg).not.toHaveClass("aaron--talking");
  });

  it("shows the current pose and whether he talks", () => {
    const { rerender } = render(<Aaron pose="idle" talking={false} />);
    const svg = screen.getByTestId("aaron");
    expect(svg).toHaveAttribute("data-pose", "idle");
    expect(svg).not.toHaveClass("aaron--pointing");

    rerender(<Aaron pose="point-right" talking />);
    expect(svg).toHaveAttribute("data-pose", "point-right");
    expect(svg).toHaveClass("aaron--pointing", "aaron--point-right", "aaron--talking");

    rerender(<Aaron pose="point-up" talking={false} />);
    expect(svg).toHaveAttribute("data-pose", "point-up");
    expect(svg).toHaveClass("aaron--pointing");
    expect(svg).not.toHaveClass("aaron--point-right");
  });

  it("gives every instance its own gradient, clip and mask ids", () => {
    render(
      <>
        <Aaron pose="idle" talking={false} />
        <Aaron pose="point-left" talking />
      </>,
    );
    const [first, second] = screen.getAllByTestId("aaron");
    if (!first || !second) throw new Error("expected two Aarons");
    const firstIds = definedIds(first);
    const secondIds = definedIds(second);
    expect(firstIds.length).toBeGreaterThan(0);
    expect(new Set([...firstIds, ...secondIds]).size).toBe(firstIds.length + secondIds.length);
    // References point at the instance's own definitions.
    const fill = first.querySelector("ellipse[fill^='url(']")?.getAttribute("fill") ?? "";
    expect(firstIds).toContain(fill.slice("url(#".length, -1));
  });

  it("stops following the mouse once removed", () => {
    const added = vi.spyOn(window, "addEventListener");
    const removed = vi.spyOn(window, "removeEventListener");
    const { unmount } = render(<Aaron pose="idle" talking />);
    const listeners = added.mock.calls.filter(([type]) => type.startsWith("pointer"));
    expect(listeners.map(([type]) => type)).toEqual(
      expect.arrayContaining(["pointermove", "pointerout"]),
    );

    unmount();
    for (const [type, listener] of listeners) {
      expect(removed).toHaveBeenCalledWith(type, listener);
    }
  });
});
