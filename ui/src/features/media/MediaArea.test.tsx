/** The resizable media area (spec 07 §4.9): handle, keyboard, collapse and the remembered height. */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { subscribeAnnouncements } from "../../a11y/announcer";
import { axeViolations } from "../../test/axe";
import { MediaArea } from "./MediaArea";
import { resetMediaResize } from "./mediaResize";
import { mediaView, openMedia, resetMediaView } from "./mediaView";

/*
 * jsdom has no layout, so the chat column, the picture and the handle get fixed boxes: a 900 px
 * column with the picture starting 60 px down and the 12 px handle right below it.
 */
const COLUMN = 900;
const TOP = 60;

function pictureHeight(): number {
  const area = document.querySelector<HTMLElement>(".media-area");
  const height = area?.style.getPropertyValue("--media-height") ?? "";
  // Auto: 16:9 of the 800 px wide column.
  return height === "" ? 450 : parseFloat(height);
}

function box(top: number, height: number): DOMRect {
  return {
    top,
    bottom: top + height,
    height,
    left: 0,
    right: 800,
    width: 800,
    x: 0,
    y: top,
  } as DOMRect;
}

function layout(this: Element): DOMRect {
  if (this.classList.contains("chat")) return box(0, COLUMN);
  if (this.classList.contains("media-frame__picture")) return box(TOP, pictureHeight());
  if (this.classList.contains("media-area__handle")) return box(TOP + pictureHeight(), 12);
  if (this.classList.contains("media-area")) return box(TOP, pictureHeight());
  return box(0, 0);
}

function Frame() {
  return (
    <section className="media-frame">
      <div className="media-frame__picture" />
      <button type="button">Stop watching</button>
    </section>
  );
}

function show() {
  return render(
    <div className="chat">
      <MediaArea>
        <Frame />
      </MediaArea>
      <p>Messages</p>
    </div>,
  );
}

const handle = () => screen.getByRole("separator", { name: "Resize media area" });
const stored = () => localStorage.getItem("mumcord.mediaHeight");

beforeEach(async () => {
  await applyLanguage("en");
  localStorage.clear();
  resetMediaResize();
  resetMediaView();
  openMedia("share");
  // jsdom leaves this undefined; the app reads null when nothing is in full screen.
  Object.defineProperty(document, "fullscreenElement", { configurable: true, value: null });
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(layout);
  vi.spyOn(Element.prototype, "clientHeight", "get").mockImplementation(function (this: Element) {
    return this.classList.contains("chat") ? COLUMN : 0;
  });
  vi.spyOn(Element.prototype, "clientWidth", "get").mockReturnValue(800);
  HTMLElement.prototype.setPointerCapture = vi.fn();
  HTMLElement.prototype.releasePointerCapture = vi.fn();
  HTMLElement.prototype.hasPointerCapture = vi.fn(() => true);
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("media area handle", () => {
  it("is a labelled horizontal separator after the media controls, in Auto", async () => {
    const { container } = show();
    const separator = handle();
    expect(separator).toHaveAttribute("aria-orientation", "horizontal");
    expect(separator).toHaveAttribute("aria-controls", "media-area");
    expect(document.getElementById("media-area")).not.toBeNull();
    expect(separator).toHaveAttribute("aria-valuenow", "50");
    expect(separator).toHaveAttribute("aria-valuemin", "20");
    // 70 % would leave the chat under 13 rem: 900 − 208 − 72 px above the chat = 620 px.
    expect(separator).toHaveAttribute("aria-valuemax", "69");
    expect(separator).toHaveAttribute("aria-valuetext", "50% of the window height");
    // Auto leaves the frame exactly as before.
    expect(document.querySelector(".media-area")).not.toHaveAttribute("data-sized");
    const order = [...container.querySelectorAll("button, [role=separator]")];
    expect(order.indexOf(separator)).toBeGreaterThan(
      order.indexOf(screen.getByRole("button", { name: "Stop watching" })),
    );
    expect(await axeViolations(container)).toEqual([]);
  });

  it("changes and remembers the height on every key press, and speaks it", () => {
    const spoken: string[] = [];
    const stop = subscribeAnnouncements(({ message }) => spoken.push(message));
    show();
    fireEvent.keyDown(handle(), { key: "Home" });
    expect(handle()).toHaveAttribute("aria-valuenow", "20");
    expect(stored()).toBe("0.2");
    fireEvent.keyDown(handle(), { key: "ArrowDown" });
    expect(handle()).toHaveAttribute("aria-valuenow", "24");
    fireEvent.keyDown(handle(), { key: "PageDown" });
    expect(handle()).toHaveAttribute("aria-valuenow", "33");
    fireEvent.keyDown(handle(), { key: "End" });
    expect(handle()).toHaveAttribute("aria-valuenow", "69");
    expect(stored()).toBe(String(620 / 900));
    expect(spoken).toContain("69% of the window height");
    stop();
  });

  it("goes back to Auto with Enter and with a double-click", () => {
    show();
    fireEvent.keyDown(handle(), { key: "End" });
    fireEvent.keyDown(handle(), { key: "Enter" });
    expect(stored()).toBe('"auto"');
    fireEvent.keyDown(handle(), { key: "Home" });
    fireEvent.doubleClick(handle());
    expect(stored()).toBe('"auto"');
  });

  it("follows a drag and saves on release; a plain click changes nothing", () => {
    show();
    fireEvent.pointerDown(handle(), { button: 0, clientY: 516, pointerId: 1 });
    fireEvent.pointerUp(handle(), { clientY: 516, pointerId: 1 });
    expect(stored()).toBeNull();

    fireEvent.pointerDown(handle(), { button: 0, clientY: 516, pointerId: 1 });
    expect(document.documentElement.dataset.rowResizing).toBe("media");
    fireEvent.pointerUp(handle(), { clientY: 516 - 200, pointerId: 1 });
    // 450 − 200 = 250 px of 900, no snap point near.
    expect(stored()).toBe(String(250 / 900));
    expect(handle()).toHaveAttribute("aria-valuenow", "28");
    expect(document.documentElement.dataset.rowResizing).toBeUndefined();
  });

  it("snaps near 1/3 unless Alt is held", () => {
    show();
    fireEvent.pointerDown(handle(), { button: 0, clientY: 516, pointerId: 1 });
    fireEvent.pointerUp(handle(), { clientY: 516 - 145, pointerId: 1 });
    expect(stored()).toBe(String(300 / 900));
    fireEvent.pointerDown(handle(), { button: 0, clientY: 372, pointerId: 1 });
    fireEvent.pointerUp(handle(), { clientY: 372 - 5, altKey: true, pointerId: 1 });
    expect(stored()).toBe(String(295 / 900));
  });

  it("offers to minimise below the minimum and keeps the earlier height for Open", () => {
    const spoken: string[] = [];
    const stop = subscribeAnnouncements(({ message }) => spoken.push(message));
    show();
    fireEvent.keyDown(handle(), { key: "PageDown" });
    expect(stored()).toBe(String(600 / 900));

    vi.useFakeTimers();
    fireEvent.pointerDown(handle(), { button: 0, clientY: 672, pointerId: 1 });
    fireEvent.pointerMove(handle(), { clientY: 672 - 560, pointerId: 1 });
    act(() => {
      vi.advanceTimersToNextFrame();
    });
    vi.useRealTimers();
    expect(screen.getByText("Release to minimise to the mini bar")).toBeInTheDocument();

    fireEvent.pointerUp(handle(), { clientY: 672 - 560, pointerId: 1 });
    expect(mediaView()).toMatchObject({ cinema: null, mini: { share: true } });
    expect(spoken).toContain("Media area minimised");
    expect(stored()).toBe(String(600 / 900));
    stop();
  });

  it("has no handle when the chat column is too short", () => {
    vi.spyOn(Element.prototype, "clientHeight", "get").mockReturnValue(400);
    show();
    expect(screen.queryByRole("separator")).toBeNull();
  });
});
