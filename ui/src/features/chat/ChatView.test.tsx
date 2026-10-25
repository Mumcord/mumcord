import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { subscribeAnnouncements } from "../../a11y/announcer";
import type { MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { selectConversation } from "../channels";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { resetFriendsStore } from "../friends";
import { resetChatStore } from "./chatStore";
import { ChatView } from "./ChatView";

const opener = vi.hoisted(() => ({ openUrl: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

const LOBBY = { kind: "channel", channelId: 1 } as const;

async function renderChat(): Promise<MockApi> {
  const api = await connectedMock();
  resetChatStore();
  stubDialog();
  // The app shell provides the landmark; tests wrap the view in one for axe.
  render(
    <main>
      <ChatView renderHeaderActions={() => <button type="button">Follow</button>} />
    </main>,
  );
  await screen.findByText(/No messages yet/);
  return api;
}

function composer(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: "Message #Lobby" });
}

function send(text: string): void {
  fireEvent.change(composer(), { target: { value: text } });
  fireEvent.keyDown(composer(), { key: "Enter" });
}

beforeEach(() => {
  opener.openUrl.mockClear();
});

afterEach(() => {
  resetChatStore();
  resetFriendsStore();
  resetAll();
  vi.restoreAllMocks();
});

describe("ChatView", () => {
  it("asks to pick a conversation when none is selected", () => {
    resetAll();
    render(<ChatView />);
    expect(screen.getByText(/pick a channel/)).toBeInTheDocument();
  });

  it("shows header, header actions and the empty state", async () => {
    await renderChat();
    expect(screen.getByRole("heading", { level: 2 })).toHaveTextContent("Lobby");
    expect(screen.getByRole("button", { name: "Follow" })).toBeInTheDocument();
    expect(composer()).toBeEnabled();
  });

  it("sends Markdown as HTML and shows the own message", async () => {
    const api = await renderChat();
    const spy = vi.spyOn(api, "sendChannelMessage");
    send("hey **there**");
    await waitFor(() => {
      expect(spy).toHaveBeenCalledWith(1, "hey <strong>there</strong>");
    });
    const list = await screen.findByRole("list", { name: "Messages in #Lobby" });
    expect(list.querySelector("strong")).toHaveTextContent("there");
    expect(composer()).toHaveValue("");
  });

  it("shows a server denial inline", async () => {
    const api = await renderChat();
    api.mock.failNext("sendChannelMessage", "permissionDenied", "Not here");
    send("hello");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "didn't allow this message: Not here",
    );
    expect(composer()).toHaveValue("hello");
  });

  it("explains too-long messages the same way for local and server checks", async () => {
    const api = await renderChat();
    send("x".repeat(5001));
    expect(await screen.findByRole("alert")).toHaveTextContent("at most 5000 characters");

    api.mock.failNext("sendChannelMessage", "messageTooLarge");
    send("short");
    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("at most 5000 characters");
    });
  });

  it("disables the input without the text message permission", async () => {
    await renderChat();
    act(() => {
      selectConversation({ kind: "channel", channelId: 3 });
    });
    const input = await screen.findByRole("textbox", { name: "Message #AFK" });
    expect(input).toBeDisabled();
    expect(screen.getByText(/don't have permission to write/)).toBeInTheDocument();
  });

  it("opens links only on click, through the system opener", async () => {
    const api = await renderChat();
    act(() => {
      api.mock.receiveMessage(LOBBY, '<a href="https://example.test/">site</a>', 2);
    });
    const link = await screen.findByText("site");
    expect(opener.openUrl).not.toHaveBeenCalled();

    const internals = window as unknown as Record<string, unknown>;
    internals.__TAURI_INTERNALS__ = {};
    try {
      fireEvent.click(link);
      await waitFor(() => {
        expect(opener.openUrl).toHaveBeenCalledWith("https://example.test/");
      });
    } finally {
      delete internals.__TAURI_INTERNALS__;
    }

    const open = vi.spyOn(window, "open").mockReturnValue(null);
    fireEvent.click(link);
    await waitFor(() => {
      expect(open).toHaveBeenCalledWith("https://example.test/", "_blank", "noopener,noreferrer");
    });
  });

  it("announces new messages politely and keeps one Tab stop in the list", async () => {
    const api = await renderChat();
    const spoken: string[] = [];
    const stop = subscribeAnnouncements(({ message, politeness }) => {
      if (politeness === "polite") spoken.push(message);
    });
    act(() => {
      api.mock.receiveMessage(LOBBY, "one", 2);
      api.mock.receiveMessage(LOBBY, "two", 2);
    });
    stop();
    expect(spoken).toEqual(["Anna: one"]);

    const items = screen.getAllByRole("listitem");
    expect(items.filter((item) => item.tabIndex === 0)).toHaveLength(1);
    expect(items.at(-1)?.tabIndex).toBe(0);
    items.at(-1)?.focus();
    fireEvent.keyDown(items.at(-1) as HTMLElement, { key: "ArrowUp" });
    expect(document.activeElement).toBe(items[0]);
    expect(items[0]?.tabIndex).toBe(0);
  });

  it("has no axe violations", async () => {
    const api = await renderChat();
    act(() => {
      api.mock.receiveMessage(LOBBY, "<b>hi</b> there", 2);
    });
    await screen.findByText("there", { exact: false });
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("pins the channel description and fetches a hash-only one when the chat opens", async () => {
    const api = await renderChat();
    expect(screen.getByRole("region", { name: "Channel info" })).toHaveTextContent(
      "Welcome to the Lobby!",
    );
    const request = vi.spyOn(api, "channelDescriptionRequest");
    act(() => {
      selectConversation({ kind: "channel", channelId: 2 });
    });
    expect(await screen.findByRole("region", { name: "Channel info" })).toHaveTextContent(
      "Raid rules",
    );
    expect(request).toHaveBeenCalledWith(2);
  });

  it("pins the welcome text in the root channel and nothing where there is no text", async () => {
    await renderChat();
    act(() => {
      selectConversation({ kind: "channel", channelId: 0 });
    });
    expect(await screen.findByRole("region", { name: "Channel info" })).toHaveTextContent(
      "Welcome to the mock server.",
    );
    act(() => {
      selectConversation({ kind: "channel", channelId: 3 });
    });
    await waitFor(() => {
      expect(screen.queryByRole("region", { name: "Channel info" })).toBeNull();
    });
  });
});

describe("ChatView private chat presence (FR-1.65)", () => {
  it("shows whether the other person is online and keeps a friend's name when they leave", async () => {
    const api = await renderChat();
    await api.friendAdd(2);
    act(() => {
      selectConversation({ kind: "private", certHash: "annahash" });
    });
    const header = await screen.findByRole("banner");
    expect(header).toHaveTextContent("Anna");
    expect(header).toHaveTextContent("Online");
    act(() => {
      api.mock.removeUser(2);
    });
    await waitFor(() => {
      expect(header).toHaveTextContent(/Offline, last seen /);
    });
    expect(header).toHaveTextContent("Anna");
  });
});
