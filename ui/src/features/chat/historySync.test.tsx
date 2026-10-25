import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatMessage, Conversation } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import {
  ensureLoaded,
  HISTORY_PAGE,
  insertByTime,
  requestSync,
  resetChatStore,
  useConversationLog,
  type ConversationLog,
} from "./chatStore";
import { MessageList } from "./MessageList";

// The mock puts the own user into Lobby.
const LOBBY: Conversation = { kind: "channel", channelId: 1 };
const NOW = Date.now();

function message(
  id: string,
  minutesAgo: number,
  body: string,
  syncedFrom: string | null = null,
): ChatMessage {
  return {
    id,
    conversation: LOBBY,
    senderName: "Maya",
    senderCertHash: "maya-hash",
    senderSession: null,
    own: false,
    sentAt: new Date(NOW - minutesAgo * 60_000).toISOString(),
    body,
    dismissedPreviews: [],
    syncedFrom,
  };
}

function bodies(messages: readonly ChatMessage[]): string[] {
  return messages.map((m) => m.body);
}

afterEach(() => {
  vi.restoreAllMocks();
  resetChatStore();
  resetAll();
});

describe("history sync in the chat store (FR-1.60b)", () => {
  it("inserts by time and keeps the engine's order for equal times", () => {
    const list = [message("a", 10, "a"), message("d", 1, "d")];
    const synced = [message("e", 20, "e"), message("b", 5, "b"), message("c", 5, "c")];
    expect(bodies(insertByTime(list, synced))).toEqual(["e", "a", "b", "c", "d"]);
    expect(bodies(insertByTime([], synced))).toEqual(["e", "b", "c"]);
  });

  it("puts synced messages between the shown ones, marked with their origin", async () => {
    const api = await connectedMock();
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    act(() => {
      ensureLoaded(LOBBY);
      api.mock.emit("messageReceived", { message: message("a", 10, "a") });
      api.mock.emit("messageReceived", { message: message("d", 1, "d") });
    });
    await waitFor(() => {
      expect(result.current.loaded).toBe(true);
    });
    act(() => {
      api.mock.emit("historySynced", {
        channelId: 1,
        messages: [message("b", 5, "b", "Alice"), message("c", 4, "c", "Alice")],
      });
    });
    expect(bodies(result.current.messages)).toEqual(["a", "b", "c", "d"]);
    expect(result.current.messages[1]?.syncedFrom).toBe("Alice");
  });

  it("leaves synced messages older than the loaded window to paging", async () => {
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
      expect(result.current.hasMore).toBe(true);
    });
    const recent = { ...message("new", 0, "recent"), sentAt: new Date().toISOString() };
    act(() => {
      api.mock.emit("historySynced", {
        channelId: 1,
        messages: [message("old", 24 * 60, "a day ago", "Alice"), recent],
      });
    });
    expect(bodies(result.current.messages)).not.toContain("a day ago");
    expect(bodies(result.current.messages)).toContain("recent");
  });

  it("asks other Mumcord users on request and reports the result", async () => {
    const api = await connectedMock();
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    act(() => {
      ensureLoaded(LOBBY);
    });
    await waitFor(() => {
      expect(result.current.loaded).toBe(true);
    });
    api.mock.offerHistory(1, "Alice", [
      { senderName: "Bob", body: "earlier", sentAt: new Date(NOW - 7_200_000).toISOString() },
    ]);
    await act(() => requestSync(LOBBY));
    expect(bodies(result.current.messages)).toEqual(["earlier"]);
    expect(result.current.messages[0]?.syncedFrom).toBe("Alice");
    expect(result.current.syncing).toBe(false);
    expect(result.current.syncResult).toBe(1);
  });

  it("shows a running sync but no result for syncs the user did not ask for", async () => {
    const api = await connectedMock();
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    act(() => {
      api.mock.emit("historySyncStatus", { channelId: 1, running: true, added: 0 });
    });
    expect(result.current.syncing).toBe(true);
    act(() => {
      api.mock.emit("historySyncStatus", { channelId: 1, running: false, added: 3 });
    });
    expect(result.current.syncing).toBe(false);
    expect(result.current.syncResult).toBeNull();
  });

  it("reports when earlier messages cannot be requested", async () => {
    await connectedMock();
    resetChatStore();
    const elsewhere: Conversation = { kind: "channel", channelId: 2 };
    const { result } = renderHook(() => useConversationLog(elsewhere));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await act(() => requestSync(elsewhere));
    expect(result.current.syncFailed).toBe(true);
  });
});

const BASE_LOG: ConversationLog = {
  messages: [],
  loaded: true,
  loading: false,
  hasMore: false,
  loadFailed: false,
  syncing: false,
  syncResult: null,
  syncFailed: false,
};

const ROW = 20;
const VIEW = 100;

function rect(top: number, height: number): DOMRect {
  return {
    top,
    bottom: top + height,
    left: 0,
    right: 0,
    width: 0,
    height,
    x: 0,
    y: top,
    toJSON: () => ({}),
  };
}

/** Gives the list a fake layout: every message is one row, the view shows five rows. */
function fakeLayout(): HTMLElement {
  const list = document.querySelector<HTMLElement>(".chat-list");
  if (!list) throw new Error("no list");
  let scrollTop = 0;
  const items = () => [...list.querySelectorAll("li.chat-message")];
  Object.defineProperty(list, "scrollTop", {
    configurable: true,
    get: () => scrollTop,
    set: (value: number) => {
      scrollTop = Math.max(0, Math.min(value, items().length * ROW - VIEW));
    },
  });
  Object.defineProperty(list, "clientHeight", { configurable: true, get: () => VIEW });
  Object.defineProperty(list, "scrollHeight", {
    configurable: true,
    get: () => items().length * ROW,
  });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this === list) return rect(0, VIEW);
    return rect(items().indexOf(this) * ROW - scrollTop, ROW);
  });
  return list;
}

function scrollTo(list: HTMLElement, top: number) {
  list.scrollTop = top;
  fireEvent.scroll(list);
}

function topMessage(list: HTMLElement): string | undefined {
  const items = [...list.querySelectorAll("li.chat-message")];
  const first = items.find((item) => item.getBoundingClientRect().bottom > 0);
  return first?.querySelector(".chat-message__body")?.textContent ?? undefined;
}

const TEN = Array.from({ length: 10 }, (_, i) =>
  message(`m${String(i)}`, 100 - i, `m${String(i)}`),
);

function renderLog(log: Partial<ConversationLog>, onSyncEarlier?: () => void) {
  const view = (next: Partial<ConversationLog>) => (
    <main>
      <MessageList
        log={{ ...BASE_LOG, ...next }}
        label="Messages in Lobby"
        onLoadOlder={() => undefined}
        onSyncEarlier={onSyncEarlier}
      />
    </main>
  );
  const result = render(view(log));
  return { ...result, update: (next: Partial<ConversationLog>) => result.rerender(view(next)) };
}

describe("synced messages in the message list (FR-1.60b, AC-1.51d)", () => {
  it("marks synced messages with the person whose history they came from", async () => {
    await connectedMock();
    renderLog({ messages: [message("1", 5, "hello", "Alice"), message("2", 1, "live")] });
    expect(screen.getByText("from Alice's history")).toBeInTheDocument();
    expect(screen.getAllByText(/history/)).toHaveLength(1);
  });

  it("offers earlier messages once the local history is exhausted and shows progress", async () => {
    await connectedMock();
    const onSync = vi.fn();
    const { update } = renderLog({ messages: TEN }, onSync);
    fireEvent.click(screen.getByRole("button", { name: "Load earlier messages" }));
    expect(onSync).toHaveBeenCalledTimes(1);
    update({ messages: TEN, syncing: true });
    expect(screen.getByRole("button", { name: "Load earlier messages" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Looking for earlier messages from other Mumcord users…",
    );
    update({ messages: TEN, syncResult: 2 });
    expect(screen.getByRole("status")).toHaveTextContent(
      "2 earlier messages added from other Mumcord users.",
    );
    update({ messages: TEN, syncResult: 0 });
    expect(screen.getByRole("status")).toHaveTextContent("No earlier messages found.");
  });

  it("keeps the visible message in place when older messages are inserted above it", async () => {
    await connectedMock();
    const { update } = renderLog({ messages: TEN });
    const list = fakeLayout();
    scrollTo(list, 40);
    expect(topMessage(list)).toBe("m2");

    // Synced messages land before and between the shown ones, all above the visible one.
    const synced = [
      message("s0", 200, "s0", "Alice"),
      message("s1", 99.5, "s1", "Alice"),
      message("s2", 98.5, "s2", "Alice"),
    ];
    update({ messages: insertByTime(TEN, synced) });
    expect(topMessage(list)).toBe("m2");
    expect(list.scrollTop).toBe(100);

    // Inserted below the visible message: nothing moves.
    update({ messages: insertByTime(insertByTime(TEN, synced), [message("s3", 95.5, "s3", "A")]) });
    expect(topMessage(list)).toBe("m2");
    expect(list.scrollTop).toBe(100);
  });

  it("shows Jump to newest with the count of new messages while scrolled up", async () => {
    await connectedMock();
    const { update } = renderLog({ messages: TEN });
    const list = fakeLayout();
    expect(screen.queryByRole("button", { name: /Jump to newest/ })).toBeNull();
    scrollTo(list, 0);
    expect(screen.getByRole("button", { name: "Jump to newest" })).toBeInTheDocument();

    const arrived = [message("n1", 0.3, "n1"), message("n2", 0.2, "n2"), message("n3", 0.1, "n3")];
    update({ messages: [...TEN, ...arrived] });
    const jump = screen.getByRole("button", { name: "Jump to newest, 3 new messages" });
    expect(jump).toHaveTextContent("3");
    expect(topMessage(list)).toBe("m0");

    fireEvent.click(jump);
    expect(list.scrollTop).toBe(13 * ROW - VIEW);
    expect(screen.queryByRole("button", { name: /Jump to newest/ })).toBeNull();
  });

  it("does not count synced history as new and jumps with Ctrl+End", async () => {
    await connectedMock();
    const { update } = renderLog({ messages: TEN });
    const list = fakeLayout();
    scrollTo(list, 0);
    update({ messages: [...TEN, message("s", 0.5, "s", "Alice")] });
    expect(screen.getByRole("button", { name: "Jump to newest" })).toBeInTheDocument();

    fireEvent.keyDown(list, { key: "End", ctrlKey: true });
    expect(list.scrollTop).toBe(11 * ROW - VIEW);
    expect(screen.queryByRole("button", { name: /Jump to newest/ })).toBeNull();
  });
});
