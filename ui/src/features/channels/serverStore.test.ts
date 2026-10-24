import { renderHook, act, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { getSelectedConversation, useSelectedConversation } from "./selection";
import type { ChannelInfo, User } from "../../api";
import {
  applyEvents,
  getServerState,
  setUpdateScheduler,
  subscribeServerChanges,
  useServerState,
  type ServerChange,
  type ServerState,
} from "./serverStore";
import { connectedMock, resetAll } from "./testUtils";
import { buildTree, visibleRows } from "./tree";

afterEach(() => {
  setUpdateScheduler(null);
  resetAll();
});

const channel = (id: number): ChannelInfo => ({
  id,
  parentId: id === 0 ? null : 0,
  name: `C${String(id)}`,
  position: 0,
  permissions: { enter: true, textMessage: true, listen: true, registerSelf: false },
  description: null,
  hasDescription: false,
});

describe("server store", () => {
  it("loads the snapshot of an existing connection and follows events", async () => {
    const api = await connectedMock();
    const { result } = renderHook(() => useServerState());
    await waitFor(() => {
      expect(result.current.serverId).toBe(1);
    });
    expect(result.current.users.get(2)?.name).toBe("Anna");

    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(result.current.talking.has(2)).toBe(true);

    await act(() => api.channelJoin(2));
    expect(result.current.users.get(1)?.channelId).toBe(2);

    act(() => {
      api.mock.emit("channelUpserted", {
        channel: {
          id: 9,
          parentId: 2,
          name: "Sub",
          position: 0,
          permissions: { enter: true, textMessage: true, listen: true, registerSelf: false },
          description: null,
          hasDescription: false,
        },
      });
      api.mock.emit("userRemoved", { session: 2 });
    });
    expect(result.current.channels.get(9)?.name).toBe("Sub");
    expect(result.current.users.has(2)).toBe(false);
    expect(result.current.talking.has(2)).toBe(false);

    await act(() => api.disconnect());
    expect(result.current.serverId).toBeNull();
    expect(result.current.users.size).toBe(0);
  });

  it("keeps the tree while reconnecting", async () => {
    const api = await connectedMock();
    renderHook(() => useServerState());
    await waitFor(() => {
      expect(getServerState().serverId).toBe(1);
    });
    act(() => {
      api.mock.loseConnection();
    });
    expect(getServerState().connection.state).toBe("reconnecting");
    expect(getServerState().users.size).toBe(4);
  });
});

describe("selected conversation", () => {
  it("starts at the own channel, follows selection and clears on disconnect", async () => {
    const api = await connectedMock();
    const { result } = renderHook(() => useSelectedConversation());
    await waitFor(() => {
      expect(result.current.conversation).toEqual({ kind: "channel", channelId: 1 });
    });

    act(() => {
      result.current.select({ kind: "channel", channelId: 3 });
    });
    expect(getSelectedConversation()).toEqual({ kind: "channel", channelId: 3 });

    act(() => {
      api.mock.emit("channelRemoved", { channelId: 3 });
    });
    expect(result.current.conversation).toEqual({ kind: "channel", channelId: 1 });

    // A reconnect keeps the current selection.
    act(() => {
      result.current.select({ kind: "private", certHash: "annahash" });
      api.mock.loseConnection();
      api.mock.completeReconnect();
    });
    expect(result.current.conversation).toEqual({ kind: "private", certHash: "annahash" });

    await act(() => api.disconnect());
    expect(result.current.conversation).toBeNull();

    await act(() => api.connect(1, null));
    expect(result.current.conversation).toEqual({ kind: "channel", channelId: 1 });
  });
});

describe("tree model", () => {
  it("nests channels by position, people first, and hides collapsed content", async () => {
    await connectedMock();
    renderHook(() => useServerState());
    await waitFor(() => {
      expect(getServerState().serverId).toBe(1);
    });
    const roots = buildTree(getServerState());
    expect(roots.map((r) => r.channel.name)).toEqual(["Root"]);
    expect(roots[0]?.children.map((c) => c.channel.name)).toEqual(["Lobby", "Raid", "AFK"]);
    const lobby = roots[0]?.children[0];
    expect(lobby?.members.map((u) => u.name)).toEqual(["Anna", "Ben", "Zoe"]);
    // Carla only listens to Lobby: she is not part of the tree.

    expect(visibleRows(roots, new Set()).map((r) => r.id)).toEqual([
      "c0",
      "c1",
      "u2",
      "u3",
      "u1",
      "c2",
      "u4",
      "c3",
    ]);
    expect(visibleRows(roots, new Set([1])).map((r) => r.id)).toEqual([
      "c0",
      "c1",
      "c2",
      "u4",
      "c3",
    ]);
  });
});

describe("per-frame batching", () => {
  it("applies a burst of events as one state change in the next frame", async () => {
    const api = await connectedMock();
    await waitFor(() => {
      expect(getServerState().serverId).toBe(1);
    });
    let pending: (() => void) | null = null;
    setUpdateScheduler((flush) => {
      pending = flush;
      return () => {
        pending = null;
      };
    });
    const { result } = renderHook(() => useServerState());
    const renders = { count: 0 };
    renderHook(() => {
      useServerState();
      renders.count += 1;
    });
    const changes: ServerChange[] = [];
    subscribeServerChanges((change) => changes.push(change));
    const before = result.current;
    const anna = before.users.get(2) as User;
    const zoe = before.users.get(1) as User;

    act(() => {
      for (let id = 10; id < 40; id += 1) {
        api.mock.emit("channelUpserted", { channel: channel(id) });
      }
      api.mock.emit("userUpserted", { user: { ...anna, channelId: 2 } });
      api.mock.emit("talkStateChanged", { session: 3, talking: true });
    });
    expect(result.current).toBe(before);
    expect(pending).not.toBeNull();

    const rendersBefore = renders.count;
    act(() => {
      (pending as unknown as () => void)();
    });
    expect(renders.count).toBe(rendersBefore + 1);
    expect(result.current.channels.size).toBe(before.channels.size + 30);
    expect(result.current.users.get(2)?.channelId).toBe(2);
    expect(result.current.talking.has(3)).toBe(true);
    // Unchanged people keep their identity, so their memoised rows skip re-rendering.
    expect(result.current.users.get(1)).toBe(zoe);
    // Runs of channel changes collapse into one; the others keep their order.
    expect(changes.map((c) => c.kind)).toEqual(["channels", "userUpserted", "talk"]);
  });

  it("flushes pending events when the state is read outside React", async () => {
    const api = await connectedMock();
    await waitFor(() => {
      expect(getServerState().serverId).toBe(1);
    });
    setUpdateScheduler(() => () => undefined);
    api.mock.emit("channelsUpserted", { channels: [channel(50), channel(51)] });
    expect(getServerState().channels.get(51)?.name).toBe("C51");
  });
});

describe("applyEvents", () => {
  const base = (): ServerState => ({
    ...getServerState(),
    channels: new Map([[0, channel(0)]]),
    users: new Map(),
    talking: new Set(),
  });

  it("copies each map once and keeps untouched maps", () => {
    const start = base();
    const { next } = applyEvents(start, [
      { kind: "channelUpserted", channel: channel(1) },
      { kind: "channelsUpserted", channels: [channel(2), channel(3)] },
      { kind: "channelRemoved", channelId: 2 },
    ]);
    expect([...next.channels.keys()]).toEqual([0, 1, 3]);
    expect(next.users).toBe(start.users);
    expect(next.talking).toBe(start.talking);
    expect(start.channels.size).toBe(1);
  });

  it("keeps the order: a disconnect in the middle clears what came before", () => {
    const { next, changes } = applyEvents(base(), [
      { kind: "channelUpserted", channel: channel(1) },
      { kind: "connection", connection: { state: "disconnected", error: null } },
      { kind: "talk", session: 5, talking: true },
    ]);
    expect(next.channels.size).toBe(0);
    expect(next.talking.has(5)).toBe(true);
    expect(changes.map((c) => c.kind)).toEqual(["channels", "connection", "talk"]);
  });

  it("ignores no-op talk changes and unknown removals", () => {
    const start = base();
    const { next, changes } = applyEvents(start, [
      { kind: "talk", session: 5, talking: false },
      { kind: "userRemoved", session: 99 },
    ]);
    expect(next).toBe(start);
    expect(changes).toEqual([]);
  });
});

describe("tree identity", () => {
  it("keeps unchanged nodes when one person moves", async () => {
    await connectedMock();
    await waitFor(() => {
      expect(getServerState().serverId).toBe(1);
    });
    const state = getServerState();
    const first = buildTree(state);
    expect(buildTree(state, first)).toBe(first);
    const anna = state.users.get(2) as User;
    const users = new Map(state.users).set(2, { ...anna, channelId: 3 });
    const second = buildTree({ channels: state.channels, users }, first);
    const [root] = first;
    const [root2] = second;
    expect(root2).not.toBe(root);
    // Raid (2) is untouched; Lobby (1) and AFK (3) changed.
    expect(root2?.children[1]).toBe(root?.children[1]);
    expect(root2?.children[0]).not.toBe(root?.children[0]);
    expect(root2?.children[2]?.members.map((u) => u.name)).toEqual(["Anna"]);
  });
});
