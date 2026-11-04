import { act, fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PluginUi, PluginWebviewPlace } from "../../../api";
import { applyChange, type PluginUiState } from "../render/pluginUiStore";
import {
  createBridge,
  fitsBridge,
  isGestureReport,
  MAX_MESSAGE_BYTES,
  newGestureKey,
  rateLimiter,
} from "./bridge";
import { PluginWebView } from "./PluginWebView";

const sendEvent = vi.fn(() => Promise.resolve(null));
let uiListener: ((event: PluginUi) => void) | undefined;
vi.mock("../../../api", async (orig) => ({
  ...(await orig<typeof import("../../../api")>()),
  getApi: () => ({
    pluginUiEvent: sendEvent,
    on: (_name: string, handler: (event: PluginUi) => void) => {
      uiListener = handler;
      return () => {
        uiListener = undefined;
      };
    },
    pluginsUiTrees: () => Promise.resolve([]),
  }),
}));

const URL = "mumcord-web://localhost/snacks/index.html";

function frameOf(container: HTMLElement): HTMLIFrameElement {
  const frame = container.querySelector("iframe");
  if (frame === null) throw new Error("no frame");
  return frame;
}

function pageOf(container: HTMLElement): Window {
  const page = frameOf(container).contentWindow;
  if (page === null) throw new Error("no page");
  return page;
}

describe("bridge limits", () => {
  it("passes strings up to 16 KiB in UTF-8 only", () => {
    expect(fitsBridge("x".repeat(MAX_MESSAGE_BYTES))).toBe(true);
    expect(fitsBridge("x".repeat(MAX_MESSAGE_BYTES + 1))).toBe(false);
    expect(fitsBridge("ä".repeat(MAX_MESSAGE_BYTES / 2 + 1))).toBe(false);
    expect(fitsBridge({ text: "x" })).toBe(false);
  });

  it("allows 60 messages per second", () => {
    let now = 0;
    const allow = rateLimiter(() => now);
    for (let i = 0; i < 60; i++) expect(allow()).toBe(true);
    expect(allow()).toBe(false);
    now = 100;
    expect(allow()).toBe(true);
  });

  it("only takes messages from its own frame and queues posts until the page loaded", () => {
    const postMessage = vi.fn();
    const frame = { postMessage } as unknown as Window;
    const toPlugin = vi.fn();
    const onEscape = vi.fn();
    const bridge = createBridge({
      frame: () => frame,
      toPlugin,
      onEscape,
      gestureKey: "k",
      onGesture: vi.fn(),
    });
    bridge.receive(new MessageEvent("message", { data: "hi", source: window }));
    bridge.receive(new MessageEvent("message", { data: "hi", source: frame }));
    bridge.receive(new MessageEvent("message", { data: { other: 1 }, source: frame }));
    bridge.receive(
      new MessageEvent("message", { data: { mumcordWebview: "escape" }, source: frame }),
    );
    expect(toPlugin).toHaveBeenCalledTimes(1);
    expect(onEscape).toHaveBeenCalledTimes(1);
    bridge.post("early");
    expect(postMessage).not.toHaveBeenCalled();
    bridge.loaded();
    bridge.post("late");
    expect(postMessage).toHaveBeenNthCalledWith(1, { mumcordWebview: "key", key: "k" }, "*");
    expect(postMessage).toHaveBeenNthCalledWith(2, "early", "*");
    expect(postMessage).toHaveBeenNthCalledWith(3, "late", "*");
  });

  it("counts only gesture reports that carry the frame's key", () => {
    const frame = { postMessage: vi.fn() } as unknown as Window;
    const toPlugin = vi.fn();
    const onGesture = vi.fn();
    const key = newGestureKey();
    expect(key).toMatch(/^[0-9a-f]{32}$/);
    expect(newGestureKey()).not.toBe(key);
    const bridge = createBridge({
      frame: () => frame,
      toPlugin,
      onEscape: vi.fn(),
      gestureKey: key,
      onGesture,
    });
    const report = (data: unknown, source: Window = frame) => {
      bridge.receive(new MessageEvent("message", { data, source }));
    };
    report({ mumcordWebview: "gesture" });
    report({ mumcordWebview: "gesture", key: "guess" });
    report({ mumcordWebview: "gesture", key }, window);
    expect(onGesture).not.toHaveBeenCalled();
    expect(toPlugin).not.toHaveBeenCalled();
    report({ mumcordWebview: "gesture", key });
    expect(onGesture).toHaveBeenCalledTimes(1);
    expect(isGestureReport({ mumcordWebview: "escape", key }, key)).toBe(false);
  });
});

describe("web view state", () => {
  const empty: PluginUiState = {
    trees: new Map(),
    dialogs: new Map(),
    menuEntries: new Map(),
    webviews: new Map(),
  };
  const change = (c: PluginUi["change"]): PluginUi => ({ plugin: "snacks", change: c });

  it("opens, reloads on a new show and closes with its dialog", () => {
    let state = applyChange(empty, change({ type: "openDialog", title: "Snacks", subtitle: null }));
    state = applyChange(state, change({ type: "webviewShow", place: "dialog", url: URL }));
    const first = state.webviews.get("snacks");
    expect(first?.url).toBe(URL);
    state = applyChange(state, change({ type: "webviewShow", place: "dialog", url: URL }));
    expect(state.webviews.get("snacks")?.seq).not.toBe(first?.seq);
    expect(
      applyChange(state, change({ type: "webviewPost", message: "x", places: ["dialog"] })),
    ).toBe(state);
    state = applyChange(state, change({ type: "closeDialog" }));
    expect(state.webviews.size).toBe(0);
    state = applyChange(state, change({ type: "webviewShow", place: "panelPopover", url: URL }));
    // A hide for another place leaves the frame open.
    expect(applyChange(state, change({ type: "webviewHide", place: "sidePanel" }))).toBe(state);
    state = applyChange(state, change({ type: "webviewHide", place: "panelPopover" }));
    expect(state.webviews.size).toBe(0);
  });
});

describe("PluginWebView", () => {
  const view = { place: "panelPopover" as const, url: URL, seq: 1 };

  it("renders a sandboxed frame without same-origin and closes it when the page navigates", () => {
    const { container } = render(<PluginWebView plugin="snacks" webview={view} title="Snacks" />);
    const frame = frameOf(container);
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("src")).toBe(URL);
    expect(frame.title).toBe("Snacks");
    fireEvent.load(frame);
    expect(container.querySelector("iframe")).not.toBeNull();
    fireEvent.load(frame);
    expect(container.querySelector("iframe")).toBeNull();
  });

  it("hands page messages to its plugin and plugin posts to the page", () => {
    sendEvent.mockClear();
    const { container } = render(<PluginWebView plugin="snacks" webview={view} title="Snacks" />);
    const frame = frameOf(container);
    const page = pageOf(container);
    const post = vi.spyOn(page, "postMessage");
    fireEvent.load(frame);
    act(() => {
      window.dispatchEvent(new MessageEvent("message", { data: "ready", source: page }));
    });
    expect(sendEvent).toHaveBeenCalledWith("snacks", {
      type: "webviewMessage",
      place: "panelPopover",
      text: "ready",
    });
    act(() => {
      const post = (plugin: string, message: string, places: PluginWebviewPlace[]) =>
        uiListener?.({ plugin, change: { type: "webviewPost", message, places } });
      post("other", "no", ["panelPopover"]);
      post("snacks", "stage only", ["stage"]);
      post("snacks", "go", ["stage", "panelPopover"]);
    });
    const strings = post.mock.calls.filter(([message]) => typeof message === "string");
    expect(strings).toEqual([["go", "*"]]);
  });

  it("hands the page its key once and reports keyed gestures for its place", () => {
    sendEvent.mockClear();
    const { container, rerender } = render(
      <PluginWebView plugin="snacks" webview={view} title="Snacks" />,
    );
    const frame = frameOf(container);
    const page = pageOf(container);
    const post = vi.spyOn(page, "postMessage");
    fireEvent.load(frame);
    rerender(<PluginWebView plugin="snacks" webview={view} title="Snacks!" />);
    const keys = post.mock.calls.map(([message]) => message as { key?: string });
    expect(keys).toHaveLength(1);
    const key = keys[0]?.key;
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", { data: { mumcordWebview: "gesture" }, source: page }),
      );
    });
    expect(sendEvent).not.toHaveBeenCalled();
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", { data: { mumcordWebview: "gesture", key }, source: page }),
      );
    });
    expect(sendEvent).toHaveBeenCalledWith("snacks", {
      type: "webviewGesture",
      place: "panelPopover",
    });
  });

  it("moves focus out of the frame on the page's Escape signal", () => {
    const { container } = render(<PluginWebView plugin="snacks" webview={view} title="Snacks" />);
    const page = pageOf(container);
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", { data: { mumcordWebview: "escape" }, source: page }),
      );
    });
    expect(document.activeElement).toBe(container.querySelector(".plugin-webview"));
  });
});
