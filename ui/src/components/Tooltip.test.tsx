import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TOOLTIP_DELAY_MS, TooltipLayer } from "./Tooltip";

function setup(trigger: React.ReactNode) {
  render(
    <>
      <TooltipLayer />
      {trigger}
    </>,
  );
}

function wait(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

describe("themed tooltip", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("appears on hover only after the delay and links itself via aria-describedby", () => {
    setup(
      <button type="button" aria-label="Microphone" data-tooltip="Hold Ctrl to talk">
        x
      </button>,
    );
    const button = screen.getByRole("button", { name: "Microphone" });
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS - 1);
    expect(screen.queryByRole("tooltip")).toBeNull();
    wait(1);
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent("Hold Ctrl to talk");
    expect(button).toHaveAttribute("aria-describedby", tip.id);
    expect(button).toHaveAccessibleDescription("Hold Ctrl to talk");

    fireEvent.pointerOut(button, { relatedTarget: document.body });
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(button).not.toHaveAttribute("aria-describedby");
  });

  it("with data-tooltip-overflow appears only while the text is cut off", () => {
    setup(
      <span data-testid="name" data-tooltip="Study Hall" data-tooltip-overflow>
        Study Hall
      </span>,
    );
    const name = screen.getByTestId("name");
    fireEvent.pointerOver(name);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.queryByRole("tooltip")).toBeNull();

    fireEvent.pointerOut(name, { relatedTarget: document.body });
    Object.defineProperty(name, "scrollWidth", { configurable: true, value: 120 });
    Object.defineProperty(name, "clientWidth", { configurable: true, value: 60 });
    fireEvent.pointerOver(name);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Study Hall");
  });

  it("shows a shortcut chip and a detail line, and describes the trigger with them", () => {
    setup(
      <button
        type="button"
        aria-label="Show members"
        data-tooltip="Show members"
        data-tooltip-shortcut="Ctrl+Shift+M"
        data-tooltip-detail="Freya is talking"
      >
        x
      </button>,
    );
    const button = screen.getByRole("button", { name: "Show members" });
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    const tip = screen.getByRole("tooltip");
    expect(tip.querySelector("kbd")).toHaveTextContent("Ctrl+Shift+M");
    expect(tip.querySelector(".tooltip__detail")).toHaveTextContent("Freya is talking");
    expect(button).toHaveAttribute("aria-describedby", tip.id);
  });

  it("appears on keyboard focus and hides on blur", () => {
    setup(
      <button type="button" aria-label="Settings" data-tooltip="Settings">
        x
      </button>,
    );
    const button = screen.getByRole("button", { name: "Settings" });
    act(() => {
      button.focus();
    });
    wait(TOOLTIP_DELAY_MS);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Settings");
    // Same text as the name: not described a second time.
    expect(button).not.toHaveAttribute("aria-describedby");
    act(() => {
      button.blur();
    });
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("keeps an existing description and adds its own after it", () => {
    setup(
      <>
        <button type="button" aria-label="Share" aria-describedby="why" data-tooltip="Beta">
          x
        </button>
        <span id="why">Join a channel first</span>
      </>,
    );
    const button = screen.getByRole("button", { name: "Share" });
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    const tip = screen.getByRole("tooltip");
    expect(button).toHaveAttribute("aria-describedby", `why ${tip.id}`);
    fireEvent.pointerOut(button);
    expect(button).toHaveAttribute("aria-describedby", "why");
  });

  it("hides on Escape and stays hidden until the pointer leaves", () => {
    setup(
      <button type="button" aria-label="Send" data-tooltip="Send message">
        x
      </button>,
    );
    const button = screen.getByRole("button", { name: "Send" });
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.pointerOut(button);
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
  });

  it("hides when the page scrolls, so the spec has to hover again to see it", () => {
    setup(
      <button type="button" aria-label="Send" data-tooltip="Send message">
        x
      </button>,
    );
    const button = screen.getByRole("button", { name: "Send" });
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    // A scroll anywhere moves the page under the tooltip, so the layer drops it: it only comes
    // back when the pointer enters the trigger again.
    fireEvent.scroll(document);
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
  });

  it("stays away after a click on the trigger until the pointer enters it again", () => {
    setup(
      <button
        type="button"
        aria-label="Show members"
        data-tooltip="Show members"
        data-tooltip-detail="Freya is talking"
      >
        x
      </button>,
    );
    const button = screen.getByRole("button", { name: "Show members" });
    // What the E2E spec (#216) has to do: a click dismisses this trigger, and the pointer must
    // leave and enter again before the tooltip – with the talking detail – comes back.
    fireEvent.pointerOver(button);
    fireEvent.pointerDown(button);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.pointerOut(button, { relatedTarget: document.body });
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    const tip = screen.getByRole("tooltip");
    expect(tip.querySelector(".tooltip__detail")).toHaveTextContent("Freya is talking");
  });

  it("does not show for a click before the delay ran out", () => {
    setup(
      <button type="button" aria-label="Send" data-tooltip="Send message">
        x
      </button>,
    );
    const button = screen.getByRole("button", { name: "Send" });
    fireEvent.pointerOver(button);
    fireEvent.pointerDown(button);
    wait(TOOLTIP_DELAY_MS);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("follows a changed label and hides when the trigger goes away", async () => {
    const { rerender } = render(
      <>
        <TooltipLayer />
        <button type="button" aria-label="Mic" data-tooltip="Muted" />
      </>,
    );
    fireEvent.pointerOver(screen.getByRole("button"));
    wait(TOOLTIP_DELAY_MS);
    rerender(
      <>
        <TooltipLayer />
        <button type="button" aria-label="Mic" data-tooltip="Talking" />
      </>,
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByRole("tooltip")).toHaveTextContent("Talking");
    rerender(<TooltipLayer />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("stays inside the window next to a trigger at the far left edge", () => {
    setup(
      <button type="button" aria-label="Server" data-tooltip="A very long server name">
        x
      </button>,
    );
    const button = screen.getByRole("button", { name: "Server" });
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
      this: Element,
    ) {
      return this === button ? new DOMRect(0, 2, 40, 40) : new DOMRect(0, 0, 200, 30);
    });
    fireEvent.pointerOver(button);
    wait(TOOLTIP_DELAY_MS);
    const tip = screen.getByRole("tooltip");
    // Centred over the button it would start at -80; above it there is no room either.
    expect(tip.style.left).toBe("8px");
    expect(tip.style.top).toBe(`${String(2 + 40 + 6)}px`);
    expect(tip.dataset.placement).toBe("bottom");
  });

  it("stacks above a modal when its trigger sits inside one", () => {
    setup(
      <div role="dialog" aria-modal="true" aria-label="Edit">
        <button type="button" aria-label="Close" data-tooltip="Close">
          x
        </button>
      </div>,
    );
    fireEvent.pointerOver(screen.getByRole("button", { name: "Close" }));
    wait(TOOLTIP_DELAY_MS);
    expect(screen.getByRole("tooltip")).toHaveAttribute("data-in-modal");
  });
});
