/**
 * Runs the real frame bootstrap (served by Rust in front of every plugin page) in jsdom, here as
 * its own top window, so `parent` is this window. jsdom cannot make trusted input events, so
 * this proves the refusals; the Rust and bridge tests cover the counting side.
 */
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import source from "../../../../../crates/plugins/src/api/webview_bootstrap.js?raw";

const post = vi.spyOn(window, "postMessage").mockImplementation(() => undefined);

function gestureReports(): unknown[] {
  return post.mock.calls
    .map(([message]) => message as unknown)
    .filter(
      (message) =>
        typeof message === "object" &&
        message !== null &&
        (message as { mumcordWebview?: unknown }).mumcordWebview === "gesture",
    );
}

function fromApp(key: string, source: Window | null = window) {
  window.dispatchEvent(
    new MessageEvent("message", { data: { mumcordWebview: "key", key }, source }),
  );
}

const SERVED = {
  mode: "light",
  scheme: "light",
  reducedMotion: false,
  tokens: { "--mc-color-text": "#111111", "--mc-space-1": "0.25rem" },
};
const themeBlock = document.createElement("script");
const themeEvents: unknown[] = [];
const root = document.documentElement;

function themeFromApp(theme: unknown, source: Window | null = window) {
  window.dispatchEvent(
    new MessageEvent("message", { data: { mumcordWebview: "theme", theme }, source }),
  );
}

describe("web view bootstrap", () => {
  beforeAll(() => {
    vi.useFakeTimers();
    // The page's code runs after the bootstrap and may replace what the bootstrap relies on.
    // jsdom runs no <script> elements, so the served text runs as a function body instead.
    // eslint-disable-next-line @typescript-eslint/no-implied-eval
    const bootstrap = new Function(source) as () => void;
    // An API 1.2 page: Rust serves the theme as a JSON block right before the bootstrap.
    themeBlock.type = "application/json";
    themeBlock.textContent = JSON.stringify(SERVED);
    const self = document.createElement("script");
    document.head.append(themeBlock, self);
    Object.defineProperty(document, "currentScript", { configurable: true, get: () => self });
    window.addEventListener("mumcord-theme-change", (event) => {
      themeEvents.push((event as CustomEvent).detail);
    });
    bootstrap();
    Reflect.deleteProperty(document, "currentScript");
    Event.prototype.stopImmediatePropagation = () => undefined;
  });
  afterEach(() => post.mockClear());

  it("takes its key only from the app and keeps it from the page's listeners", () => {
    const pageListener = vi.fn();
    window.addEventListener("message", pageListener, true);
    fromApp("forged", null);
    expect(pageListener).toHaveBeenCalledTimes(1);
    fromApp("secret");
    fromApp("second");
    expect(pageListener).toHaveBeenCalledTimes(1);
    window.removeEventListener("message", pageListener, true);
  });

  it("never reports clicks or key presses that a script made", () => {
    fromApp("secret");
    document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    document.body.click();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
    expect(gestureReports()).toEqual([]);
  });

  it("applies the served theme before the page's code and removes the block", () => {
    expect(root.style.getPropertyValue("--mc-color-text")).toBe("#111111");
    expect(root.style.getPropertyValue("--mc-space-1")).toBe("0.25rem");
    expect(root.dataset.mcMode).toBe("light");
    expect(root.dataset.mcScheme).toBe("light");
    expect(root.dataset.mcReducedMotion).toBe("false");
    expect(root.style.getPropertyValue("color-scheme")).toBe("light");
    expect(themeBlock.isConnected).toBe(false);
    // The initial values are no change (FR-13.16).
    expect(themeEvents).toEqual([]);
  });

  it("applies a live change from the app, fires the event and hides the message", () => {
    const pageListener = vi.fn();
    window.addEventListener("message", pageListener, true);
    themeFromApp({
      mode: "high-contrast",
      scheme: "dark",
      reducedMotion: true,
      tokens: { "--mc-color-text": "#eeeeee", "--mc-duration-fast": "0ms" },
    });
    expect(pageListener).not.toHaveBeenCalled();
    window.removeEventListener("message", pageListener, true);

    expect(root.style.getPropertyValue("--mc-color-text")).toBe("#eeeeee");
    expect(root.style.getPropertyValue("--mc-duration-fast")).toBe("0ms");
    // A token the new theme lacks does not keep its old value.
    expect(root.style.getPropertyValue("--mc-space-1")).toBe("");
    expect(root.dataset.mcMode).toBe("high-contrast");
    expect(root.dataset.mcScheme).toBe("dark");
    expect(root.dataset.mcReducedMotion).toBe("true");
    expect(root.style.getPropertyValue("color-scheme")).toBe("dark");
    expect(themeEvents).toEqual([{ mode: "high-contrast", scheme: "dark", reducedMotion: true }]);
  });

  it("ignores a theme from anyone but the app", () => {
    themeEvents.length = 0;
    const before = root.style.getPropertyValue("--mc-color-text");
    themeFromApp({ mode: "light", scheme: "light", tokens: { "--mc-color-text": "#000" } }, null);
    expect(root.style.getPropertyValue("--mc-color-text")).toBe(before);
    expect(themeEvents).toEqual([]);
  });
});
