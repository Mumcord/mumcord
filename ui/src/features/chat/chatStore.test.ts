import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Conversation, FileCard } from "../../api";
import type { RelayInfo } from "../../bindings/commands";
import { connectedMock, resetAll } from "../channels/testUtils";
import {
  addSentMessage,
  clearConversation,
  ensureLoaded,
  HISTORY_PAGE,
  loadOlder,
  resetChatStore,
  useConversationLog,
} from "./chatStore";

const LOBBY: Conversation = { kind: "channel", channelId: 1 };

afterEach(() => {
  resetChatStore();
  resetAll();
});

describe("chat store", () => {
  it("collects live messages per conversation and de-duplicates by id", async () => {
    const api = await connectedMock({ bindStore: true });
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    let first: ReturnType<typeof api.mock.receiveMessage> | undefined;
    act(() => {
      first = api.mock.receiveMessage(LOBBY, "hi", 2);
      api.mock.receiveMessage({ kind: "channel", channelId: 2 }, "elsewhere", 4);
    });
    expect(result.current.messages.map((m) => m.body)).toEqual(["hi"]);
    act(() => {
      if (first) api.mock.emit("messageReceived", { serverId: 1, message: first });
    });
    expect(result.current.messages).toHaveLength(1);
  });

  it("puts a file card's new state into its message (FR-7.20)", async () => {
    const api = await connectedMock({ bindStore: true });
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    const offer: FileCard = {
      shareId: "s1",
      name: "slides.pdf",
      size: 1000,
      mime: "application/pdf",
      sha256: "ab".repeat(32),
      expires: 0,
      senderName: "Alice",
      own: false,
      risky: false,
      state: { kind: "available" },
      downloadBlock: null,
      share: null,
    };
    act(() => {
      api.mock.receiveMessage(LOBBY, "offer", 2, offer);
      api.mock.receiveMessage(LOBBY, "text", 2);
    });
    const text = result.current.messages[1];
    act(() => {
      api.mock.emit("fileCardChanged", {
        serverId: 1,
        card: { ...offer, state: { kind: "queued", position: 1 } },
      });
      api.mock.emit("fileCardChanged", {
        serverId: 2,
        card: { ...offer, state: { kind: "checking" } },
      });
    });
    expect(result.current.messages[0]?.file?.state).toEqual({ kind: "queued", position: 1 });
    expect(result.current.messages[1]).toBe(text);
  });

  it("reads the file cards of a server again when its relay changes", async () => {
    const api = await connectedMock({ bindStore: true });
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    const offer: FileCard = {
      shareId: "s1",
      name: "slides.pdf",
      size: 1000,
      mime: "application/pdf",
      sha256: "ab".repeat(32),
      expires: 0,
      senderName: "Alice",
      own: false,
      risky: false,
      state: { kind: "available" },
      downloadBlock: null,
      share: null,
    };
    act(() => {
      api.mock.receiveMessage(LOBBY, "offer", 2, offer);
    });
    const blocked: FileCard = {
      ...offer,
      downloadBlock: { kind: "unavailable", reason: { kind: "noRelay" } },
    };
    const read = vi.spyOn(api, "filesCard").mockResolvedValue(blocked);
    act(() => {
      api.mock.emit("relayChanged", { serverId: 1, info: {} as RelayInfo });
    });
    await waitFor(() => {
      expect(result.current.messages[0]?.file?.downloadBlock).toEqual(blocked.downloadBlock);
    });
    expect(read).toHaveBeenCalledWith(1, "s1");
  });

  it("loads history pages, merges live messages and pages backwards", async () => {
    const api = await connectedMock({ bindStore: true });
    resetChatStore();
    for (let i = 0; i < HISTORY_PAGE + 5; i += 1)
      api.mock.receiveMessage(LOBBY, `m${String(i)}`, 2);
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    act(() => {
      ensureLoaded(LOBBY);
    });
    await waitFor(() => {
      expect(result.current.loaded).toBe(true);
    });
    expect(result.current.messages).toHaveLength(HISTORY_PAGE);
    expect(result.current.hasMore).toBe(true);
    expect(result.current.messages.at(-1)?.body).toBe(`m${String(HISTORY_PAGE + 4)}`);

    await act(() => loadOlder(LOBBY));
    expect(result.current.messages).toHaveLength(HISTORY_PAGE + 5);
    expect(result.current.messages[0]?.body).toBe("m0");
    expect(result.current.hasMore).toBe(false);
    expect(new Set(result.current.messages.map((m) => m.id)).size).toBe(HISTORY_PAGE + 5);
  });

  it("adds own sent messages and clears on resync, disconnect and on request", async () => {
    const api = await connectedMock({ bindStore: true });
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    const sent = await api.sendChannelMessage(1, 1, "mine");
    act(() => {
      addSentMessage(sent);
      addSentMessage(sent);
    });
    expect(result.current.messages.map((m) => m.body)).toEqual(["mine"]);

    act(() => {
      clearConversation(LOBBY);
    });
    expect(result.current.messages).toHaveLength(0);

    act(() => {
      addSentMessage(sent);
      api.mock.loseConnection();
      api.mock.completeReconnect();
    });
    expect(result.current.messages).toHaveLength(0);

    act(() => {
      addSentMessage(sent);
    });
    await act(() => api.disconnect(1));
    expect(result.current.messages).toHaveLength(0);
  });
});
