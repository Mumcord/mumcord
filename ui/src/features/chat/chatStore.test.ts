import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Conversation } from "../../api";
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
    const api = await connectedMock();
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    let first: ReturnType<typeof api.mock.receiveMessage> | undefined;
    act(() => {
      first = api.mock.receiveMessage(LOBBY, "hi", 2);
      api.mock.receiveMessage({ kind: "channel", channelId: 2 }, "elsewhere", 4);
    });
    expect(result.current.messages.map((m) => m.body)).toEqual(["hi"]);
    act(() => {
      if (first) api.mock.emit("messageReceived", { message: first });
    });
    expect(result.current.messages).toHaveLength(1);
  });

  it("loads history pages, merges live messages and pages backwards", async () => {
    const api = await connectedMock();
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
    const api = await connectedMock();
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    const sent = await api.sendChannelMessage(1, "mine");
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
    await act(() => api.disconnect());
    expect(result.current.messages).toHaveLength(0);
  });
});
