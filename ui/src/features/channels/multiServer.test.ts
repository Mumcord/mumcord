import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Conversation, MockApi } from "../../api";
import { resetChatStore, useConversationLog } from "../chat/chatStore";
import { getFollowState, resetFollowStore } from "../follow/followStore";
import { getSelectedConversation, selectConversation, useSelectedConversation } from "./selection";
import {
  getServerState,
  getServerStateOf,
  getShownServerId,
  getVoiceServerId,
  showServer,
  subscribeAllServerChanges,
  subscribeServerChanges,
  type ServerChange,
} from "./serverStore";
import { connectedMock, resetAll } from "./testUtils";

afterEach(() => {
  resetChatStore();
  resetFollowStore();
  resetAll();
});

const LOBBY: Conversation = { kind: "channel", channelId: 1 };

/** Connected to the seeded server 1 (shown, read-only) and a second saved server 2. */
async function twoServers(): Promise<MockApi> {
  const api = await connectedMock({ bindStore: true });
  const second = await api.serverAdd({
    label: "Second",
    address: "two.example.org",
    port: 64738,
    username: "Zoe",
    password: { kind: "clear" },
  });
  expect(second.id).toBe(2);
  await api.connect(2, null, null);
  api.mock.focusServer(1);
  return api;
}

describe("several connected servers", () => {
  it("keeps one state per server; events of one server leave the other alone", async () => {
    const api = await twoServers();
    const before = getServerStateOf(2);
    expect(getServerStateOf(1).serverId).toBe(1);
    expect(before.serverId).toBe(2);

    act(() => {
      api.mock.emit("channelRemoved", { serverId: 1, channelId: 3 });
      api.mock.emit("userRemoved", { serverId: 1, session: 2 });
      api.mock.emit("talkStateChanged", { serverId: 1, session: 3, talking: true });
    });
    expect(getServerStateOf(1).channels.has(3)).toBe(false);
    expect(getServerStateOf(1).users.has(2)).toBe(false);
    expect(getServerStateOf(1).talking.has(3)).toBe(true);
    expect(getServerStateOf(2)).toBe(before);
  });

  it("shows the first server that connects, and switches only on request", async () => {
    await twoServers();
    expect(getShownServerId()).toBe(1);
    expect(getServerState().serverId).toBe(1);

    const changes: ServerChange["kind"][] = [];
    const unsubscribe = subscribeServerChanges((change) => {
      changes.push(change.kind);
    });
    act(() => {
      showServer(2);
    });
    expect(getShownServerId()).toBe(2);
    expect(getServerState().serverId).toBe(2);
    expect(changes).toEqual(["shown"]);
    unsubscribe();
  });

  it("reports changes of the shown server only, and every server to all-listeners", async () => {
    const api = await twoServers();
    const shown: ServerChange["kind"][] = [];
    const all: number[] = [];
    subscribeServerChanges((change) => {
      shown.push(change.kind);
    });
    subscribeAllServerChanges((serverId) => {
      all.push(serverId);
    });
    act(() => {
      api.mock.emit("channelRemoved", { serverId: 2, channelId: 3 });
    });
    expect(shown).toEqual([]);
    expect(all).toEqual([2]);
  });

  it("clears only the disconnected server and falls back to another shown server", async () => {
    const api = await twoServers();
    const second = getServerStateOf(2);
    await act(() => api.disconnect(1));
    expect(getServerStateOf(1).serverId).toBeNull();
    expect(getServerStateOf(2)).toBe(second);
    expect(getShownServerId()).toBe(2);
    expect(getServerState()).toBe(second);

    await act(() => api.disconnect(2));
    expect(getShownServerId()).toBeNull();
    expect(getServerState().serverId).toBeNull();
  });

  it("moves voice to the server where a channel is joined (FR-2.2)", async () => {
    const api = await twoServers();
    expect(getVoiceServerId()).toBeNull();
    await act(() => api.channelJoin(2, 2));
    expect(getVoiceServerId()).toBe(2);
    expect(getShownServerId()).toBe(1);

    await act(() => api.channelJoin(1, 2));
    expect(getVoiceServerId()).toBe(1);
    // The previous voice server stays connected, read-only again.
    const self = getServerStateOf(2).users.get(1);
    expect(self).toMatchObject({ selfMute: true, selfDeaf: true });
  });

  it("remembers the selected conversation per server", async () => {
    await twoServers();
    renderHook(() => useSelectedConversation());
    expect(getSelectedConversation()).toEqual(LOBBY);
    act(() => {
      selectConversation({ kind: "channel", channelId: 3 });
      showServer(2);
    });
    // A server shown for the first time starts at the own channel.
    expect(getSelectedConversation()).toEqual(LOBBY);
    act(() => {
      selectConversation({ kind: "channel", channelId: 2 });
      showServer(1);
    });
    expect(getSelectedConversation()).toEqual({ kind: "channel", channelId: 3 });
    act(() => {
      showServer(2);
    });
    expect(getSelectedConversation()).toEqual({ kind: "channel", channelId: 2 });
  });

  it("keeps chat messages per server and clears only the disconnected server's", async () => {
    const api = await twoServers();
    resetChatStore();
    const first = renderHook(() => useConversationLog(LOBBY, 1));
    const second = renderHook(() => useConversationLog(LOBBY, 2));
    const shown = renderHook(() => useConversationLog(LOBBY));
    act(() => {
      api.mock.receiveMessage(LOBBY, "on one", 2);
      api.mock.focusServer(2);
      api.mock.receiveMessage(LOBBY, "on two", 2);
    });
    expect(first.result.current.messages.map((m) => m.body)).toEqual(["on one"]);
    expect(second.result.current.messages.map((m) => m.body)).toEqual(["on two"]);
    expect(shown.result.current.messages.map((m) => m.body)).toEqual(["on one"]);

    act(() => {
      showServer(2);
    });
    expect(shown.result.current.messages.map((m) => m.body)).toEqual(["on two"]);

    await act(() => api.disconnect(1));
    expect(first.result.current.messages).toEqual([]);
    expect(second.result.current.messages.map((m) => m.body)).toEqual(["on two"]);
  });

  it("keeps unread counts and followed channels per server", async () => {
    const api = await twoServers();
    getFollowState();
    act(() => {
      api.mock.emit("unreadChanged", { serverId: 2, conversation: LOBBY, count: 3 });
      api.mock.emit("followedChannelsChanged", { serverId: 2, channelIds: [2] });
    });
    expect(getFollowState(2).unread.get("channel:1")).toBe(3);
    expect(getFollowState(2).followed).toEqual([2]);
    expect(getFollowState(1).unread.size).toBe(0);
    expect(getFollowState(1).followed).toEqual([]);
  });
});
