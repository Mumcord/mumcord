/** Big servers (QA.27): collapsed start, remembered choices, "Jump to my channel", deep indent. */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type ChannelInfo, type MockApi, type User } from "../../api";
import { applyLanguage } from "../../i18n";
import { ChannelsPanel } from "./ChannelsPanel";
import { ChannelTree, type ChannelTreeHandle } from "./ChannelTree";
import { getServerState, type ServerState } from "./serverStore";
import { connectedMock, resetAll, stubDialog } from "./testUtils";
import { BIG_SERVER_CHANNELS, buildTree, channelPath, indentRem, initialCollapsed } from "./tree";

let base: ServerState;

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
 * Root (0) → groups 1–6 → ten sub-channels each (ids 10·group + 0…9), plus "Deep" (100) → … five
 * levels below sub-channel 30. Each sub-channel holds one person; the own user is in channel 34.
 */
function bigState(channelCount = 6): ServerState {
  const template = [...base.channels.values()][0];
  if (template === undefined) throw new Error("seeded server has no channels");
  const channel = (id: number, parentId: number | null, name: string): ChannelInfo => ({
    ...template,
    id,
    parentId,
    name,
    position: id,
  });
  const channels: ChannelInfo[] = [channel(0, null, "Root")];
  const people: User[] = [];
  for (let group = 1; group <= channelCount; group++) {
    channels.push(channel(group, 0, `Group ${String(group)}`));
    for (let i = 0; i < 10; i++) {
      const id = group * 10 + i;
      channels.push(channel(id, group, `Sub ${String(id)}`));
      people.push(user(1000 + id, `Person ${String(id)}`, id));
    }
  }
  [100, 101, 102].forEach((id, i) =>
    channels.push(channel(id, i === 0 ? 30 : id - 1, `Deep ${String(id)}`)),
  );
  people.push(user(1, "Me", 34));
  return {
    ...base,
    serverId: base.serverId,
    selfSession: 1,
    channels: new Map(channels.map((c) => [c.id, c])),
    users: new Map(people.map((u) => [u.session, u])),
    talking: new Set(),
  };
}

const noop = () => undefined;
function tree(state: ServerState, ref = createRef<ChannelTreeHandle>(), readOnly = false) {
  return (
    <div data-testid="scroller" style={{ overflowY: "auto" }}>
      <ChannelTree
        ref={ref}
        state={state}
        readOnly={readOnly}
        selectedChannelId={null}
        onSelect={noop}
        onJoin={noop}
        onOpenPerson={noop}
      />
    </div>
  );
}
const item = (name: RegExp) => screen.getByRole("treeitem", { name });
const expanded = (name: RegExp) => item(name).getAttribute("aria-expanded");

beforeEach(async () => {
  stubDialog();
  await applyLanguage("en");
  const api = createMockApi();
  setApi(api);
  getServerState();
  await api.connect(1, null);
  base = getServerState();
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  resetAll();
});

describe("tree helpers", () => {
  it("finds the path from the top level down to a channel", () => {
    const roots = buildTree(bigState());
    expect(channelPath(roots, 34)).toEqual([0, 3, 34]);
    expect(channelPath(roots, 102)).toEqual([0, 3, 30, 100, 101, 102]);
    expect(channelPath(roots, 0)).toEqual([0]);
    expect(channelPath(roots, 999)).toEqual([]);
  });

  it("starts small servers fully expanded", () => {
    const state = bigState(4); // 1 + 4 + 40 + 3 = 48 channels
    expect(state.channels.size).toBeLessThanOrEqual(BIG_SERVER_CHANNELS);
    expect(initialCollapsed(buildTree(state), state.channels.size, 34).size).toBe(0);
  });

  it("starts big servers with only the top level and the own path expanded", () => {
    const state = bigState();
    const collapsed = initialCollapsed(buildTree(state), state.channels.size, 34);
    for (const open of [0, 3, 34]) expect(collapsed.has(open)).toBe(false);
    for (const shut of [1, 2, 4, 5, 6, 30, 33, 100]) expect(collapsed.has(shut)).toBe(true);
    // Without a known own channel only the top level stays open.
    const none = initialCollapsed(buildTree(state), state.channels.size, null);
    expect(none.has(0)).toBe(false);
    expect(none.has(3)).toBe(true);
  });

  it("indents the first two nested levels fully and deeper ones by half", () => {
    expect([2, 3, 4, 5, 9].map(indentRem)).toEqual([1, 1, 0.5, 0.5, 0.5]);
  });
});

describe("ChannelTree on a big server", () => {
  it("shows the top level and the path to the own channel on first display", () => {
    render(tree(bigState()));
    expect(expanded(/^Root/)).toBe("true");
    expect(expanded(/^Group 3/)).toBe("true");
    expect(expanded(/^Group 1/)).toBe("false");
    expect(expanded(/^Sub 34/)).toBe("true");
    expect(expanded(/^Sub 33/)).toBe("false");
    expect(screen.queryByRole("treeitem", { name: /^Sub 12/ })).toBeNull();
    expect(item(/^Me/)).toBeInTheDocument();
  });

  it("keeps a small server fully expanded", () => {
    render(tree(bigState(4)));
    expect(expanded(/^Group 1/)).toBe("true");
    expect(item(/^Deep 102/)).toBeInTheDocument();
  });

  it("remembers the user's choices through store updates and a new tree", () => {
    const state = bigState();
    const view = render(tree(state));
    fireEvent.click(item(/^Group 1/).querySelector(".tree-channel__toggle") as Element);
    fireEvent.click(item(/^Group 3/).querySelector(".tree-channel__toggle") as Element);
    expect(expanded(/^Group 1/)).toBe("true");
    expect(expanded(/^Group 3/)).toBe("false");
    // Someone moves: a new server state, the choices stay.
    const users = new Map(state.users);
    users.set(1010, user(1010, "Person 10", 11));
    view.rerender(tree({ ...state, users }));
    expect(expanded(/^Group 1/)).toBe("true");
    expect(expanded(/^Group 3/)).toBe("false");
    // The tree is shown again (reconnect): still the user's choices, not the start layout.
    view.unmount();
    render(tree(bigState()));
    expect(expanded(/^Group 1/)).toBe("true");
    expect(expanded(/^Group 3/)).toBe("false");
  });
});

describe("jump to my channel", () => {
  it("expands the path, focuses, scrolls only the tree container and highlights the channel", () => {
    const ref = createRef<ChannelTreeHandle>();
    render(tree(bigState(), ref));
    const scroller = screen.getByTestId("scroller");
    const scrollTo = vi.fn();
    scroller.scrollTo = scrollTo;
    const windowScroll = vi.spyOn(window, "scrollTo").mockImplementation(noop);
    fireEvent.click(item(/^Group 3/).querySelector(".tree-channel__toggle") as Element);
    expect(screen.queryByRole("treeitem", { name: /^Sub 34/ })).toBeNull();

    vi.useFakeTimers();
    act(() => {
      ref.current?.jumpToVoice();
    });
    const own = item(/^Sub 34/);
    expect(expanded(/^Group 3/)).toBe("true");
    act(() => {
      vi.advanceTimersByTime(20); // the frame in which the row is focused and highlighted
    });
    expect(own).toHaveFocus();
    expect(own).toHaveAttribute("tabindex", "0");
    expect(scrollTo).toHaveBeenCalledWith(expect.objectContaining({ behavior: "smooth" }));
    expect(windowScroll).not.toHaveBeenCalled();
    expect(own).toHaveAttribute("data-flash");
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(own).not.toHaveAttribute("data-flash");
    vi.useRealTimers();
  });

  it("scrolls without animation when the system asks for reduced motion", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce") }));
    const ref = createRef<ChannelTreeHandle>();
    render(tree(bigState(), ref));
    const scrollTo = vi.fn();
    screen.getByTestId("scroller").scrollTo = scrollTo;
    act(() => {
      ref.current?.jumpToVoice();
    });
    await waitFor(() => {
      expect(scrollTo).toHaveBeenCalledWith(expect.objectContaining({ behavior: "auto" }));
    });
  });

  it("does nothing while reading along", () => {
    const ref = createRef<ChannelTreeHandle>();
    render(tree(bigState(), ref, true));
    fireEvent.click(item(/^Group 3/).querySelector(".tree-channel__toggle") as Element);
    act(() => {
      ref.current?.jumpToVoice();
    });
    expect(expanded(/^Group 3/)).toBe("false");
  });
});

describe("JumpToChannelButton in the channels panel", () => {
  let api: MockApi;

  it("is enabled only while joined for voice and jumps to the voice channel", async () => {
    api = await connectedMock();
    render(<ChannelsPanel />);
    const button = await screen.findByRole("button", { name: "Jump to my channel" });
    expect(button).toHaveAttribute("title", "Jump to my channel");
    expect(button).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(button);
    expect(document.activeElement).toBe(document.body);

    await act(() => api.channelJoin(1));
    await waitFor(() => {
      expect(button).toHaveAttribute("aria-disabled", "false");
    });
    fireEvent.click(button);
    await waitFor(() => {
      expect(screen.getByRole("treeitem", { name: /^Lobby/ })).toHaveFocus();
    });
  });

  it("is translated", async () => {
    api = await connectedMock();
    await applyLanguage("de");
    render(<ChannelsPanel />);
    expect(
      await screen.findByRole("button", { name: "Zu meinem Kanal springen" }),
    ).toBeInTheDocument();
    await applyLanguage("es");
    expect(await screen.findByRole("button", { name: "Ir a mi canal" })).toBeInTheDocument();
    await applyLanguage("en");
  });
});

describe("deep channel names", () => {
  it("indents deep levels less and shows the full name as a tooltip", () => {
    render(tree(bigState(4)));
    // A channel's style holds the indent of its children.
    expect(item(/^Root/).style.getPropertyValue("--tree-indent")).toBe("1rem");
    expect(item(/^Sub 30/).style.getPropertyValue("--tree-indent")).toBe("0.5rem");
    expect(item(/^Deep 100/).querySelector(".tree-channel__name")).toHaveAttribute(
      "title",
      "Deep 100",
    );
  });
});
