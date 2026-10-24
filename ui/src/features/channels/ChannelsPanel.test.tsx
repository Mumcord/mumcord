import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { subscribeAnnouncements, type Announcement } from "../../a11y/announcer";
import type { MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { applyThemeToDocument } from "../../theme/theme";
import { AnnouncementLimiter, GLOBAL_INTERVAL_MS, TALK_INTERVAL_MS } from "./announcements";
import { ChannelsPanel } from "./ChannelsPanel";
import { getSelectedConversation } from "./selection";
import { getServerState } from "./serverStore";
import { connectedMock, resetAll, stubDialog } from "./testUtils";

let api: MockApi;

async function renderPanel(props: Parameters<typeof ChannelsPanel>[0] = {}) {
  api = await connectedMock();
  const view = render(<ChannelsPanel {...props} />);
  await screen.findByRole("tree", { name: "Channels" });
  return view;
}

const item = (name: RegExp | string) => screen.getByRole("treeitem", { name });

beforeEach(stubDialog);
afterEach(() => {
  vi.useRealTimers();
  resetAll();
});

describe("ChannelsPanel", () => {
  it("shows an empty state when not connected", async () => {
    api = await connectedMock();
    await act(() => api.disconnect());
    render(<ChannelsPanel />);
    expect(screen.getByText(/Not connected/)).toBeInTheDocument();
  });

  it("renders channels with people, listeners and their status", async () => {
    await renderPanel();
    const lobby = item(/^Lobby/);
    expect(lobby).toHaveAttribute("aria-expanded", "true");
    expect(lobby).toHaveAttribute("aria-selected", "true");
    expect(lobby).toHaveAttribute("aria-keyshortcuts", "Shift+Enter");
    expect(lobby).toHaveAccessibleName(/your channel, 3 people/);
    expect(item(/^Ben/)).toHaveAccessibleName(/not registered/);
    const carla = within(lobby).getByRole("treeitem", { name: /^Carla/ });
    expect(carla).toHaveAccessibleName(/listening/);
    expect(carla.querySelector(".person-icon")).not.toBeNull();
    expect(screen.getByText(/Shift\+Enter or double-click: join/)).toBeInTheDocument();
  });

  it("updates the talking ring and spoken status", async () => {
    await renderPanel();
    act(() => {
      api.mock.setTalking(2, true);
    });
    const anna = item(/^Anna/);
    expect(anna).toHaveAttribute("data-talking", "true");
    expect(anna).toHaveAccessibleName(/talking/);
    act(() => {
      api.mock.setTalking(2, false);
    });
    expect(item(/^Anna/)).toHaveAttribute("data-talking", "false");
  });

  it("moves with the arrow keys using a single Tab stop", async () => {
    await renderPanel();
    const tabStops = screen.getAllByRole("treeitem").filter((el) => el.tabIndex === 0);
    expect(tabStops).toHaveLength(1);
    const lobby = item(/^Lobby/);
    expect(tabStops[0]).toBe(lobby);
    lobby.focus();

    fireEvent.keyDown(lobby, { key: "ArrowDown" });
    expect(item(/^Anna/)).toHaveFocus();
    expect(item(/^Anna/).tabIndex).toBe(0);
    expect(lobby.tabIndex).toBe(-1);

    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "ArrowLeft" });
    expect(lobby).toHaveFocus();
    fireEvent.keyDown(lobby, { key: "ArrowLeft" });
    expect(lobby).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("treeitem", { name: /^Anna/ })).toBeNull();
    fireEvent.keyDown(lobby, { key: "ArrowRight" });
    expect(lobby).toHaveAttribute("aria-expanded", "true");
    fireEvent.keyDown(lobby, { key: "ArrowRight" });
    expect(item(/^Anna/)).toHaveFocus();

    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "End" });
    expect(item(/^AFK/)).toHaveFocus();
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "Home" });
    expect(item(/^Root/)).toHaveFocus();
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "ArrowUp" });
    expect(item(/^Root/)).toHaveFocus();
  });

  it("selects with Enter/Space and joins with Shift+Enter", async () => {
    await renderPanel();
    const raid = item(/^Raid/);
    raid.focus();
    fireEvent.keyDown(raid, { key: "Enter" });
    expect(getSelectedConversation()).toEqual({ kind: "channel", channelId: 2 });
    expect(getServerState().users.get(1)?.channelId).toBe(1);

    const afk = item(/^AFK/);
    fireEvent.keyDown(afk, { key: " " });
    expect(getSelectedConversation()).toEqual({ kind: "channel", channelId: 3 });

    fireEvent.keyDown(raid, { key: "Enter", shiftKey: true });
    await waitFor(() => {
      expect(getServerState().users.get(1)?.channelId).toBe(2);
    });
  });

  it("selects on click and joins on double-click or the Join button", async () => {
    await renderPanel();
    const raidRow = item(/^Raid/).querySelector(".tree-channel__row") as HTMLElement;
    fireEvent.click(raidRow);
    expect(getSelectedConversation()).toEqual({ kind: "channel", channelId: 2 });
    expect(getServerState().users.get(1)?.channelId).toBe(1);

    fireEvent.doubleClick(raidRow);
    await waitFor(() => {
      expect(getServerState().users.get(1)?.channelId).toBe(2);
    });

    const afk = item(/^AFK/);
    const join = afk.querySelector("button") as HTMLButtonElement;
    expect(join.tabIndex).toBe(-1);
    fireEvent.click(join);
    await waitFor(() => {
      expect(getServerState().users.get(1)?.channelId).toBe(3);
    });
    // The Join button doesn't change the selected chat.
    expect(getSelectedConversation()).toEqual({ kind: "channel", channelId: 2 });
  });

  it("joins exactly once on a realistic double-click sequence", async () => {
    await renderPanel();
    const spy = vi.spyOn(api, "channelJoin");
    const name = () => item(/^Raid/).querySelector(".tree-channel__name") as HTMLElement;
    for (const detail of [1, 2]) {
      const el = name();
      fireEvent.mouseDown(el, { detail });
      fireEvent.mouseUp(el, { detail });
      fireEvent.click(el, { detail });
      if (detail === 2) fireEvent.doubleClick(el, { detail });
    }
    await waitFor(() => {
      expect(getServerState().users.get(1)?.channelId).toBe(2);
    });
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("shows an error when joining is denied", async () => {
    await renderPanel();
    api.mock.failNext("channelJoin", "permissionDenied");
    fireEvent.keyDown(item(/^Raid/), { key: "Enter", shiftKey: true });
    expect(await screen.findByRole("alert")).toHaveTextContent(/not allowed/);
  });

  it("opens the profile card for a person with Enter and returns focus", async () => {
    await renderPanel();
    const anna = item(/^Anna/);
    anna.focus();
    fireEvent.keyDown(anna, { key: "Enter" });
    const dialog = await screen.findByRole("dialog", { name: "Anna" });
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(item(/^Anna/)).toHaveFocus();
  });

  it("renders the channel badge slot", async () => {
    await renderPanel({ channelBadge: (id) => (id === 2 ? <span>5 unread</span> : null) });
    expect(item(/^Raid/)).toHaveAccessibleName(/5 unread/);
  });

  it.each(["dark", "light"] as const)("has no axe violations (%s)", async (theme) => {
    applyThemeToDocument(theme);
    const { container } = await renderPanel();
    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("announcements", () => {
  it("limits to one per person per 10 s and one overall per 1.5 s", () => {
    const limiter = new AnnouncementLimiter();
    expect(limiter.allow(0, 2)).toBe(true);
    expect(limiter.allow(GLOBAL_INTERVAL_MS - 1, 3)).toBe(false);
    expect(limiter.allow(GLOBAL_INTERVAL_MS, 2)).toBe(false);
    expect(limiter.allow(GLOBAL_INTERVAL_MS, 3)).toBe(true);
    expect(limiter.allow(TALK_INTERVAL_MS - 1, 2)).toBe(false);
    expect(limiter.allow(TALK_INTERVAL_MS, 2)).toBe(true);
    expect(limiter.allow(TALK_INTERVAL_MS + GLOBAL_INTERVAL_MS)).toBe(true);
  });

  it("announces joins, leaves and talking in the own channel politely", async () => {
    const heard: Announcement[] = [];
    const stop = subscribeAnnouncements((a) => heard.push(a));
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(0);
    await renderPanel();

    act(() => {
      api.mock.setTalking(2, true);
      // Within 1.5 s: dropped.
      api.mock.setTalking(3, true);
    });
    expect(heard).toEqual([{ message: "Anna is talking", politeness: "polite" }]);

    vi.setSystemTime(2000);
    act(() => {
      api.mock.setTalking(2, false);
      api.mock.setTalking(2, true);
    });
    expect(heard).toHaveLength(1);

    vi.setSystemTime(4000);
    const carla = getServerState().users.get(4);
    if (!carla) throw new Error("seed changed");
    act(() => {
      api.mock.emit("userUpserted", { user: { ...carla, channelId: 1 } });
    });
    expect(heard.at(-1)?.message).toBe("Carla joined your channel");

    vi.setSystemTime(6000);
    act(() => {
      api.mock.emit("userRemoved", { session: 3 });
    });
    expect(heard.at(-1)?.message).toBe("Ben left your channel");

    vi.setSystemTime(8000);
    act(() => {
      // Talking elsewhere is not announced.
      api.mock.emit("userUpserted", { user: { ...carla, channelId: 2 } });
    });
    vi.setSystemTime(10_000);
    act(() => {
      api.mock.setTalking(4, true);
    });
    expect(heard.map((a) => a.message)).toEqual([
      "Anna is talking",
      "Carla joined your channel",
      "Ben left your channel",
      "Carla left your channel",
    ]);
    stop();
  });
});
