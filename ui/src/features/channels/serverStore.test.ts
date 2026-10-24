import { renderHook, act, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { getSelectedConversation, useSelectedConversation } from "./selection";
import { getServerState, useServerState } from "./serverStore";
import { connectedMock, resetAll } from "./testUtils";
import { buildTree, visibleRows } from "./tree";

afterEach(resetAll);

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
