/** Channel tree search (QA.27): matching, filtering, expand state, field interactions. */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { ChannelInfo, User } from "../../api";
import { subscribeAnnouncements } from "../../a11y/announcer";
import { ChannelsPanel } from "./ChannelsPanel";
import { ChannelTree } from "./ChannelTree";
import { resetCollapseMemory } from "./collapseMemory";
import { buildSearchIndex, foldName, foldQuery, matchRange, searchTree } from "./search";
import { getSelectedConversation } from "./selection";
import { getServerState, type ServerState } from "./serverStore";
import { connectedMock, resetAll, stubDialog } from "./testUtils";
import { buildTree, channelRowId, memberRowId, visibleRows, visibleStacks } from "./tree";

let base: ServerState;
let announced: string[] = [];
let unsubscribe: () => void = () => undefined;

function user(session: number, name: string, channelId: number): User {
  return {
    session,
    channelId,
    name,
    certHash: null,
    registered: false,
    selfMute: false,
    selfDeaf: false,
    serverMute: false,
    serverDeaf: false,
    suppressed: false,
    listeningChannelIds: [],
    hasAvatar: false,
    avatarHash: null,
    hasComment: false,
    volume: 100,
    localMute: false,
  };
}

/**
 * Root (0) → "Café Lounge" (1) → "Back Room" (2); Root → "Gaming" (3) → "Squad Ä" (4).
 * "Squad Ä" is crowded: eight people, one of them "José".
 */
function searchState(): ServerState {
  const template = [...base.channels.values()][0];
  if (template === undefined) throw new Error("seeded server has no channels");
  const channel = (id: number, parentId: number | null, name: string): ChannelInfo => ({
    ...template,
    id,
    parentId,
    name,
    position: id,
  });
  const channels = [
    channel(0, null, "Root"),
    channel(1, 0, "Café Lounge"),
    channel(2, 1, "Back Room"),
    channel(3, 0, "Gaming"),
    channel(4, 3, "Squad Ä"),
  ];
  const people = [
    user(1, "Me", 1),
    user(2, "Mia", 2),
    user(10, "José", 4),
    ...[11, 12, 13, 14, 15, 16, 17].map((s) => user(s, `Player ${String(s)}`, 4)),
  ];
  return {
    ...base,
    selfSession: 1,
    channels: new Map(channels.map((c) => [c.id, c])),
    users: new Map(people.map((u) => [u.session, u])),
    talking: new Set(),
  };
}

function search(query: string) {
  const roots = buildTree(searchState());
  return searchTree(roots, buildSearchIndex(roots), foldQuery(query));
}

beforeEach(async () => {
  stubDialog();
  await connectedMock();
  await waitFor(() => {
    expect(getServerState().channels.size).toBeGreaterThan(0);
  });
  base = getServerState();
  announced = [];
  unsubscribe = subscribeAnnouncements((a) => announced.push(a.message));
});
afterEach(() => {
  unsubscribe();
  resetCollapseMemory();
  resetAll();
});

describe("matching", () => {
  it("ignores case and diacritics and maps the match back to the original name", () => {
    expect(foldQuery("  JOSÉ ")).toBe("jose");
    const name = foldName("Squad Ä café");
    expect(name.text).toBe("squad a cafe");
    expect(matchRange(name, "cafe")).toEqual({ start: 8, end: 12 });
    // A decomposed "é" (e + combining accent) is two characters in the original name.
    expect(matchRange(foldName("Café X"), "cafe")).toEqual({ start: 0, end: 5 });
    expect(matchRange(name, "zzz")).toBeNull();
    expect(matchRange(name, "")).toBeNull();
  });
});

describe("searchTree", () => {
  it("keeps matching channels and the path to them", () => {
    const result = search("back");
    expect(result.channels).toBe(1);
    expect(result.people).toBe(0);
    expect(result.matches).toEqual([channelRowId(2)]);
    const rows = visibleRows(result.roots, new Set());
    // The matching channel lists its people as usual; "Me" on the path to it is not listed.
    expect(rows.map((r) => r.id)).toEqual([
      channelRowId(0),
      channelRowId(1),
      channelRowId(2),
      memberRowId(2),
    ]);
  });

  it("finds people in nested crowded channels and lists only them", () => {
    const result = search("jose");
    expect(result.people).toBe(1);
    expect(result.matches).toEqual([memberRowId(10)]);
    const rows = visibleRows(result.roots, new Set());
    expect(rows.map((r) => r.id)).toEqual([
      channelRowId(0),
      channelRowId(3),
      channelRowId(4),
      memberRowId(10),
    ]);
    // Listed by name instead of the avatar stack, though the channel has eight people.
    expect(visibleStacks(result.roots, new Set(), new Set(), new Set()).size).toBe(0);
  });

  it("counts channels and people and orders the matches as displayed", () => {
    const result = search("m");
    // Channels: "Back Room", "Gaming"; people: "Me", "Mia".
    expect([result.channels, result.people]).toEqual([2, 2]);
    expect(result.matches).toEqual([
      memberRowId(1),
      channelRowId(2),
      memberRowId(2),
      channelRowId(3),
    ]);
  });

  it("returns an empty tree when nothing matches", () => {
    const result = search("nothing");
    expect(result.roots).toEqual([]);
    expect(result.matches).toEqual([]);
  });

  it("keeps unchanged nodes when searching again", () => {
    const roots = buildTree(searchState());
    const index = buildSearchIndex(roots);
    const first = searchTree(roots, index, "jose");
    const again = searchTree(roots, index, "jose", first);
    expect(again.roots[0]).toBe(first.roots[0]);
  });
});

const noop = () => undefined;

describe("ChannelTree with a filter", () => {
  function renderTree(filter: string) {
    const state = searchState();
    const props = {
      state,
      selectedChannelId: null,
      onSelect: noop,
      onJoin: noop,
      onOpenPerson: noop,
    };
    const view = render(<ChannelTree {...props} filter={filter} />);
    return {
      rerender: (next: string) => {
        view.rerender(<ChannelTree {...props} filter={next} />);
      },
    };
  }
  const item = (name: RegExp) => screen.getByRole("treeitem", { name });

  it("opens the path to matches and restores the expand state when cleared", () => {
    const view = renderTree("");
    fireEvent.click(item(/^Gaming/).querySelector(".tree-channel__toggle") as HTMLElement);
    expect(item(/^Gaming/)).toHaveAttribute("aria-expanded", "false");

    view.rerender("josé");
    expect(item(/^Gaming/)).toHaveAttribute("aria-expanded", "true");
    expect(item(/^José/)).toBeInTheDocument();
    expect(screen.queryByRole("treeitem", { name: /^Café/ })).toBeNull();
    // Collapsing a result does not touch the remembered state either.
    fireEvent.click(item(/^Squad/).querySelector(".tree-channel__toggle") as HTMLElement);

    view.rerender("");
    expect(item(/^Gaming/)).toHaveAttribute("aria-expanded", "false");
    expect(item(/^Café/)).toHaveAttribute("aria-expanded", "true");
  });

  it("highlights the matched text", () => {
    renderTree("lou");
    const mark = screen.getByText("Lou", { selector: "mark" });
    expect(mark.parentElement).toHaveTextContent("Café Lounge");
  });

  it("shows an empty state and announces the counts", () => {
    const view = renderTree("m");
    expect(announced).toEqual(["2 channels, 2 people"]);
    view.rerender("xyz");
    expect(screen.getByText(/No channels or people match “xyz”/)).toBeInTheDocument();
    expect(announced.at(-1)).toBe("0 channels, 0 people");
    view.rerender("");
    expect(announced).toHaveLength(2);
  });
});

describe("search field", () => {
  async function renderPanel() {
    render(<ChannelsPanel />);
    await screen.findByRole("tree", { name: "Channels" });
    return screen.getByRole("searchbox", { name: "Search channels and people" });
  }

  it("filters after a short pause and clears with Escape", async () => {
    const input = await renderPanel();
    expect(screen.getByRole("search")).toContainElement(input);
    expect(input).toHaveAttribute("placeholder", "Search channels and people");
    fireEvent.change(input, { target: { value: "ann" } });
    await waitFor(() => {
      expect(screen.queryByRole("treeitem", { name: /^Raid/ })).toBeNull();
    });
    expect(screen.getByRole("treeitem", { name: /^Anna/ })).toBeInTheDocument();
    expect(announced.at(-1)).toBe("0 channels, 1 person");

    fireEvent.keyDown(input, { key: "Escape" });
    expect(input).toHaveValue("");
    expect(screen.getByRole("treeitem", { name: /^Raid/ })).toBeInTheDocument();
  });

  it("clears with the × button", async () => {
    const input = await renderPanel();
    fireEvent.change(input, { target: { value: "raid" } });
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(input).toHaveValue("");
    expect(input).toHaveFocus();
    expect(screen.queryByRole("button", { name: "Clear search" })).toBeNull();
  });

  it("selects the first matching channel on Enter", async () => {
    const input = await renderPanel();
    fireEvent.change(input, { target: { value: "raid" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const raid = [...getServerState().channels.values()].find((c) => c.name === "Raid");
    await waitFor(() => {
      expect(getSelectedConversation()).toEqual({ kind: "channel", channelId: raid?.id });
    });
  });

  it("focuses the first matching person on Enter", async () => {
    const input = await renderPanel();
    fireEvent.change(input, { target: { value: "anna" } });
    await waitFor(() => {
      expect(screen.queryByRole("treeitem", { name: /^Raid/ })).toBeNull();
    });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByRole("treeitem", { name: /^Anna/ })).toHaveFocus();
  });

  it("is focused with Ctrl+K and Ctrl+F", async () => {
    const input = await renderPanel();
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(input).toHaveFocus();
    act(() => {
      input.blur();
    });
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    expect(input).toHaveFocus();
  });
});
