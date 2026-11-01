import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { hideStartScreen, rememberStartTheme, START_THEME_KEY } from "./startScreen";

function addStartScreen(): HTMLElement {
  const screen = document.createElement("div");
  screen.id = "start-screen";
  screen.setAttribute("aria-busy", "true");
  document.body.append(screen);
  return screen;
}

function stubReducedMotion(reduce: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: reduce && query.includes("prefers-reduced-motion"),
    media: query,
  }));
}

describe("hideStartScreen", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  it("fades the start screen out and then removes it", () => {
    stubReducedMotion(false);
    const screen = addStartScreen();

    hideStartScreen();

    expect(screen.hasAttribute("data-leaving")).toBe(true);
    expect(screen.getAttribute("aria-busy")).toBe("false");
    expect(screen.isConnected).toBe(true);
    screen.dispatchEvent(new Event("transitionend"));
    expect(screen.isConnected).toBe(false);
  });

  it("removes it even when the fade never reports its end", () => {
    stubReducedMotion(false);
    const screen = addStartScreen();

    hideStartScreen();
    vi.advanceTimersByTime(300);

    expect(screen.isConnected).toBe(false);
  });

  it("removes it at once with reduced motion", () => {
    stubReducedMotion(true);
    const screen = addStartScreen();

    hideStartScreen();

    expect(screen.isConnected).toBe(false);
  });

  it("does nothing without a start screen or when it is already leaving", () => {
    stubReducedMotion(false);
    expect(() => {
      hideStartScreen();
    }).not.toThrow();

    const screen = addStartScreen();
    hideStartScreen();
    const listener = vi.spyOn(screen, "addEventListener");
    hideStartScreen();
    expect(listener).not.toHaveBeenCalled();
  });
});

describe("rememberStartTheme", () => {
  afterEach(() => {
    localStorage.clear();
  });

  it("stores a fixed theme for the next start", () => {
    rememberStartTheme("light");
    expect(localStorage.getItem(START_THEME_KEY)).toBe("light");
  });

  it("forgets it when the theme follows the system again", () => {
    localStorage.setItem(START_THEME_KEY, "dark");
    rememberStartTheme("system");
    expect(localStorage.getItem(START_THEME_KEY)).toBeNull();
  });
});
