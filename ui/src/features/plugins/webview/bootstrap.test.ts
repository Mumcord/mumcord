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

describe("web view bootstrap", () => {
  beforeAll(() => {
    vi.useFakeTimers();
    // The page's code runs after the bootstrap and may replace what the bootstrap relies on.
    // jsdom runs no <script> elements, so the served text runs as a function body instead.
    // eslint-disable-next-line @typescript-eslint/no-implied-eval
    const bootstrap = new Function(source) as () => void;
    bootstrap();
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
});
