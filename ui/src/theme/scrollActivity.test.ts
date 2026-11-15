import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installScrollActivity, SCROLL_IDLE_MS } from "./scrollActivity";

describe("installScrollActivity (#875)", () => {
  let uninstall: () => void;
  let list: HTMLDivElement;

  beforeEach(() => {
    vi.useFakeTimers();
    uninstall = installScrollActivity(document);
    list = document.body.appendChild(document.createElement("div"));
  });

  afterEach(() => {
    uninstall();
    list.remove();
    vi.useRealTimers();
  });

  const scroll = () => list.dispatchEvent(new Event("scroll"));

  it("marks an element while it scrolls and unmarks it once scrolling stops", () => {
    expect(list.hasAttribute("data-scrolling")).toBe(false);
    scroll();
    expect(list.hasAttribute("data-scrolling")).toBe(true);
    vi.advanceTimersByTime(SCROLL_IDLE_MS - 1);
    expect(list.hasAttribute("data-scrolling")).toBe(true);
    vi.advanceTimersByTime(1);
    expect(list.hasAttribute("data-scrolling")).toBe(false);
  });

  it("keeps the mark while scrolling goes on", () => {
    scroll();
    vi.advanceTimersByTime(SCROLL_IDLE_MS - 100);
    scroll();
    vi.advanceTimersByTime(SCROLL_IDLE_MS - 100);
    expect(list.hasAttribute("data-scrolling")).toBe(true);
    vi.advanceTimersByTime(100);
    expect(list.hasAttribute("data-scrolling")).toBe(false);
  });

  it("stops marking once uninstalled", () => {
    uninstall();
    scroll();
    expect(list.hasAttribute("data-scrolling")).toBe(false);
  });
});
