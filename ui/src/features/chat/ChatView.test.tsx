import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { subscribeAnnouncements } from "../../a11y/announcer";
import type { MockApi, User } from "../../api";
import { axeViolations } from "../../test/axe";
import { getServerState, selectConversation, shownServerId } from "../channels";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { resetFriendsStore } from "../friends";
import { resetChatStore } from "./chatStore";
import { ChatView } from "./ChatView";

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
      expect(spy).toHaveBeenCalledWith(1, 1, "hey <strong>there</strong>");
    });
    const list = await screen.findByRole("list", { name: "Messages in #Lobby" });
    expect(list.querySelector("strong")).toHaveTextContent("there");
    expect(composer()).toHaveValue("");
  });

  it("shows a server denial under the message and announces it politely", async () => {
    const api = await renderChat();
    const spoken: string[] = [];
    const stop = subscribeAnnouncements(({ message, politeness }) => {
      if (politeness === "polite") spoken.push(message);
    });
    api.mock.failNext("sendChannelMessage", "permissionDenied", "Not here");
    send("hello");
    expect(composer()).toHaveValue("");
    const failed = await screen.findByText("Not sent.");
    stop();
    expect(failed.closest("p")).toHaveTextContent("didn't allow this message: Not here");
    expect(spoken).toContain("A message couldn’t be sent.");
    expect(screen.getByText("hello").closest("li")).toHaveClass("chat-message--pending");

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(screen.queryByText("Not sent.")).not.toBeInTheDocument();
    });
    expect(screen.getAllByText("hello")).toHaveLength(1);
    expect(screen.getByText("hello").closest("li")).not.toHaveClass("chat-message--pending");
  });

  it("keeps a refused message after a failed retry and lets the user delete it", async () => {
    const api = await renderChat();
    api.mock.failNext("sendChannelMessage", "permissionDenied", "Not here");
    send("hello");
    await screen.findByRole("button", { name: "Retry" });

    api.mock.failNext("sendChannelMessage", "permissionDenied", "Still not");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText(/Still not/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.queryByText("hello")).not.toBeInTheDocument();
    expect(screen.queryByText("Not sent.")).not.toBeInTheDocument();
  });

  it("explains too-long messages the same way for local and server checks", async () => {
    const api = await renderChat();
    send("x".repeat(5001));
    expect(await screen.findByRole("alert")).toHaveTextContent("at most 5000 characters");

    api.mock.failNext("sendChannelMessage", "messageTooLarge");
    send("short");
    const failed = await screen.findByText("Not sent.");
    expect(failed.closest("p")).toHaveTextContent("at most 5000 characters");
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

  it("opens links only on click, through the engine's system opener", async () => {
    const api = await renderChat();
    const openLink = vi.spyOn(api, "openLink");
    act(() => {
      api.mock.receiveMessage(LOBBY, '<a href="https://example.test/">site</a>', 2);
    });
    const link = await screen.findByText("site");
    expect(openLink).not.toHaveBeenCalled();

    // The mock engine opens a new tab without access to this page.
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    fireEvent.click(link);
    expect(openLink).toHaveBeenCalledWith("https://example.test/");
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
    expect(request).toHaveBeenCalledWith(1, 2);
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

describe("ChatView instant send (FR-2.74)", () => {
  /** Holds the next send of `command` until `release` runs, so the pending state can be seen. */
  function holdSend(
    api: MockApi,
    command: "sendChannelMessage" | "sendPrivateMessage",
  ): { release: () => void } {
    const original = api[command].bind(api);
    let release = (): void => undefined;
    vi.spyOn(api, command).mockImplementationOnce(
      (serverId, target, html) =>
        new Promise((resolve, reject) => {
          release = () => {
            original(serverId, target, html).then(resolve, reject);
          };
        }),
    );
    return {
      release: () => {
        release();
      },
    };
  }

  it("empties the input at once, greys the message and confirms it in place once", async () => {
    const api = await renderChat();
    const gate = holdSend(api, "sendChannelMessage");
    send("on its way");
    expect(composer()).toHaveValue("");
    const pending = (await screen.findByText("on its way")).closest("li");
    expect(pending).toHaveClass("chat-message--pending");

    act(() => {
      gate.release();
    });
    await waitFor(() => {
      expect(pending).not.toHaveClass("chat-message--pending");
    });
    expect(pending?.isConnected).toBe(true);
    expect(screen.getAllByText("on its way")).toHaveLength(1);
  });

  it("sends direct messages the same way", async () => {
    const api = await renderChat();
    act(() => {
      selectConversation({ kind: "private", certHash: "annahash" });
    });
    const input = await screen.findByRole("textbox", { name: /Anna/ });
    const gate = holdSend(api, "sendPrivateMessage");
    fireEvent.change(input, { target: { value: "hi Anna" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(input).toHaveValue("");
    const pending = (await screen.findByText("hi Anna")).closest("li");
    expect(pending).toHaveClass("chat-message--pending");

    act(() => {
      gate.release();
    });
    await waitFor(() => {
      expect(pending).not.toHaveClass("chat-message--pending");
    });
    expect(screen.getAllByText("hi Anna")).toHaveLength(1);
  });

  it("keeps a failed direct message for retry", async () => {
    const api = await renderChat();
    act(() => {
      selectConversation({ kind: "private", certHash: "annahash" });
    });
    const input = await screen.findByRole("textbox", { name: /Anna/ });
    api.mock.failNext("sendPrivateMessage", "notConnected");
    fireEvent.change(input, { target: { value: "later" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText("Not sent.");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(screen.queryByText("Not sent.")).not.toBeInTheDocument();
    });
    expect(screen.getAllByText("later")).toHaveLength(1);
  });
});

describe("ChatView private chat presence (FR-1.65)", () => {
  it("shows whether the other person is online and keeps a friend's name when they leave", async () => {
    const api = await renderChat();
    await api.friendAdd(shownServerId(), 2);
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

describe("ChatView sender avatars (F21)", () => {
  function anna(): User {
    const user = getServerState().users.get(2);
    if (!user) throw new Error("Anna is part of the mock server");
    return user;
  }

  async function annaWrote(api: MockApi): Promise<HTMLElement> {
    act(() => {
      api.mock.receiveMessage(LOBBY, "hi from Anna", 2);
    });
    return screen.findByRole("list", { name: "Messages in #Lobby" });
  }

  it("keeps showing the sender's avatar after they rejoin with a new session", async () => {
    const api = await renderChat();
    const list = await annaWrote(api);
    await waitFor(() => {
      expect(list.querySelector("img")).not.toBeNull();
    });
    const before = anna();
    act(() => {
      api.mock.removeUser(2);
      api.mock.upsertUser({ ...before, session: 7 });
    });
    await waitFor(() => {
      expect(list.querySelector("img")).not.toBeNull();
    });
  });

  it("shows the avatar once it arrives after a first empty answer", async () => {
    const api = await renderChat();
    const profile = vi.spyOn(api, "personProfile");
    profile.mockResolvedValueOnce({ comment: null, avatar: null, note: "" });
    const list = await annaWrote(api);
    await waitFor(() => {
      expect(profile).toHaveBeenCalledTimes(1);
    });
    expect(list.querySelector("img")).toBeNull();
    // The texture blob arrives later; the hash the server announced stays the same.
    act(() => {
      api.mock.upsertUser({ ...anna() });
    });
    await waitFor(() => {
      expect(list.querySelector("img")).not.toBeNull();
    });
  });

  it("does not report a vanished person as an error and tries again later", async () => {
    const api = await renderChat();
    const error = vi.spyOn(console, "error");
    const debug = vi.spyOn(console, "debug").mockImplementation(() => undefined);
    api.mock.failNext("personProfile", "notFound", "unknown person");
    const list = await annaWrote(api);
    await waitFor(() => {
      expect(debug).toHaveBeenCalledTimes(1);
    });
    expect(error).not.toHaveBeenCalled();
    act(() => {
      api.mock.upsertUser({ ...anna() });
    });
    await waitFor(() => {
      expect(list.querySelector("img")).not.toBeNull();
    });
  });
});
