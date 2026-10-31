import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { ChannelsPanel, selectConversation } from "../channels";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { ChatView } from "../chat";
import { resetChatStore } from "../chat/chatStore";
import { resetFollowStore } from "../follow";
import { ChannelHeader, ChannelTreeMenu } from ".";

let api: MockApi;

async function renderTree() {
  resetFollowStore();
  api = await connectedMock({ bindStore: true });
  stubDialog();
  const view = render(
    <ChannelsPanel channelMenu={(menu) => <ChannelTreeMenu key={menu.channelId} {...menu} />} />,
  );
  await screen.findByRole("tree", { name: "Channels" });
  return view;
}

const row = (name: RegExp) => screen.getByRole("treeitem", { name });
function openTreeMenu(name: RegExp) {
  fireEvent.contextMenu(row(name), { clientX: 40, clientY: 50 });
  return screen.getByRole("menu");
}

/** Plain item labels (without the hint at the end of a row). */
function itemLabels(menu: HTMLElement): string[] {
  return within(menu)
    .getAllByRole("menuitem")
    .map((item) => {
      const hint = item.querySelector(".server-menu__end")?.textContent ?? "";
      const text = item.textContent;
      return hint === "" ? text : text.slice(0, text.length - hint.length);
    });
}

beforeEach(() => {
  resetFollowStore();
});

afterEach(() => {
  resetAll();
});

describe("shared channel actions (issue #172)", () => {
  it("gives the header and the tree the same list; the tree adds Join and Move", async () => {
    await renderTree();
    act(() => {
      api.mock.updateChannel(2, { permissions: { ...permissionsOf(2), moveUsers: true } });
    });
    const tree = itemLabels(openTreeMenu(/^Raid/));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    cleanup();
    resetAll();

    resetFollowStore();
    api = await connectedMock({ bindStore: true });
    resetChatStore();
    act(() => {
      api.mock.updateChannel(2, { permissions: { ...permissionsOf(2), moveUsers: true } });
    });
    render(<ChatView renderChannelTitle={(id) => <ChannelHeader channelId={id} />} />);
    act(() => {
      selectConversation({ kind: "channel", channelId: 2 });
    });
    const heading = await screen.findByRole("heading", { level: 2 });
    fireEvent.click(within(heading).getByRole("button"));
    const header = itemLabels(screen.getByRole("menu"));

    expect(header).toEqual([
      "Follow channel",
      "Audio for this channel…",
      "Copy channel link",
      "Channel info",
      "Clear chat history…",
    ]);
    expect(tree).toEqual([
      "Join channel",
      ...header.slice(0, -1),
      "Move users here…",
      "Clear chat history…",
    ]);
  });

  it("hides items the user may not use instead of disabling them", async () => {
    await renderTree();
    const menu = openTreeMenu(/^Raid/);
    const shown = itemLabels(menu);
    expect(shown).not.toContain("Move users here…");
    expect(shown).not.toContain("Channel password…");
    expect(within(menu).queryAllByRole("menuitem", { hidden: true })).toHaveLength(shown.length);
    fireEvent.keyDown(menu, { key: "Escape" });

    // The own voice channel has no "Join channel"; a locked one offers its password.
    await act(() => api.channelJoin(1, 1));
    expect(itemLabels(openTreeMenu(/^Lobby/))).not.toContain("Join channel");
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    act(() => {
      api.mock.lockChannel(2);
    });
    expect(itemLabels(openTreeMenu(/^Raid/))).toContain("Channel password…");
  });

  it("shows Shift+Enter as the hint of Join channel and joins from the menu", async () => {
    await renderTree();
    const join = vi.spyOn(api, "channelJoin");
    const item = within(openTreeMenu(/^Raid/)).getByRole("menuitem", { name: /^Join channel/ });
    expect(item).toHaveTextContent("Shift+Enter");
    fireEvent.click(item);
    expect(join).toHaveBeenCalledWith(1, 2);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("keeps the browser menu away and passes the axe check with the menu open", async () => {
    const view = await renderTree();
    const event = createEvent.contextMenu(row(/^Raid/));
    fireEvent(row(/^Raid/), event);
    expect(event.defaultPrevented).toBe(true);
    expect(await axeViolations(view.container)).toEqual([]);
    expect(await axeViolations(screen.getByRole("menu"))).toEqual([]);
  });
});

describe("tree menu keyboard (issue #172)", () => {
  it("opens with the menu key and Shift+F10 and returns focus to the row on Escape", async () => {
    await renderTree();
    const raid = row(/^Raid/);
    raid.focus();
    fireEvent.keyDown(raid, { key: "ContextMenu" });
    const menu = screen.getByRole("menu", { name: "Channel options for Raid" });
    expect(within(menu).getAllByRole("menuitem")[0]).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    await waitFor(() => {
      expect(row(/^Raid/)).toHaveFocus();
    });

    fireEvent.keyDown(row(/^Raid/), { key: "F10", shiftKey: true });
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });

  it("joins with Shift+Enter and shows the chat with Enter", async () => {
    await renderTree();
    const join = vi.spyOn(api, "channelJoin");
    fireEvent.keyDown(row(/^Raid/), { key: "Enter", shiftKey: true });
    expect(join).toHaveBeenCalledWith(1, 2);
  });
});

describe("user limit and status icons (FR-3.85, FR-3.86)", () => {
  it("shows (n/max) only with a limit and marks a full channel", async () => {
    await renderTree();
    const limit = () => row(/^Raid/).querySelector(".tree-channel__limit");
    expect(limit()).toBeNull();

    act(() => {
      api.mock.updateChannel(2, { maxUsers: 10 });
    });
    expect(limit()).toHaveAttribute("data-limit", "(1/10)");
    expect(row(/^Raid/)).toHaveAccessibleName(/1 of 10 places taken/);
    expect(row(/^Raid/)).not.toHaveAttribute("data-full");

    act(() => {
      api.mock.updateChannel(2, { maxUsers: 1 });
    });
    expect(limit()).toHaveAttribute("data-limit", "(1/1)");
    expect(limit()).toHaveAttribute("data-full", "true");
    expect(row(/^Raid/)).toHaveAttribute("data-full");
    expect(row(/^Raid/)).toHaveAttribute("data-tooltip", "Channel full");
    expect(row(/^Raid/)).toHaveAccessibleName(/Channel full, 1 of 1 place taken/);
  });

  it("announces temporary, linked and the own channel", async () => {
    await renderTree();
    await act(() => api.channelJoin(1, 1));
    act(() => {
      api.mock.updateChannel(2, { temporary: true, links: [1] });
    });
    expect(row(/^Raid/)).toHaveAccessibleName(/temporary, linked/);
    expect(row(/^Lobby/)).toHaveAccessibleName(/you are here/);
    expect(row(/^Lobby/)).toHaveTextContent("you are here");
  });
});

describe("moving people (issue #172)", () => {
  it("offers dragging only with the move permission and moves on drop", async () => {
    await renderTree();
    const carla = () => screen.getByRole("treeitem", { name: /^Carla/ });
    expect(carla()).not.toHaveAttribute("draggable");

    act(() => {
      for (const id of [1, 2]) {
        api.mock.updateChannel(id, { permissions: { ...permissionsOf(id), moveUsers: true } });
      }
    });
    expect(carla()).toHaveAttribute("draggable", "true");
    const move = vi.spyOn(api, "personMove");
    const data = new Map<string, string>();
    const dataTransfer = {
      get types() {
        return [...data.keys()];
      },
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? "",
      effectAllowed: "",
      dropEffect: "",
    };
    fireEvent.dragStart(carla(), { dataTransfer });
    const lobby = row(/^Lobby/);
    const over = createEvent.dragOver(lobby, { dataTransfer });
    fireEvent(lobby, over);
    expect(over.defaultPrevented).toBe(true);
    fireEvent.drop(lobby, { dataTransfer });
    expect(move).toHaveBeenCalledWith(1, 4, 1);
    await waitFor(() => {
      expect(row(/^Lobby/)).toHaveAccessibleName(/4 people/);
    });
  });

  it("moves the chosen people from Move users here…", async () => {
    await renderTree();
    act(() => {
      for (const id of [1, 2]) {
        api.mock.updateChannel(id, { permissions: { ...permissionsOf(id), moveUsers: true } });
      }
    });
    const move = vi.spyOn(api, "personMove");
    fireEvent.click(
      within(openTreeMenu(/^Raid/)).getByRole("menuitem", { name: "Move users here…" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Move people to Raid" });
    // Not oneself, and not Carla, who is already in Raid.
    const people = within(dialog).getAllByRole("checkbox");
    expect(people.map((box) => box.closest("label")?.textContent)).toEqual([
      "AnnaLobby",
      "BenLobby",
    ]);
    fireEvent.click(within(dialog).getByRole("checkbox", { name: /Anna/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Move" }));
    await waitFor(() => {
      expect(move).toHaveBeenCalledWith(1, 2, 2);
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });
});

function permissionsOf(channelId: number) {
  const permissions = {
    enter: true,
    textMessage: true,
    listen: true,
    registerSelf: false,
    speak: true,
    whisper: true,
    write: false,
    moveUsers: false,
  };
  return channelId === 0 ? { ...permissions, registerSelf: true } : permissions;
}
