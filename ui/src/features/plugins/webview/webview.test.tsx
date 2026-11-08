import { readFileSync } from "node:fs";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginFrameMute, PluginUi, PluginWebviewPlace } from "../../../api";
import { applyLanguage } from "../../../i18n";
import axe from "axe-core";
import { StageShown } from "../../media/stageContext";
import { applyChange, type PluginUiState, type PluginWebview } from "../render/pluginUiStore";
import {
  createBridge,
  inOrder,
  isGestureReport,
  MAX_MESSAGE_BYTES,
  MAX_STAGE_MESSAGE_BYTES,
  maxMessageBytes,
  messageBytes,
  newGestureKey,
  rateLimiter,
} from "./bridge";
import { resetFrameMute, usePluginFramesDeafen } from "./frameMute";
import { HEARTBEAT_TIMEOUT_MS } from "./heartbeat";
import { PluginFrame } from "./PluginFrame";

const api = vi.hoisted(() => ({
  sendEvent: vi.fn<(plugin: string, event: unknown) => Promise<null>>(() => Promise.resolve(null)),
  framesMute: vi.fn<(plugin: string | null, muted: boolean) => Promise<null>>(() =>
    Promise.resolve(null),
  ),
  muteState: { all: false, plugins: [] as string[] },
  listeners: new Map<string, (payload: unknown) => void>(),
  plugins: [] as { id: string; name: string; source: { kind: string } }[],
}));
vi.mock("../../../api", async (orig) => {
  // One object, so the stores bind to it once.
  const mocked = {
    pluginUiEvent: api.sendEvent,
    pluginFramesMute: api.framesMute,
    pluginFramesMuteState: () => Promise.resolve({ ...api.muteState }),
    pluginsList: () => Promise.resolve({ noPlugins: false, allOff: false, plugins: api.plugins }),
    on: (name: string, handler: (payload: unknown) => void) => {
      const key = `${name}:${String(Math.random())}`;
      api.listeners.set(key, handler);
      return () => {
        api.listeners.delete(key);
      };
    },
    pluginsUiTrees: () => Promise.resolve([]),
  };
  return { ...(await orig<typeof import("../../../api")>()), getApi: () => mocked };
});

const deaf = vi.hoisted(() => ({ self: false, server: false, voice: 1 }));
vi.mock("../../channels/serverStore", () => ({
  useVoiceServerId: () => deaf.voice,
  useServerStateOf: () => ({}),
  selfUser: () => ({ selfDeaf: deaf.self, serverDeaf: deaf.server }),
}));

function emit(name: string, payload: unknown) {
  act(() => {
    for (const [key, handler] of api.listeners) if (key.startsWith(`${name}:`)) handler(payload);
  });
}

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

function fromPage(page: Window, data: unknown) {
  act(() => {
    window.dispatchEvent(new MessageEvent("message", { data, source: page }));
  });
}

/** axe on the chrome only: the sealed page is the plugin's, and jsdom cannot reach into it. */
async function axeViolations(container: Element): Promise<string[]> {
  const results = await axe.run(container, { iframes: false });
  return results.violations.map((violation) => violation.id);
}

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api.muteState = { all: false, plugins: [] };
  api.sendEvent.mockClear();
  api.framesMute.mockClear();
  api.plugins = [{ id: "snacks", name: "Snacks", source: { kind: "official" } }];
  resetFrameMute();
});

describe("bridge limits", () => {
  it("limits each place like Rust: 64 KiB on the stage, 16 KiB elsewhere (FR-9.37)", () => {
    expect(maxMessageBytes("stage")).toBe(64 * 1024);
    for (const place of ["panelPopover", "dialog", "sidePanel"] as const)
      expect(maxMessageBytes(place)).toBe(16 * 1024);
    // One source of truth: the Rust limits, which check every message again.
    const rust = readFileSync("crates/plugins/src/api/webview.rs", "utf8");
    const constant = (name: string) => {
      const match = new RegExp(`pub const ${name}: usize = (\\d+) \\* 1024;`).exec(rust);
      if (match === null) throw new Error(`${name} not found`);
      return Number(match[1]) * 1024;
    };
    expect(constant("MAX_MESSAGE_BYTES")).toBe(MAX_MESSAGE_BYTES);
    expect(constant("MAX_STAGE_MESSAGE_BYTES")).toBe(MAX_STAGE_MESSAGE_BYTES);
  });

  it("measures messages in UTF-8 and refuses ones above its limit with their size only", () => {
    expect(messageBytes("ä")).toBe(2);
    const toPlugin = vi.fn();
    const tooLarge: number[] = [];
    const dropped = vi.fn();
    const bridge = createBridge({
      frame: () => null,
      toPlugin,
      maxBytes: 4,
      onTooLarge: (bytes) => tooLarge.push(bytes),
      onDropped: dropped,
      onEscape: vi.fn(),
      gestureKey: "k",
      onGesture: vi.fn(),
    });
    bridge.receiveData("xxxx");
    bridge.receiveData("äää");
    bridge.receiveData({ text: "x" });
    expect(toPlugin.mock.calls).toEqual([["xxxx"]]);
    expect(tooLarge).toEqual([6]);
    expect(dropped).toHaveBeenCalledTimes(1);
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
      maxBytes: MAX_MESSAGE_BYTES,
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
    bridge.mute(true);
    expect(postMessage).not.toHaveBeenCalled();
    bridge.loaded();
    bridge.post("late");
    expect(postMessage.mock.calls).toEqual([
      [{ mumcordWebview: "key", key: "k" }, "*"],
      ["early", "*"],
      [{ mumcordWebview: "mute", muted: true }, "*"],
      ["late", "*"],
    ]);
  });

  it("routes the bootstrap's heartbeat, F6 and sound signals and never passes them on", () => {
    const frame = { postMessage: vi.fn() } as unknown as Window;
    const calls: string[] = [];
    const bridge = createBridge({
      frame: () => frame,
      maxBytes: MAX_MESSAGE_BYTES,
      toPlugin: () => calls.push("plugin"),
      onEscape: () => calls.push("escape"),
      onF6: () => calls.push("f6"),
      onHeartbeat: () => calls.push("heartbeat"),
      onSound: () => calls.push("sound"),
      onDropped: () => calls.push("dropped"),
      gestureKey: "k",
      onGesture: vi.fn(),
    });
    for (const kind of ["heartbeat", "f6", "sound", "escape", "mute", "key"]) {
      bridge.receive(
        new MessageEvent("message", { data: { mumcordWebview: kind }, source: frame }),
      );
    }
    expect(calls).toEqual(["heartbeat", "f6", "sound", "escape", "dropped", "dropped"]);
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
      maxBytes: MAX_MESSAGE_BYTES,
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

describe("page messages to the plugin", () => {
  it("go out one after the other, so the plugin gets them in the page's order", async () => {
    const started: string[] = [];
    const finish: (() => void)[] = [];
    const send = inOrder(
      (text: string) =>
        new Promise<void>((resolve, reject) => {
          started.push(text);
          // The first call fails: the next one must still go out.
          finish.push(
            text === "running"
              ? () => {
                  reject(new Error("dropped"));
                }
              : resolve,
          );
        }),
    );
    send("running");
    send("blocked");
    send("next");
    expect(started).toEqual(["running"]);
    finish[0]?.();
    await vi.waitFor(() => {
      expect(started).toEqual(["running", "blocked"]);
    });
    finish[1]?.();
    await vi.waitFor(() => {
      expect(started).toEqual(["running", "blocked", "next"]);
    });
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
  const show = (place: PluginWebviewPlace) => change({ type: "webviewShow", place, url: URL });

  it("keeps one frame per place, reloads on a new show and closes the dialog's with it", () => {
    let state = applyChange(empty, change({ type: "openDialog", title: "Snacks", subtitle: null }));
    state = applyChange(state, show("dialog"));
    const first = state.webviews.get("snacks")?.get("dialog");
    expect(first?.url).toBe(URL);
    state = applyChange(state, show("dialog"));
    expect(state.webviews.get("snacks")?.get("dialog")?.seq).not.toBe(first?.seq);
    state = applyChange(state, show("stage"));
    state = applyChange(state, show("sidePanel"));
    expect([...(state.webviews.get("snacks")?.keys() ?? [])]).toEqual([
      "dialog",
      "stage",
      "sidePanel",
    ]);
    expect(
      applyChange(state, change({ type: "webviewPost", message: "x", places: ["dialog"] })),
    ).toBe(state);
    state = applyChange(state, change({ type: "closeDialog" }));
    expect([...(state.webviews.get("snacks")?.keys() ?? [])]).toEqual(["stage", "sidePanel"]);
    // A hide for a place without a frame changes nothing.
    expect(applyChange(state, change({ type: "webviewHide", place: "panelPopover" }))).toBe(state);
    state = applyChange(state, change({ type: "webviewHide", place: "stage" }));
    state = applyChange(state, change({ type: "webviewHide", place: "sidePanel" }));
    expect(state.webviews.size).toBe(0);
  });

  it("keeps the side panel's size hint", () => {
    const state = applyChange(
      empty,
      change({ type: "webviewSizeHint", minWidth: 320, minHeight: 200 }),
    );
    expect(state.sizeHints?.get("snacks")).toEqual({ minWidth: 320, minHeight: 200 });
  });

  it("keeps the side panel's status until it is cleared or the panel closes", () => {
    const open = applyChange(empty, change({ type: "webviewShow", place: "sidePanel", url: URL }));
    let state = applyChange(open, change({ type: "sidePanelStatus", text: "3 drawing" }));
    expect(state.sidePanelStatus?.get("snacks")).toBe("3 drawing");
    state = applyChange(state, change({ type: "sidePanelStatus", text: "" }));
    expect(state.sidePanelStatus?.has("snacks")).toBe(false);
    state = applyChange(state, change({ type: "sidePanelStatus", text: "relay" }));
    state = applyChange(state, change({ type: "webviewHide", place: "sidePanel" }));
    expect(state.sidePanelStatus?.has("snacks")).toBe(false);
  });
});

const PLACES: PluginWebviewPlace[] = ["stage", "sidePanel", "dialog", "panelPopover"];
const viewIn = (place: PluginWebviewPlace, seq = 1): PluginWebview => ({ place, url: URL, seq });

describe("PluginFrame", () => {
  const view = viewIn("panelPopover");

  it.each(PLACES)(
    "mounts in the %s with its tag and accessible name, without axe findings",
    async (place) => {
      const { container } = render(<PluginFrame plugin="snacks" webview={viewIn(place)} />);
      await screen.findByText("Plugin content · Snacks");
      expect(frameOf(container).title).toBe("Snacks, plugin content");
      expect(container.querySelector(`.plugin-frame[data-place="${place}"]`)).not.toBeNull();
      expect(await axeViolations(container)).toEqual([]);
    },
  );

  it("tags unofficial plugins", async () => {
    api.plugins = [{ id: "snacks", name: "Snacks", source: { kind: "repo" } }];
    render(<PluginFrame plugin="snacks" webview={view} />);
    expect(await screen.findByText("Plugin content · Snacks · Unofficial")).toBeInTheDocument();
  });

  it("renders a sandboxed frame without same-origin and closes it when the page navigates", () => {
    const { container } = render(<PluginFrame plugin="snacks" webview={view} />);
    const frame = frameOf(container);
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("src")).toBe(URL);
    fireEvent.load(frame);
    expect(container.querySelector("iframe")).not.toBeNull();
    fireEvent.load(frame);
    expect(container.querySelector("iframe")).toBeNull();
  });

  it("takes page messages up to the place's limit: 64 KiB on the stage (FR-9.37)", () => {
    const { container } = render(<PluginFrame plugin="snacks" webview={viewIn("stage")} />);
    const page = pageOf(container);
    fireEvent.load(frameOf(container));
    const text = "x".repeat(40 * 1024);
    fromPage(page, text);
    expect(api.sendEvent).toHaveBeenCalledWith("snacks", {
      type: "webviewMessage",
      place: "stage",
      text,
    });
  });

  it("refuses a page message above 16 KiB in the side panel and logs only size and place", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { container } = render(<PluginFrame plugin="snacks" webview={viewIn("sidePanel")} />);
    const page = pageOf(container);
    fireEvent.load(frameOf(container));
    const text = "secret".padEnd(20 * 1024, "x");
    fromPage(page, text);
    expect(api.sendEvent).not.toHaveBeenCalledWith(
      "snacks",
      expect.objectContaining({ type: "webviewMessage" }),
    );
    expect(warn).toHaveBeenCalledTimes(1);
    const logged = JSON.stringify(warn.mock.calls);
    expect(logged).toContain(String(20 * 1024));
    expect(logged).toContain("sidePanel");
    expect(logged).not.toContain("secret");
    warn.mockRestore();
  });

  it("hands page messages to its plugin and plugin posts to its place only", () => {
    const { container } = render(<PluginFrame plugin="snacks" webview={view} />);
    const frame = frameOf(container);
    const page = pageOf(container);
    const post = vi.spyOn(page, "postMessage");
    fireEvent.load(frame);
    fromPage(page, "ready");
    expect(api.sendEvent).toHaveBeenCalledWith("snacks", {
      type: "webviewMessage",
      place: "panelPopover",
      text: "ready",
    });
    const postTo = (plugin: string, message: string, places: PluginWebviewPlace[]) => {
      emit("pluginUi", { plugin, change: { type: "webviewPost", message, places } });
    };
    postTo("other", "no", ["panelPopover"]);
    postTo("snacks", "stage only", ["stage"]);
    postTo("snacks", "go", ["stage", "panelPopover"]);
    const strings = post.mock.calls.filter(([message]) => typeof message === "string");
    expect(strings).toEqual([["go", "*"]]);
  });

  it("hands the page its key once and reports keyed gestures for its place", () => {
    const { container, rerender } = render(<PluginFrame plugin="snacks" webview={view} />);
    const frame = frameOf(container);
    const page = pageOf(container);
    const post = vi.spyOn(page, "postMessage");
    fireEvent.load(frame);
    rerender(<PluginFrame plugin="snacks" webview={view} onEscape={vi.fn()} />);
    const keys = post.mock.calls
      .map(([message]) => message as { mumcordWebview?: string; key?: string })
      .filter((message) => message.mumcordWebview === "key");
    expect(keys).toHaveLength(1);
    const key = keys[0]?.key;
    fromPage(page, { mumcordWebview: "gesture" });
    expect(api.sendEvent).not.toHaveBeenCalled();
    fromPage(page, { mumcordWebview: "gesture", key });
    expect(api.sendEvent).toHaveBeenCalledWith("snacks", {
      type: "webviewGesture",
      place: "panelPopover",
    });
  });

  it("returns focus to Mumcord on Escape and F6; only Escape reaches the place", () => {
    const onEscape = vi.fn();
    const { container } = render(
      <PluginFrame plugin="snacks" webview={viewIn("dialog")} onEscape={onEscape} />,
    );
    const page = pageOf(container);
    const wrapper = container.querySelector(".plugin-webview");
    fromPage(page, { mumcordWebview: "f6" });
    expect(document.activeElement).toBe(wrapper);
    expect(onEscape).not.toHaveBeenCalled();
    act(() => {
      (document.activeElement as HTMLElement).blur();
    });
    fromPage(page, { mumcordWebview: "escape" });
    expect(document.activeElement).toBe(wrapper);
    expect(onEscape).toHaveBeenCalledTimes(1);
  });

  it("takes the keyboard back from the page's frame on Escape and F6", () => {
    const windowFocus = vi.spyOn(window, "focus").mockImplementation(() => {});
    const { container } = render(
      <PluginFrame plugin="snacks" webview={viewIn("stage")} onEscape={vi.fn()} />,
    );
    const page = pageOf(container);
    fromPage(page, { mumcordWebview: "escape" });
    expect(windowFocus).toHaveBeenCalledTimes(1);
    fromPage(page, { mumcordWebview: "f6" });
    expect(windowFocus).toHaveBeenCalledTimes(2);
    windowFocus.mockRestore();
  });
});

describe("PluginFrame heartbeat", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("removes a silent page, says it stopped and reloads it", () => {
    const { container } = render(<PluginFrame plugin="snacks" webview={viewIn("stage")} />);
    fireEvent.load(frameOf(container));
    const page = pageOf(container);
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    fromPage(page, { mumcordWebview: "heartbeat" });
    act(() => {
      vi.advanceTimersByTime(2500);
    });
    expect(container.querySelector("iframe")).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(HEARTBEAT_TIMEOUT_MS);
    });
    expect(container.querySelector("iframe")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent(/stopped responding/);
    fireEvent.click(screen.getByRole("button", { name: "Reload" }));
    expect(container.querySelector("iframe")).not.toBeNull();
    expect(document.activeElement).toBe(container.querySelector(".plugin-webview"));
  });

  it("does not watch a hidden stage tab", () => {
    const { container } = render(
      <StageShown.Provider value={false}>
        <PluginFrame plugin="snacks" webview={viewIn("stage")} />
      </StageShown.Provider>,
    );
    fireEvent.load(frameOf(container));
    act(() => {
      vi.advanceTimersByTime(HEARTBEAT_TIMEOUT_MS * 3);
    });
    expect(container.querySelector("iframe")).not.toBeNull();
  });
});

describe("PluginFrame sound", () => {
  const muteMessages = (post: { mock: { calls: unknown[][] } }) =>
    post.mock.calls
      .map(([message]) => message as { mumcordWebview?: string; muted?: boolean })
      .filter((message) => message.mumcordWebview === "mute")
      .map((message) => message.muted);

  it("shows the mute button once the page plays sound and mutes through Rust", async () => {
    const { container } = render(<PluginFrame plugin="snacks" webview={view()} />);
    await screen.findByText("Plugin content · Snacks");
    const page = pageOf(container);
    const post = vi.spyOn(page, "postMessage");
    fireEvent.load(frameOf(container));
    expect(muteMessages(post)).toEqual([false]);
    expect(screen.queryByRole("button", { name: /Mute/ })).toBeNull();
    fromPage(page, { mumcordWebview: "sound" });
    fireEvent.click(screen.getByRole("button", { name: "Mute sounds from Snacks" }));
    expect(api.framesMute).toHaveBeenCalledWith("snacks", true);
    emit("pluginFrameMute", { plugin: "snacks", muted: true } satisfies PluginFrameMute);
    expect(muteMessages(post)).toEqual([false, true]);
    expect(screen.getByRole("button", { name: "Unmute sounds from Snacks" })).toBeInTheDocument();
    // Another plugin's mute changes nothing here.
    emit("pluginFrameMute", { plugin: "other", muted: false } satisfies PluginFrameMute);
    expect(muteMessages(post)).toEqual([false, true]);
    expect(await axeViolations(container)).toEqual([]);
  });

  it("replays the stored mute state to a window that opens after the mute", async () => {
    api.muteState = { all: false, plugins: ["snacks"] };
    const { container } = render(<PluginFrame plugin="snacks" webview={view()} />);
    await screen.findByText("Plugin content · Snacks");
    const page = pageOf(container);
    const post = vi.spyOn(page, "postMessage");
    fireEvent.load(frameOf(container));
    fromPage(page, { mumcordWebview: "sound" });
    await screen.findByRole("button", { name: "Unmute sounds from Snacks" });
    expect(muteMessages(post).at(-1)).toBe(true);
  });

  it("mutes every frame while deafened and resends the state to a frame that opens later", () => {
    renderHook(() => {
      usePluginFramesDeafen();
    });
    emit("pluginFrameMute", { plugin: null, muted: true } satisfies PluginFrameMute);
    const { container } = render(<PluginFrame plugin="snacks" webview={view()} />);
    const page = pageOf(container);
    const post = vi.spyOn(page, "postMessage");
    fireEvent.load(frameOf(container));
    expect(muteMessages(post)).toEqual([true]);
    emit("pluginFrameMute", { plugin: null, muted: false } satisfies PluginFrameMute);
    expect(muteMessages(post)).toEqual([true, false]);
  });

  it("asks Rust to mute all frames while the user is deafened", () => {
    deaf.self = false;
    const { rerender } = renderHook(() => {
      usePluginFramesDeafen();
    });
    expect(api.framesMute).toHaveBeenLastCalledWith(null, false);
    deaf.self = true;
    rerender();
    expect(api.framesMute).toHaveBeenLastCalledWith(null, true);
    deaf.self = false;
    deaf.server = true;
    rerender();
    expect(api.framesMute).toHaveBeenCalledTimes(2);
    deaf.server = false;
  });

  function view(): PluginWebview {
    return viewIn("sidePanel");
  }
});
