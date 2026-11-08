import {
  act,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { ChannelTreeMenu } from "../channelheader";
import { PersonMenuHost } from "../members/PersonMenu";
import { closePersonMenu } from "../members/personMenuStore";
import { getToasts, resetNotifications } from "../notifications/notificationStore";
import { ChannelsPanel } from "./ChannelsPanel";
import { MovePeopleDialogHost } from "./MovePeopleDialog";
import { closeMovePeople, setMoveArrivalGrace } from "./movePeople";
import { clearPeopleSelection, getPeopleSelection, resetPeopleSelection } from "./peopleSelection";
import { getServerState } from "./serverStore";
import { connectedMock, resetAll, stubDialog } from "./testUtils";

let api: MockApi;

/** The tree with the channel menu, the person menu and the picker, as the window shows them. */
async function renderWindow() {
  api = await connectedMock({ bindStore: true });
  render(
    <>
      <ChannelsPanel channelMenu={(menu) => <ChannelTreeMenu key={menu.channelId} {...menu} />} />
      <PersonMenuHost />
      <MovePeopleDialogHost />
    </>,
  );
  await screen.findByRole("tree", { name: "Channels" });
}

/** The move right in these channels, as the server would grant it. */
function grantMove(...channelIds: number[]) {
  act(() => {
    for (const id of channelIds) {
      const channel = getServerState().channels.get(id);
      if (!channel) throw new Error("seed channel missing");
      api.mock.updateChannel(id, {
        permissions: { ...channel.permissions, moveUsers: true },
      });
    }
  });
}

const treeRow = (name: RegExp) => screen.getByRole("treeitem", { name });

function openPersonMenu(name: RegExp) {
  fireEvent.contextMenu(treeRow(name), { clientX: 10, clientY: 10 });
  return screen.getByRole("menu");
}

function openChannelMenu(name: RegExp) {
  fireEvent.contextMenu(treeRow(name), { clientX: 10, clientY: 10 });
  return screen.getByRole("menu");
}

/** Opens the Admin ▸ sub-menu of the open menu (FR-10.6, FR-10.8) and returns it. */
async function openAdmin(menu: HTMLElement): Promise<HTMLElement> {
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Admin" }));
  return waitFor(() => {
    const menus = screen.getAllByRole("menu");
    const submenu = menus.at(-1);
    if (submenu === undefined || submenu === menu) throw new Error("no sub-menu yet");
    return submenu;
  });
}

/** The labels of the open menu's Admin ▸ items; empty when there is no Admin entry at all. */
async function adminLabels(menu: HTMLElement): Promise<string[]> {
  const admin = within(menu).queryByRole("menuitem", { name: "Admin" });
  if (admin === null) return [];
  const submenu = await openAdmin(menu);
  return within(submenu)
    .getAllByRole("menuitem")
    .map((item) => item.textContent);
}

/** Enough of a drag to reach the row's own handlers. */
function dragData() {
  const data = new Map<string, string>();
  return {
    get types() {
      return [...data.keys()];
    },
    setData: (type: string, value: string) => data.set(type, value),
    getData: (type: string) => data.get(type) ?? "",
    effectAllowed: "",
    dropEffect: "",
  };
}

const channelOf = (session: number) => getServerState().users.get(session)?.channelId;

beforeEach(() => {
  stubDialog();
  // The result notice only arrives after the arrival grace period; tests do not sit it out.
  setMoveArrivalGrace(0);
});
afterEach(() => {
  act(() => {
    closePersonMenu();
    closeMovePeople();
    resetPeopleSelection();
    resetNotifications();
  });
  setMoveArrivalGrace(5000);
  resetAll();
});

describe("moving people (Epic 4, issue #255)", () => {
  it("moves a person to the own channel from their menu", async () => {
    await renderWindow();
    // Carla sits in Raid (2); the own channel is Lobby (1).
    expect(treeRow(/^Carla/).getAttribute("draggable")).toBeNull();
    expect(await adminLabels(openPersonMenu(/^Carla/))).toEqual([]);
    act(() => {
      closePersonMenu();
    });
    grantMove(1, 2);

    const move = vi.spyOn(api, "personMove");
    const admin = await openAdmin(openPersonMenu(/^Carla/));
    expect(
      within(admin)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Move to my channel", "Move to"]);
    fireEvent.click(within(admin).getByRole("menuitem", { name: "Move to my channel" }));
    expect(move).toHaveBeenCalledWith(1, 4, 1);
    await waitFor(() => {
      expect(channelOf(4)).toBe(1);
    });
  });

  it("never offers the move items for people in the own channel or for yourself", async () => {
    await renderWindow();
    grantMove(1);
    expect(await adminLabels(openPersonMenu(/^Anna/))).toEqual(["Move to"]);
    act(() => {
      closePersonMenu();
    });
    const state = getServerState();
    const ownName = state.users.get(state.selfSession ?? -1)?.name ?? "";
    expect(await adminLabels(openPersonMenu(new RegExp(`^${ownName}`)))).toEqual([]);
  });

  it("offers channels with their user limit and confirms in words", async () => {
    await renderWindow();
    grantMove(1, 2);
    act(() => {
      api.mock.limitChannel(2, 1);
    });
    const admin = await openAdmin(openPersonMenu(/^Anna/));
    fireEvent.click(within(admin).getByRole("menuitem", { name: "Move to" }));
    const dialog = await screen.findByRole("dialog", { name: "Move people" });

    // Anna sits in Lobby, so Lobby is no target; Raid shows that it is full.
    expect(within(dialog).getByText("From Lobby")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("combobox", { name: "Channel" }));
    const raid = within(dialog).getByRole("option", { name: "Raid (1/1)" });
    fireEvent.click(raid);
    expect(within(dialog).getByText("Move 1 person from Lobby to Raid?")).toBeInTheDocument();

    const move = vi.spyOn(api, "personMove");
    fireEvent.click(within(dialog).getByRole("button", { name: "Move" }));
    expect(move).toHaveBeenCalledWith(1, 2, 2);
    await waitFor(() => {
      expect(channelOf(2)).toBe(2);
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });

  it("moves everyone in a channel at once from the channel menu's Admin ▸", async () => {
    await renderWindow();
    grantMove(1, 2);
    act(() => {
      api.mock.limitChannel(2, 3);
    });
    const admin = await openAdmin(openChannelMenu(/^Lobby/));
    fireEvent.click(within(admin).getByRole("menuitem", { name: "Move everyone to" }));
    const dialog = await screen.findByRole("dialog", { name: "Move people" });
    // The own user stays: joining is how one moves oneself (Q5.2).
    expect(within(dialog).getByText("From Lobby")).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("combobox", { name: "Channel" }));
    fireEvent.click(within(dialog).getByRole("option", { name: "Raid (1/3)" }));
    expect(within(dialog).getByText("Move 2 people from Lobby to Raid?")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Move" }));

    await waitFor(() => {
      expect(channelOf(2)).toBe(2);
      expect(channelOf(3)).toBe(2);
    });
    expect(channelOf(1)).toBe(1);
    // One result for the whole group, not one per person (FR-10.17).
    await waitFor(() => {
      expect(getToasts().map((toast) => toast.text)).toEqual(["Moved 2 people to Raid."]);
    });
  });

  it("counts who did not arrive and says so once", async () => {
    await renderWindow();
    grantMove(1, 2);
    // Raid takes nobody: the server refuses every move of the group.
    const raid = getServerState().channels.get(2);
    if (!raid) throw new Error("seed channel missing");
    act(() => {
      api.mock.updateChannel(2, {
        permissions: { ...raid.permissions, moveUsers: false },
      });
    });
    const admin = await openAdmin(openChannelMenu(/^Lobby/));
    fireEvent.click(within(admin).getByRole("menuitem", { name: "Move everyone to" }));
    const dialog = await screen.findByRole("dialog", { name: "Move people" });
    fireEvent.click(within(dialog).getByRole("combobox", { name: "Channel" }));
    fireEvent.click(within(dialog).getByRole("option", { name: /^Raid/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Move" }));

    await waitFor(() => {
      expect(getToasts().map((toast) => toast.text)).toEqual([
        "Moved 0 of 2 people to Raid. The rest were refused.",
      ]);
    });
  });

  it("takes the people picked with Ctrl-click along when one of them is dragged", async () => {
    await renderWindow();
    grantMove(1, 2);
    const anna = treeRow(/^Anna/);
    const ben = treeRow(/^Ben/);
    fireEvent.click(anna, { ctrlKey: true });
    fireEvent.click(ben, { ctrlKey: true });
    expect(getPeopleSelection()).toEqual(new Set([2, 3]));
    expect(anna).toHaveAttribute("aria-selected", "true");

    const move = vi.spyOn(api, "personMove");
    const dataTransfer = dragData();
    fireEvent.dragStart(anna, { dataTransfer });
    const raid = treeRow(/^Raid/);
    fireEvent(raid, createEvent.dragOver(raid, { dataTransfer }));
    fireEvent.drop(raid, { dataTransfer });
    expect(move.mock.calls.map((call) => call[1])).toEqual([2, 3]);
    await waitFor(() => {
      expect(getPeopleSelection().size).toBe(0);
    });
  });

  it("clears the picked people when a channel is clicked", async () => {
    await renderWindow();
    grantMove(1);
    fireEvent.click(treeRow(/^Anna/), { ctrlKey: true });
    expect(getPeopleSelection().size).toBe(1);
    fireEvent.click(treeRow(/^Raid/));
    expect(getPeopleSelection().size).toBe(0);
    act(() => {
      clearPeopleSelection();
    });
  });
});
