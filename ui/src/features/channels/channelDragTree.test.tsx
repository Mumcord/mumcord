import { act, createEvent, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { getToasts, resetNotifications } from "../notifications/notificationStore";
import { draggedChannel } from "./channelDrag";
import { ChannelsPanel } from "./ChannelsPanel";
import { setMoveArrivalGrace } from "./movePeople";
import { getServerState } from "./serverStore";
import { connectedMock, resetAll, stubDialog } from "./testUtils";

let api: MockApi;

async function renderTree() {
  api = await connectedMock({ bindStore: true });
  render(<ChannelsPanel />);
  await screen.findByRole("tree", { name: "Channels" });
}

/** Changes rights in these channels, as the server would grant them. */
function grant(rights: { write?: boolean; moveUsers?: boolean }, ...channelIds: number[]) {
  act(() => {
    for (const id of channelIds) {
      const channel = getServerState().channels.get(id);
      if (!channel) throw new Error("seed channel missing");
      api.mock.updateChannel(id, { permissions: { ...channel.permissions, ...rights } });
    }
  });
}

const treeRow = (name: RegExp) => screen.getByRole("treeitem", { name });

/** Enough of a drag to reach the rows' own handlers. */
function dragData() {
  const data = new Map<string, string>();
  return {
    get types() {
      return [...data.keys()];
    },
    setData: (type: string, value: string) => data.set(type, value),
    getData: (type: string) => data.get(type) ?? "",
    setDragImage: () => undefined,
    effectAllowed: "",
    dropEffect: "",
  };
}

/** Each row is 40 px high from y = 0, so the pointer's `clientY` picks the drop zone. */
const ROW = { top: 0, height: 40 };

/** jsdom has no DragEvent, so the pointer position is set on the plain event. */
function at(event: Event, clientY: number): Event {
  Object.defineProperty(event, "clientY", { value: clientY });
  return event;
}

/** Drags the channel row `from` over `to` at `clientY`; `drop()` lets go there. */
function dragOver(from: RegExp, to: RegExp, clientY: number) {
  const dataTransfer = dragData();
  fireEvent.dragStart(treeRow(from), { dataTransfer });
  const target = treeRow(to);
  const over = at(createEvent.dragOver(target, { dataTransfer }), clientY);
  fireEvent(target, over);
  const drop = () => fireEvent(target, at(createEvent.drop(target, { dataTransfer }), clientY));
  return { target, over, drop };
}

const rowOf = (item: HTMLElement) => item.querySelector(".tree-channel__row");
const channelOf = (id: number) => getServerState().channels.get(id);

beforeEach(() => {
  stubDialog();
  setMoveArrivalGrace(0);
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    ...ROW,
    bottom: ROW.height,
    left: 0,
    right: 200,
    width: 200,
    x: 0,
    y: 0,
    toJSON: () => ROW,
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  act(() => {
    draggedChannel.set(null);
    resetNotifications();
  });
  setMoveArrivalGrace(5000);
  resetAll();
});

// Seed: Root holds Lobby (1, position 0), Raid (2, position 1) and AFK (3, position 2).
describe("dragging channels (issue #676)", () => {
  it("lets only channels with Write be dragged, never the root", async () => {
    await renderTree();
    expect(treeRow(/^AFK/)).not.toHaveAttribute("draggable");
    grant({ write: true }, 0, 3);
    expect(treeRow(/^AFK/)).toHaveAttribute("draggable", "true");
    expect(treeRow(/^Root/)).not.toHaveAttribute("draggable");
  });

  it("reorders among siblings without asking", async () => {
    await renderTree();
    grant({ write: true }, 3);
    const edit = vi.spyOn(api, "channelEdit");
    const { target, over, drop } = dragOver(/^AFK/, /^Raid/, 2);
    expect(over.defaultPrevented).toBe(true);
    expect(rowOf(target)).toHaveAttribute("data-drop-line", "above");
    expect(target).toHaveTextContent("Place above Raid");
    drop();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(edit).toHaveBeenCalledWith(expect.any(Number), 3, {
      name: null,
      description: null,
      parentId: null,
      position: 1,
      maxUsers: null,
      linksAdd: [],
      linksRemove: [],
    });
    await waitFor(() => {
      expect(channelOf(3)?.position).toBe(1);
    });
  });

  it("asks before a new parent; Esc leaves everything as it was", async () => {
    await renderTree();
    grant({ write: true }, 3);
    const edit = vi.spyOn(api, "channelEdit");
    const { target, drop } = dragOver(/^AFK/, /^Raid/, 20);
    expect(rowOf(target)).toHaveAttribute("data-drop", "true");
    expect(target).toHaveTextContent("Move into Raid");
    drop();

    const dialog = screen.getByRole("dialog", { name: "Move “AFK”?" });
    expect(dialog).toHaveTextContent("Only AFK moves; it is empty.");
    expect(dialog).toHaveTextContent("AFK → to Raid › AFK");
    // Enter confirms: focus starts on Move.
    expect(screen.getByRole("button", { name: "Move" })).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent(dialog, new Event("cancel"));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(edit).not.toHaveBeenCalled();
    expect(channelOf(3)?.parentId).toBe(0);
  });

  it("moves the channel under the new parent once confirmed", async () => {
    await renderTree();
    grant({ write: true }, 2);
    const { drop } = dragOver(/^Raid/, /^AFK/, 20);
    drop();
    const dialog = screen.getByRole("dialog", { name: "Move “Raid”?" });
    // Carla sits in Raid.
    expect(dialog).toHaveTextContent("Raid and the 1 person in it move along.");
    fireEvent.click(screen.getByRole("button", { name: "Move" }));
    await waitFor(() => {
      expect(channelOf(2)?.parentId).toBe(3);
    });
  });

  it("refuses a drop into the channel's own sub-channel", async () => {
    await renderTree();
    grant({ write: true }, 2);
    act(() => {
      api.mock.updateChannel(3, { parentId: 2 });
    });
    const edit = vi.spyOn(api, "channelEdit");
    const { target, over, drop } = dragOver(/^Raid/, /^AFK/, 20);
    expect(over.defaultPrevented).toBe(false);
    expect(rowOf(target)).toHaveAttribute("data-drop", "refused");
    expect(target).toHaveTextContent("Can’t go into its own sub-channel");
    drop();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(edit).not.toHaveBeenCalled();
  });

  it("shows a toast when the server refuses and leaves the tree as it was", async () => {
    await renderTree();
    grant({ write: true }, 3);
    vi.spyOn(api, "channelEdit").mockResolvedValue({
      kind: "refused",
      denial: "permission",
      permission: "write",
      reason: null,
    });
    const { drop } = dragOver(/^AFK/, /^Raid/, 20);
    drop();
    fireEvent.click(screen.getByRole("button", { name: "Move" }));
    await waitFor(() => {
      expect(getToasts().map((toast) => [toast.title, toast.text, toast.tone])).toEqual([
        ["Couldn’t move AFK", "You don’t have permission to move channels into Raid.", "error"],
      ]);
    });
    expect(channelOf(3)?.parentId).toBe(0);
  });

  it("still moves dragged people into a channel", async () => {
    await renderTree();
    grant({ moveUsers: true, write: true }, 1, 2);
    const move = vi.spyOn(api, "personMove");
    const dataTransfer = dragData();
    fireEvent.dragStart(treeRow(/^Anna/), { dataTransfer });
    const raid = treeRow(/^Raid/);
    const over = at(createEvent.dragOver(raid, { dataTransfer }), 2);
    fireEvent(raid, over);
    expect(over.defaultPrevented).toBe(true);
    expect(rowOf(raid)).toHaveAttribute("data-drop", "true");
    expect(raid).not.toHaveTextContent("Place above");
    fireEvent.drop(raid, { dataTransfer });
    expect(move.mock.calls.map((call) => call.slice(1))).toEqual([[2, 2]]);
  });
});
