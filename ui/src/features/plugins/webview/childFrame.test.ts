import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi, type PluginFrameBounds } from "../../../api";
import { createBridge } from "./bridge";
import {
  childFramesUsed,
  listenFrame,
  newFrameId,
  pagePost,
  sameBounds,
  toDelivery,
  trackPlacement,
  visibleBounds,
  type Scheduler,
} from "./childFrame";

const viewport = { left: 0, top: 0, right: 1000, bottom: 800 };

describe("visibleBounds", () => {
  it("scales a fully visible frame to device pixels", () => {
    const rect = { left: 10.25, top: 20, right: 310.25, bottom: 220 };
    expect(visibleBounds(rect, [viewport], 2)).toEqual({ x: 21, y: 40, width: 600, height: 400 });
  });

  it("cuts a frame to its scrolled container and the window", () => {
    const panel = { left: 0, top: 100, right: 400, bottom: 500 };
    const rect = { left: 50, top: 50, right: 1200, bottom: 300 };
    expect(visibleBounds(rect, [viewport, panel], 1)).toEqual({
      x: 50,
      y: 100,
      width: 350,
      height: 200,
    });
  });

  it("hides a frame that is scrolled away or has no size", () => {
    const below = { left: 0, top: 900, right: 100, bottom: 1000 };
    expect(visibleBounds(below, [viewport], 1)).toBeNull();
    const empty = { left: 10, top: 10, right: 10, bottom: 10 };
    expect(visibleBounds(empty, [viewport], 1)).toBeNull();
  });

  it("rounds edges, so neighbours neither overlap nor leave a gap", () => {
    const a = visibleBounds({ left: 0, top: 0, right: 100.4, bottom: 10 }, [viewport], 1.5);
    const b = visibleBounds({ left: 100.4, top: 0, right: 200, bottom: 10 }, [viewport], 1.5);
    expect(a).not.toBeNull();
    expect(b).not.toBeNull();
    if (a && b) expect(a.x + a.width).toBe(b.x);
  });
});

describe("trackPlacement", () => {
  function manualFrames() {
    let pending: (() => void) | null = null;
    const scheduler: Scheduler = {
      request: (callback) => {
        pending = callback;
        return 1;
      },
      cancel: () => {
        pending = null;
      },
    };
    return {
      scheduler,
      step: () => {
        const run = pending;
        pending = null;
        run?.();
      },
      waiting: () => pending !== null,
    };
  }

  it("reports the first placement and then only changes", () => {
    const frames = manualFrames();
    const element = document.createElement("div");
    const place: PluginFrameBounds = { x: 0, y: 0, width: 100, height: 100 };
    let current: PluginFrameBounds | null = place;
    const seen: (PluginFrameBounds | null)[] = [];
    const stop = trackPlacement(
      element,
      (b) => seen.push(b),
      () => current,
      frames.scheduler,
    );
    frames.step();
    frames.step();
    current = { ...place, x: 30 }; // the side panel opened next to it
    frames.step();
    current = null; // its tab was hidden
    frames.step();
    frames.step();
    current = { ...place, x: 30 };
    frames.step();
    expect(seen).toEqual([place, { ...place, x: 30 }, null, { ...place, x: 30 }]);
    stop();
    expect(frames.waiting()).toBe(false);
  });

  it("compares bounds by value", () => {
    const a = { x: 1, y: 2, width: 3, height: 4 };
    expect(sameBounds(a, { ...a })).toBe(true);
    expect(sameBounds(a, { ...a, height: 5 })).toBe(false);
    expect(sameBounds(null, null)).toBe(true);
    expect(sameBounds(a, null)).toBe(false);
  });
});

describe("message routing", () => {
  let api: MockApi;
  beforeEach(() => {
    api = createMockApi();
    setApi(api);
  });
  afterEach(() => {
    setApi(undefined);
  });

  it("uses iframes outside the macOS app", () => {
    expect(childFramesUsed()).toBe(false);
  });

  it("makes random hex frame ids the Rust side accepts", () => {
    const id = newFrameId();
    expect(id).toMatch(/^[0-9a-f]{16}$/);
    expect(newFrameId()).not.toBe(id);
  });

  it("turns bridge posts into deliveries and drops anything else", () => {
    expect(toDelivery("hello")).toEqual({ type: "text", text: "hello" });
    expect(toDelivery({ mumcordWebview: "key", key: "abc" })).toEqual({ type: "key", key: "abc" });
    expect(toDelivery({ mumcordWebview: "mute", muted: true })).toEqual({
      type: "mute",
      muted: true,
    });
    for (const other of [42, null, { mumcordWebview: "key" }, { mumcordWebview: "escape" }]) {
      expect(toDelivery(other)).toBeNull();
    }
  });

  it("delivers each frame's events to that frame only", () => {
    const a: unknown[] = [];
    const b: unknown[] = [];
    const stopA = listenFrame("aaaaaaaaaaaaaaaa", (change) => a.push(change));
    const stopB = listenFrame("bbbbbbbbbbbbbbbb", (change) => b.push(change));
    api.mock.emit("pluginFrame", { frame: "aaaaaaaaaaaaaaaa", change: { type: "loaded" } });
    api.mock.emit("pluginFrame", {
      frame: "bbbbbbbbbbbbbbbb",
      change: { type: "message", text: "hi" },
    });
    api.mock.emit("pluginFrame", { frame: "cccccccccccccccc", change: { type: "loaded" } });
    stopA();
    api.mock.emit("pluginFrame", { frame: "aaaaaaaaaaaaaaaa", change: { type: "navigated" } });
    expect(a).toEqual([{ type: "loaded" }]);
    expect(b).toEqual([{ type: "message", text: "hi" }]);
    stopB();
  });

  it("feeds child web view posts through the same bridge rules as an iframe", () => {
    const sent: unknown[] = [];
    const toPlugin: string[] = [];
    let escapes = 0;
    let gestures = 0;
    const bridge = createBridge({
      frame: () => ({ postMessage: (data) => sent.push(toDelivery(data)) }),
      toPlugin: (text) => toPlugin.push(text),
      onEscape: () => {
        escapes += 1;
      },
      gestureKey: "secret",
      onGesture: () => {
        gestures += 1;
      },
    });
    bridge.post("early");
    bridge.loaded();
    bridge.mute(true);
    expect(sent).toEqual([
      { type: "key", key: "secret" },
      { type: "text", text: "early" },
      { type: "mute", muted: true },
    ]);
    bridge.receiveData(pagePost({ type: "message", text: "move 3" }));
    bridge.receiveData(pagePost({ type: "signal", kind: "escape", key: null }));
    bridge.receiveData(pagePost({ type: "signal", kind: "gesture", key: "guess" }));
    bridge.receiveData(pagePost({ type: "signal", kind: "gesture", key: "secret" }));
    expect(toPlugin).toEqual(["move 3"]);
    expect(escapes).toBe(1);
    expect(gestures).toBe(1);
  });
});
