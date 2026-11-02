import { afterEach, describe, expect, it, vi } from "vitest";
import type { PluginNode, PluginUi } from "../../../api";
import { applyChange } from "../render/pluginUiStore";
import { cardAnnouncement, createLineAnnouncer, lineView, type LineView } from "./lineAnnouncement";
import { applySupersede, type PluginChatState } from "./pluginChatStore";

const CHAT: PluginChatState = {
  placed: new Map(),
  superseded: new Map(),
  replacing: new Set(),
  commands: [],
  names: new Map([["poll", "Poll"]]),
};

const UI = applyChange(
  {
    trees: new Map(),
    dialogs: new Map(),
    menuEntries: new Map(),
    webviews: new Map(),
  },
  {
    plugin: "poll",
    change: {
      type: "setTree",
      slot: { kind: "chatCard", id: "p1" },
      serverId: 1,
      nodes: [
        { component: { type: "stack" }, animation: null, children: [1] },
        {
          component: { type: "text", text: "Lunch at noon?", style: "heading", color: null },
          animation: null,
          children: [],
        },
      ] as unknown as PluginNode[],
    },
  } as unknown as PluginUi,
);

afterEach(() => {
  vi.useRealTimers();
});

describe("lineView", () => {
  it("reads a line nobody replaced as itself", () => {
    expect(lineView(CHAT, UI, "m1")).toEqual({ kind: "line" });
  });

  it("reads a replaced line as its card with the card's first text", () => {
    const chat = applySupersede(CHAT, "poll", "p1", "m1");
    expect(lineView(chat, UI, "m1")).toEqual({
      kind: "card",
      plugin: "Poll",
      summary: "Lunch at noon?",
    });
    const t = ((key: string, o: { plugin: string }) => `${key}|${o.plugin}`) as never;
    expect(cardAnnouncement(t, { kind: "card", plugin: "Poll", summary: "Lunch?" })).toBe(
      "card.label|Poll: Lunch?",
    );
  });

  it("hides a line a plugin hid", () => {
    const chat = applySupersede(CHAT, "poll", null, "m1");
    expect(lineView(chat, UI, "m1")).toEqual({ kind: "hidden" });
  });

  it("reads a line whose card is gone as itself, like it shows", () => {
    const chat = applySupersede(CHAT, "poll", "gone", "m1");
    expect(lineView(chat, UI, "m1")).toEqual({ kind: "line" });
  });
});

describe("createLineAnnouncer", () => {
  it("waits for a plugin that hides the line after it arrived, then stays silent", () => {
    vi.useFakeTimers();
    let view: LineView = { kind: "line" };
    const speak = vi.fn();
    const lines = createLineAnnouncer(
      () => view,
      () => true,
      500,
    );
    lines.resolve("m1", speak);
    view = { kind: "hidden" };
    vi.advanceTimersByTime(500);
    expect(speak).not.toHaveBeenCalled();
  });

  it("announces the card when it replaces the line within the grace period", () => {
    vi.useFakeTimers();
    let view: LineView = { kind: "line" };
    const speak = vi.fn();
    const lines = createLineAnnouncer(
      () => view,
      () => true,
      500,
    );
    lines.resolve("m1", speak);
    vi.advanceTimersByTime(200);
    view = { kind: "card", plugin: "Poll", summary: null };
    vi.advanceTimersByTime(300);
    expect(speak).toHaveBeenCalledExactlyOnceWith({ kind: "card", plugin: "Poll", summary: null });
  });

  it("announces at once without plugins and nothing after dispose", () => {
    vi.useFakeTimers();
    const speak = vi.fn();
    let plugins = false;
    const lines = createLineAnnouncer(
      () => ({ kind: "line" }),
      () => plugins,
    );
    lines.resolve("m1", speak);
    expect(speak).toHaveBeenCalledTimes(1);
    plugins = true;
    lines.resolve("m2", speak);
    lines.dispose();
    vi.runAllTimers();
    expect(speak).toHaveBeenCalledTimes(1);
  });
});
