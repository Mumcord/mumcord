import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ChatDisplay, ChatMessage } from "../../api";
import { getApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { connectedMock, resetAll } from "../channels/testUtils";
import { closePersonMenu, currentPersonMenu } from "../members/personMenuStore";
import {
  closePreview,
  currentPreview,
  PIN_DELAY_MS,
  PREVIEW_OPEN_DELAY_MS,
} from "../members/profilePreviewStore";
import { getSelectedConversation } from "../channels/selection";
import { setUpdateScheduler, useServerState } from "../channels/serverStore";
import { MessageList } from "./MessageList";

// Fixed local noon: messages dated "minutes ago" must never straddle midnight.
const NOW = new Date(2026, 5, 15, 12, 0, 0);

const AVATAR = "data:image/png;base64,iVBORw0KGgo=";

function message(id: string, sender: string, minutesAgo: number, body: string): ChatMessage {
  return {
    id,
    conversation: { kind: "channel", channelId: 1 },
    senderName: sender,
    senderCertHash: `${sender}-hash`,
    senderSession: null,
    own: false,
    sentAt: new Date(NOW.getTime() - minutesAgo * 60_000).toISOString(),
    body,
    dismissedPreviews: [],
    syncedFrom: null,
  };
}

const MESSAGES = [
  message("1", "Maya", 30, "first"),
  message("2", "Maya", 29, "second"),
  message("3", "Jonas", 28, "third"),
];

function renderList(display: ChatDisplay, messages = MESSAGES) {
  return render(
    <main>
      <MessageList
        log={{
          messages,
          loaded: true,
          loading: false,
          hasMore: false,
          loadFailed: false,
          syncing: false,
          syncResult: null,
          syncFailed: false,
        }}
        label="Messages in #Lobby"
        onLoadOlder={() => undefined}
        display={display}
        avatarFor={(m) => (m.senderName === "Maya" ? AVATAR : null)}
      />
    </main>,
  );
}

beforeAll(async () => {
  await applyLanguage("en");
});

describe("MessageList profile preview", () => {
  it("previews every sender on hover of avatar or name, oneself included", () => {
    const messages = [
      { ...message("1", "Maya", 30, "first"), senderSession: 7 },
      { ...message("2", "Jonas", 28, "mine"), senderSession: 1, own: true },
    ];
    const { container } = render(
      <MessageList
        log={{
          messages,
          loaded: true,
          loading: false,
          hasMore: false,
          loadFailed: false,
          syncing: false,
          syncResult: null,
          syncFailed: false,
        }}
        label="Messages in #Lobby"
        onLoadOlder={() => undefined}
        clickablePeople
      />,
    );
    const triggers = container.querySelectorAll(".profile-preview-trigger");
    expect(triggers).toHaveLength(4);
    expect(triggers[0]?.querySelector(".avatar")).not.toBeNull();
    expect(triggers[1]).toContainElement(screen.getByRole("button", { name: "Maya" }));

    vi.useFakeTimers();
    try {
      fireEvent.pointerEnter(triggers[0] as Element, { pointerType: "mouse" });
      vi.advanceTimersByTime(PREVIEW_OPEN_DELAY_MS);
      expect(currentPreview()?.session).toBe(7);
    } finally {
      closePreview();
      vi.useRealTimers();
    }
  });

  it.each<ChatDisplay>(["cozy", "compact"])(
    "opens preview and menu for a sender who is not on the server (%s)",
    (display) => {
      const messages = [message("1", "Ghost", 5, "boo")];
      const { container } = render(
        <MessageList
          log={{
            messages,
            loaded: true,
            loading: false,
            hasMore: false,
            loadFailed: false,
            syncing: false,
            syncResult: null,
            syncFailed: false,
          }}
          label="Messages"
          onLoadOlder={() => undefined}
          display={display}
        />,
      );
      const triggers = container.querySelectorAll(".profile-preview-trigger");
      expect(triggers).toHaveLength(display === "cozy" ? 2 : 1);
      vi.useFakeTimers();
      try {
        fireEvent.pointerEnter(triggers[0] as Element, { pointerType: "mouse" });
        vi.advanceTimersByTime(PREVIEW_OPEN_DELAY_MS);
        expect(currentPreview()).toMatchObject({ session: null, name: "Ghost" });
        fireEvent.contextMenu(triggers[triggers.length - 1] as Element, { clientX: 5, clientY: 5 });
        expect(currentPersonMenu()).toMatchObject({ session: null, name: "Ghost" });
      } finally {
        closePreview();
        closePersonMenu();
        vi.useRealTimers();
      }
    },
  );
});

describe("MessageList cozy", () => {
  it("shows the avatar and name once per group, initials when there is no image", () => {
    const { container } = renderList("cozy");
    const items = screen.getAllByRole("listitem");
    expect(items.map((item) => item.querySelectorAll(".avatar").length)).toEqual([1, 0, 1]);
    expect(items[0]?.querySelector("img")).toHaveAttribute("src", AVATAR);
    expect(items[2]?.querySelector(".avatar")).toHaveAttribute("data-initial", "J");
    expect(container.querySelectorAll(".chat-message__header")).toHaveLength(2);
  });

  it("keeps the avatar and name in the sticky structure (FR-2.62)", () => {
    // jsdom does no layout: assert the header and the avatar sit directly in the row's grid columns, which bound the sticky range.
    const { container } = renderList("cozy");
    const row = container.querySelector(".chat-message__row") as HTMLElement;
    const gutter = row.querySelector(":scope > .chat-message__gutter");
    expect(gutter?.querySelector(".avatar")).not.toBeNull();
    expect(
      row.querySelector(":scope > .chat-message__content > .chat-message__header"),
    ).not.toBeNull();
  });

  it("gives every message sender, time and text once, grouped ones included", () => {
    renderList("cozy");
    const items = screen.getAllByRole("listitem");
    const grouped = items[1] as HTMLElement;
    expect(within(grouped).getByText("Maya")).toHaveClass("visually-hidden");
    expect(grouped.querySelectorAll("time")).toHaveLength(1);
    expect(grouped).toHaveTextContent(/^Maya\s*\d\d:\d\d(\s[AP]M)?\s*second$/);
    expect(items[0]).toHaveTextContent(/Maya\s*\d\d:\d\d(\s[AP]M)?\s*first$/);
  });

  it("puts a day separator above the first message of each day", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
    try {
      const old = { ...message("0", "Priya", 0, "old"), sentAt: "2020-01-02T12:00:00Z" };
      renderList("cozy", [old, ...MESSAGES]);
      const days = document.querySelectorAll(".chat-day");
      expect([...days].map((d) => d.textContent)).toEqual([
        expect.stringContaining("2020"),
        "Today",
      ]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("has no axe violations", async () => {
    renderList("cozy");
    expect(await axeViolations(document.body)).toEqual([]);
  });
});

describe("MessageList compact", () => {
  it("shows time, name and text on one line without avatars", async () => {
    renderList("compact");
    const list = screen.getByRole("list", { name: "Messages in #Lobby" });
    expect(list).toHaveAttribute("data-display", "compact");
    expect(list.querySelectorAll(".avatar")).toHaveLength(0);
    const items = screen.getAllByRole("listitem");
    expect(items[1]).toHaveTextContent(/^\d\d:\d\d(\s[AP]M)?\s*Maya\s*second$/);
    expect(within(items[1] as HTMLElement).getByText("Maya")).not.toHaveClass("visually-hidden");
    expect(await axeViolations(document.body)).toEqual([]);
  });
});

describe("MessageList mentions", () => {
  afterEach(() => {
    resetAll();
  });

  it("highlights messages that mention the user, and only those", async () => {
    await connectedMock();
    renderList("cozy", [
      message("1", "Maya", 5, "<p>hello @zoe</p>"),
      message("2", "Maya", 4, "<p>mail zoe@example.com</p>"),
    ]);
    const items = await screen.findAllByRole("listitem");
    await waitFor(() => {
      expect(items[0]).toHaveClass("chat-message--mention");
    });
    expect(items[0]).toHaveTextContent("Mentions you");
    expect(items[1]).not.toHaveClass("chat-message--mention");
  });

  it("redraws only the messages that changed (AC-2.17)", () => {
    const many = Array.from({ length: 20 }, (_, i) =>
      message(`m${String(i)}`, "Maya", 60 - i, "hi"),
    );
    // Every drawn message reads the time format once, so reads count the messages drawn.
    const proto = Intl.DateTimeFormat.prototype;
    const original = Object.getOwnPropertyDescriptor(proto, "format");
    let count = 0;
    Object.defineProperty(proto, "format", {
      configurable: true,
      get() {
        count++;
        return original?.get?.call(this) as unknown;
      },
    });
    const view = renderList("cozy", many);
    const first = count;
    expect(first).toBeGreaterThanOrEqual(many.length);
    count = 0;
    const log = {
      messages: [...many, message("new", "Maya", 1, "newest")],
      loaded: true,
      loading: false,
      hasMore: false,
      loadFailed: false,
      syncing: false,
      syncResult: null,
      syncFailed: false,
    };
    view.rerender(
      <main>
        <MessageList log={log} label="Messages in #Lobby" onLoadOlder={() => undefined} />
      </main>,
    );
    // The new message and the one that lost the Tab stop; not all 21.
    expect(count).toBeLessThanOrEqual(first / 4);
    if (original) Object.defineProperty(proto, "format", original);
  });

  it("does not apply queued server events while rendering a message (F44)", async () => {
    const api = await connectedMock({ bindStore: true });
    const error = vi.spyOn(console, "error");
    function Talking() {
      return <p>{`talking: ${String(useServerState().talking.size)}`}</p>;
    }
    const log = (messages: ChatMessage[]) => ({
      messages,
      loaded: true,
      loading: false,
      hasMore: false,
      loadFailed: false,
      syncing: false,
      syncResult: null,
      syncFailed: false,
    });
    const list = (messages: ChatMessage[]) => (
      <main>
        <Talking />
        <MessageList log={log(messages)} label="Messages" onLoadOlder={() => undefined} />
      </main>
    );
    const view = render(list(MESSAGES));
    // Let the stores finish loading before events start to wait.
    await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
    // Server events wait for the next frame, as in the app; this frame never comes.
    setUpdateScheduler(() => () => undefined);
    act(() => {
      api.mock.setTalking(2, true);
    });
    view.rerender(list([...MESSAGES, message("4", "Maya", 1, "<p>hello @zoe</p>")]));
    expect(await screen.findByText("Mentions you", { exact: false })).toBeInTheDocument();
    // Rendering the new message left the talking change queued for its frame.
    expect(screen.getByText("talking: 0")).toBeInTheDocument();
    expect(error).not.toHaveBeenCalledWith(
      expect.stringContaining("Cannot update a component"),
      expect.anything(),
      expect.anything(),
      expect.anything(),
    );
    act(() => {
      setUpdateScheduler(null);
    });
    expect(screen.getByText("talking: 1")).toBeInTheDocument();
  });
});

describe("MessageList mention chips", () => {
  afterEach(() => {
    resetAll();
    closePreview();
    closePersonMenu();
  });

  async function renderMention() {
    await connectedMock();
    render(
      <MessageList
        log={{
          messages: [message("1", "Maya", 5, "<p>hi @Anna and @Nobody</p>")],
          loaded: true,
          loading: false,
          hasMore: false,
          loadFailed: false,
          syncing: false,
          syncResult: null,
          syncFailed: false,
        }}
        label="Messages in #Lobby"
        onLoadOlder={() => undefined}
        clickablePeople
      />,
    );
    return { chip: await screen.findByRole("button", { name: "Anna, mention" }) };
  }

  it("makes only people on the server interactive", async () => {
    await renderMention();
    expect(screen.queryByRole("button", { name: /Nobody/ })).toBeNull();
  });

  it("opens the private chat on double click", async () => {
    const { chip } = await renderMention();
    fireEvent.doubleClick(chip);
    expect(getSelectedConversation()).toEqual({ kind: "private", certHash: "annahash" });
  });

  it("previews on hover, pins on click and opens the menu on right-click", async () => {
    const { chip } = await renderMention();
    vi.useFakeTimers();
    try {
      fireEvent.pointerOver(chip);
      vi.advanceTimersByTime(PREVIEW_OPEN_DELAY_MS);
      expect(currentPreview()?.session).toBe(2);
      fireEvent.click(chip);
      vi.advanceTimersByTime(PIN_DELAY_MS);
      expect(currentPreview()?.pinned).toBe(true);
      fireEvent.contextMenu(chip, { clientX: 5, clientY: 6 });
      expect(currentPersonMenu()?.session).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("MessageList replies", () => {
  function renderReplies(messages: ChatMessage[], onReply = vi.fn()) {
    render(
      <MessageList
        log={{
          messages,
          loaded: true,
          loading: false,
          hasMore: false,
          loadFailed: false,
          syncing: false,
          syncResult: null,
          syncFailed: false,
        }}
        label="Messages in #Lobby"
        onLoadOlder={() => undefined}
        onReply={onReply}
      />,
    );
    return onReply;
  }

  it("offers Reply on hover button, context menu and R", () => {
    const onReply = renderReplies(MESSAGES);
    fireEvent.click(screen.getByRole("button", { name: "Reply to Jonas" }));
    expect(onReply).toHaveBeenLastCalledWith({ name: "Jonas", snippet: "third" });

    const items = screen.getAllByRole("listitem");
    fireEvent.contextMenu(items[0] as HTMLElement, { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Reply" }));
    expect(onReply).toHaveBeenLastCalledWith({ name: "Maya", snippet: "first" });

    act(() => {
      (items[1] as HTMLElement).focus();
    });
    fireEvent.keyDown(items[1] as HTMLElement, { key: "r" });
    expect(onReply).toHaveBeenLastCalledWith({ name: "Maya", snippet: "second" });
  });

  describe("link menu", () => {
    const LINK = '<a href="https://example.test/page">site</a>';

    function renderLinks() {
      const onReply = vi.fn();
      render(
        <MessageList
          log={{
            messages: [message("1", "Maya", 30, LINK), message("2", "Jonas", 29, "plain")],
            loaded: true,
            loading: false,
            hasMore: false,
            loadFailed: false,
            syncing: false,
            syncResult: null,
            syncFailed: false,
          }}
          label="Messages in #Lobby"
          onLoadOlder={() => undefined}
          onReply={onReply}
        />,
      );
    }

    it("copies the full URL and confirms it", async () => {
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
      renderLinks();
      fireEvent.contextMenu(screen.getByRole("link", { name: "site" }), {
        clientX: 10,
        clientY: 10,
      });
      expect(screen.queryByRole("menuitem", { name: "Reply" })).toBeNull();
      fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
      expect(writeText).toHaveBeenCalledWith("https://example.test/page");
      expect(await screen.findByText("Link copied")).toBeTruthy();
    });

    it("opens the link through the safe path", () => {
      const openLink = vi.spyOn(getApi(), "openLink").mockResolvedValue(null);
      renderLinks();
      fireEvent.contextMenu(screen.getByRole("link", { name: "site" }), {
        clientX: 10,
        clientY: 10,
      });
      fireEvent.click(screen.getByRole("menuitem", { name: "Open link" }));
      expect(openLink).toHaveBeenCalledWith("https://example.test/page");
    });

    it("opens with the menu key on a focused link (no pointer position)", () => {
      renderLinks();
      fireEvent.contextMenu(screen.getByRole("link", { name: "site" }), {
        clientX: 0,
        clientY: 0,
      });
      expect(screen.getByRole("menu", { name: "Link" })).toBeTruthy();
    });

    it("keeps the message menu for a right-click elsewhere", () => {
      renderLinks();
      fireEvent.contextMenu(screen.getAllByRole("listitem")[1] as HTMLElement, {
        clientX: 10,
        clientY: 10,
      });
      expect(screen.getByRole("menuitem", { name: "Reply" })).toBeTruthy();
      expect(screen.queryByRole("menuitem", { name: "Copy link" })).toBeNull();
    });
  });

  it("shows the reply header as plain text and jumps to the original", () => {
    renderReplies([
      ...MESSAGES,
      message("4", "Jonas", 27, "<blockquote><b>Maya</b>: second</blockquote>agreed"),
      message(
        "5",
        "Jonas",
        26,
        "<blockquote><b>Eve</b>: &lt;img src=x onerror=alert(1)&gt;</blockquote>hm",
      ),
    ]);
    const jump = screen.getByRole("button", {
      name: "Reply to Maya second. Jump to the original message",
    });
    const items = screen.getAllByRole("listitem");
    expect(items[3]).toHaveTextContent("agreed");
    expect(items[3]?.querySelector("blockquote")).toBeNull();
    fireEvent.click(jump);
    expect(items[1]).toHaveClass("chat-message--highlight");
    expect(document.activeElement).toBe(items[1]);

    // The original is not loaded: no jump; the quoted HTML stays text.
    const eve = items[4] as HTMLElement;
    expect(within(eve).queryByRole("button", { name: /Jump/ })).toBeNull();
    expect(eve).toHaveTextContent("<img src=x onerror=alert(1)>");
    expect(eve.querySelector(".chat-reply img")).toBeNull();
  });
});
