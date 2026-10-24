import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

  it("renders channels with people and their status", async () => {
    await renderPanel();
    const lobby = item(/^Lobby/);
    expect(lobby).toHaveAttribute("aria-expanded", "true");
    expect(lobby).toHaveAttribute("aria-selected", "true");
    expect(lobby).toHaveAttribute("aria-keyshortcuts", "Shift+Enter");
    expect(lobby).toHaveAccessibleName(/your channel, 3 people/);
    expect(item(/^Ben/)).toHaveAccessibleName(/not registered/);
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

  it("shows an avatar per person with the ring only while talking", async () => {
    await renderPanel();
    const avatarOf = (row: Element | null) => row?.querySelector(".avatar");
    expect(avatarOf(item(/^Anna/))).toHaveAttribute("data-talking", "false");
    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(avatarOf(item(/^Anna/))).toHaveAttribute("data-talking", "true");
  });

  it("lights only the speaker, never quiet people (AC-1.7)", async () => {
    await renderPanel();
    const lit = () =>
      Array.from(document.querySelectorAll(".tree-person[data-talking='true']")).map(
        (row) => row.querySelector(".tree-person__name")?.firstChild?.textContent,
      );
    expect(lit()).toEqual([]);
    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(lit()).toEqual(["Anna"]);
    act(() => {
      api.mock.setTalking(2, false);
      api.mock.setTalking(getServerState().selfSession ?? -1, true);
    });
    expect(lit()).toHaveLength(1);
    expect(lit()).not.toContain("Anna");
    act(() => {
      api.mock.setTalking(4, true);
    });
    // Carla talks: only her own row lights up.
    expect(lit().filter((n) => n === "Carla")).toHaveLength(1);
    expect(lit()).toHaveLength(2);
  });

  it("shows channel listeners nowhere in the tree (QA.11)", async () => {
    await renderPanel();
    expect(screen.queryByRole("treeitem", { name: /listening/ })).toBeNull();
    // Carla listens to Lobby and lives in Raid: exactly one row, no ear marker.
    const carla = screen.getAllByRole("treeitem", { name: /^Carla/ });
    expect(carla).toHaveLength(1);
    expect(carla[0]?.querySelector(".person-icon")).toBeNull();
    const self = getServerState().selfSession ?? -1;
    await act(async () => {
      await api.channelFollow(2);
    });
    expect(document.querySelectorAll(`.tree-person[data-row='u${String(self)}']`)).toHaveLength(1);
    expect(item(/^Lobby/)).toHaveAccessibleName(/3 people/);
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
    // The label is CSS-generated so the (focus-visible) button adds no text to the treeitem (WCAG 2.5.3).
    expect(join.dataset.label).toBe("Join");
    expect(afk.textContent).not.toContain("Join");
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

  it.each(["dark", "light"] as const)(
    "expanded channels with members keep label-in-name (%s)",
    async (theme) => {
      applyThemeToDocument(theme);
      const { container } = await renderPanel();
      const expanded = container.querySelector('[role="treeitem"][aria-expanded="true"]');
      expect(expanded).not.toBeNull();
      expect(expanded?.textContent).not.toContain("(you)");
      expect(await axeViolations(container)).toEqual([]);
    },
  );

  // jsdom axe treats visually hidden text as visible; WebKit doesn't. Check what a sighted user sees
  // (DOM text minus visually hidden spans) is a contiguous run of the name, as axe does in WebKit.
  it("keeps each treeitem's visible text inside its accessible name", async () => {
    const { container } = await renderPanel();
    const words = (text: string) => text.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [];
    const items = Array.from(container.querySelectorAll<HTMLElement>('[role="treeitem"]'));
    expect(items.length).toBeGreaterThan(0);
    for (const item of items) {
      const clone = item.cloneNode(true) as HTMLElement;
      // The avatar is decorative (aria-hidden); its initial is not visible label text.
      clone.querySelectorAll(".visually-hidden, .avatar").forEach((el) => {
        el.remove();
      });
      const visible = words(clone.textContent.replace(/\([^()]*\)/g, " ")).join(" ");
      const labelId = item.getAttribute("aria-labelledby") ?? "";
      const name = words(document.getElementById(labelId)?.textContent ?? "").join(" ");
      expect(` ${name} `).toContain(` ${visible} `);
    }
    expect(container.querySelector(".tree-channel__count")?.getAttribute("data-count")).toMatch(
      /^\d+$/,
    );
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
