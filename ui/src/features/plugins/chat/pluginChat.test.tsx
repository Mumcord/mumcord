import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatMessage, InstalledPlugin, PluginCommand, PluginUi } from "../../../api";
import type { MockApi } from "../../../api/mock";
import { axeViolations } from "../../../test/axe";
import { connectedMock, resetAll } from "../../channels/testUtils";
import { Composer } from "../../chat/Composer";
import { PluginLine } from "./PluginChatCard";
import {
  applySupersede,
  applyUi,
  cardKey,
  cardsAfter,
  channelOfListKey,
  commandOf,
  matchCommands,
  resetPluginChatStore,
  viaPlugin,
  type PlacedCard,
  type PluginChatState,
} from "./pluginChatStore";
import { reportMention } from "./pluginMentions";

const ROLL: PluginCommand = {
  pluginId: "dice",
  pluginName: "Dice roller",
  name: "roll",
  description: "Roll dice",
  usage: "/roll 2d6",
  takenBy: null,
};
const TAKEN: PluginCommand = { ...ROLL, pluginId: "dice2", pluginName: "Dice 2", takenBy: "Dice" };

const EMPTY: PluginChatState = {
  placed: new Map(),
  superseded: new Map(),
  replacing: new Set(),
  commands: [],
  names: new Map(),
};

function message(id: string, body: string, own = false): ChatMessage {
  return {
    id,
    conversation: { kind: "channel", channelId: 1 },
    senderName: "Bob",
    senderCertHash: null,
    senderSession: 7,
    own,
    sentAt: "2026-11-01T10:00:00Z",
    body,
    dismissedPreviews: [],
    syncedFrom: null,
  };
}

function cardUi(plugin: string, card: string, text: string): PluginUi {
  return {
    plugin,
    change: {
      type: "setTree",
      slot: { kind: "chatCard", id: card },
      nodes: [
        {
          component: { type: "text", text, style: "body", color: null },
          animation: null,
          children: [],
        },
      ],
    },
  };
}

const placed = (plugin: string, card: string, after: string | null): PlacedCard => ({
  plugin,
  card,
  serverId: 1,
  channelId: 1,
  after,
});

describe("plugin chat store", () => {
  it("places a new chat card once, where the user is when it appears", () => {
    const place = vi.fn((plugin: string, card: string) => placed(plugin, card, "m1"));
    let state = applyUi(EMPTY, cardUi("poll", "p1", "Lunch?"), place);
    state = applyUi(state, cardUi("poll", "p1", "Lunch? (1 vote)"), place);
    expect(place).toHaveBeenCalledTimes(1);
    expect(state.placed.get(cardKey("poll", "p1"))?.after).toBe("m1");
    const panel = { plugin: "poll", change: { type: "clear", slot: { kind: "panel" } } };
    expect(applyUi(state, panel as PluginUi, place)).toBe(state);
  });

  it("shows a card that replaces a line there, not as its own item", () => {
    const place = (plugin: string, card: string) => placed(plugin, card, "m1");
    let state = applyUi(EMPTY, cardUi("poll", "p1", "Lunch?"), place);
    expect(cardsAfter(state, message("m1", "hi"))).toHaveLength(1);
    state = applySupersede(state, "poll", "p1", "m2");
    expect(cardsAfter(state, message("m1", "hi"))).toHaveLength(0);
    expect(state.superseded.get("m2")).toEqual({ plugin: "poll", card: "p1" });
  });

  it("offers only owned commands and finds the command a line starts with", () => {
    expect(matchCommands([ROLL, TAKEN], "r")).toEqual([ROLL]);
    expect(matchCommands([ROLL], "x")).toEqual([]);
    expect(commandOf([ROLL], "/roll 2d6")).toBe(ROLL);
    expect(commandOf([ROLL], "/roll")).toBe(ROLL);
    expect(commandOf([ROLL], "/rolls")).toBeUndefined();
    expect(commandOf([ROLL], "roll 2d6")).toBeUndefined();
    expect(commandOf([TAKEN], "/roll")).toBeUndefined();
  });

  it("labels lines that start with an installed plugin's name", () => {
    const names = new Map([["poll", "Poll"]]);
    expect(viaPlugin(names, "[Poll] Lunch? 1) Pizza 2) Pasta")).toBe("Poll");
    expect(viaPlugin(names, "[Dice] rolled 4")).toBeNull();
    expect(viaPlugin(names, "Poll results")).toBeNull();
  });

  it("reads the channel of a chat list key", () => {
    expect(channelOfListKey(`3:${JSON.stringify({ kind: "channel", channelId: 9 })}`)).toEqual({
      serverId: 3,
      channelId: 9,
    });
    expect(channelOfListKey(`3:${JSON.stringify({ kind: "privateSession", session: 2 })}`)).toBe(
      null,
    );
    expect(channelOfListKey("")).toBeNull();
  });
});

describe("plugin chat UI", () => {
  let api: MockApi;

  beforeEach(async () => {
    resetPluginChatStore();
    api = await connectedMock();
    api.pluginCommands = () => Promise.resolve([ROLL]);
    api.pluginsList = () =>
      Promise.resolve({
        noPlugins: false,
        allOff: false,
        plugins: [{ id: "poll", name: "Poll" } as InstalledPlugin],
      });
  });

  afterEach(() => {
    resetPluginChatStore();
    resetAll();
  });

  function renderComposer() {
    render(
      <main>
        <Composer
          target={{ kind: "channel", channelId: 1 }}
          limits={{ text: 5000, image: 100_000, allowHtml: true }}
          label="Message #Lobby"
          blockedReason={null}
        />
      </main>,
    );
    return screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
  }

  it("autocompletes plugin commands with the plugin name", async () => {
    const input = renderComposer();
    fireEvent.change(input, { target: { value: "/r" } });
    const list = await screen.findByRole("listbox", { name: "Plugin commands" });
    expect(list.textContent).toContain("/roll");
    expect(list.textContent).toContain("Dice roller");
    expect(input.getAttribute("aria-expanded")).toBe("true");
    fireEvent.keyDown(input, { key: "Tab" });
    expect(input.value).toBe("/roll ");
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("sends a typed command to its plugin and never into the chat", async () => {
    const run = vi.fn(() => Promise.resolve("plugin" as const));
    api.pluginRunCommand = run;
    const send = vi.spyOn(api, "sendChannelMessage");
    const input = renderComposer();
    await screen.findByRole("textbox");
    fireEvent.change(input, { target: { value: "/r" } });
    await screen.findByRole("listbox", { name: "Plugin commands" });
    fireEvent.change(input, { target: { value: "/roll 2d6" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(run).toHaveBeenCalledWith("/roll 2d6");
    expect(input.value).toBe("");
    await act(() => Promise.resolve());
    expect(send).not.toHaveBeenCalled();
  });

  it("sends an unknown slash line as normal text", async () => {
    const run = vi.fn(() => Promise.resolve("plugin" as const));
    api.pluginRunCommand = run;
    const send = vi.spyOn(api, "sendChannelMessage");
    const input = renderComposer();
    fireEvent.change(input, { target: { value: "/r" } });
    await screen.findByRole("listbox", { name: "Plugin commands" });
    fireEvent.change(input, { target: { value: "/shrug" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(send).toHaveBeenCalled();
    });
    expect(run).not.toHaveBeenCalled();
  });

  it("replaces a superseded line with the card, in either order, and reveals the text", async () => {
    render(
      <main>
        <PluginLine message={message("42", "[Poll] Lunch? 1) Pizza 2) Pasta")}>
          <p>fallback line</p>
        </PluginLine>
      </main>,
    );
    expect(await screen.findByText("via Poll")).toBeTruthy();
    act(() => {
      api.mock.emit("pluginSupersede", { plugin: "poll", card: "p1", messageId: "42" });
    });
    // No card yet: the line stays.
    expect(screen.getByText("fallback line")).toBeTruthy();
    act(() => {
      api.mock.emit("pluginUi", cardUi("poll", "p1", "Lunch?"));
    });
    expect(await screen.findByRole("region", { name: "Card from Poll" })).toBeTruthy();
    expect(screen.queryByText("fallback line")).toBeNull();
    expect(screen.queryByText("via Poll")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show text" }));
    expect(screen.getByText("fallback line")).toBeTruthy();
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("reports mentions from others only", () => {
    const mentioned = vi.spyOn(api, "pluginsMentioned");
    reportMention(message("1", "hello", true), 1);
    reportMention({ ...message("2", "hello"), senderSession: null }, 1);
    expect(mentioned).not.toHaveBeenCalled();
  });
});
