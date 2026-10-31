import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Conversation, MockApi, MumcordApi, WatchParty } from "../../api";
import { axeViolations } from "../../test/axe";
import { getSelectedConversation } from "../channels";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetChatStore } from "../chat/chatStore";
import { resetRequestsStore } from "../requests/requestsStore";
import { getCentreState, resetCentre } from "./centreStore";
import { bindCentreSources } from "./centreSources";
import { NotificationBell, NotificationsHost, resetNotifications } from ".";

const LOBBY: Conversation = { kind: "channel", channelId: 1 };
const RAID: Conversation = { kind: "channel", channelId: 2 };
const ANNA = 2;
let api: MockApi;

async function setup() {
  resetNotifications();
  resetCentre();
  resetRequestsStore();
  api = await connectedMock();
  resetChatStore();
  const view = render(
    <>
      <NotificationBell />
      <main id="chat" tabIndex={-1} />
      <NotificationsHost overlayOpen={false} onOpenChat={vi.fn()} />
    </>,
  );
  await waitFor(() => {
    expect(getSelectedConversation()).toEqual(LOBBY);
  });
  return view;
}

function receive(conversation: Conversation, body: string, session = ANNA) {
  act(() => {
    api.mock.receiveMessage(conversation, body, session);
  });
}

function bell() {
  return screen.getByTestId("notification-bell");
}

function panel() {
  return screen.getByRole("dialog", { name: "Notifications" });
}

function openPanel() {
  fireEvent.click(bell());
  return panel();
}

afterEach(() => {
  resetNotifications();
  resetCentre();
  resetRequestsStore();
  resetAll();
});

describe("notification centre", () => {
  it("collects a mention with server and channel; clicking it opens the chat and reads it", async () => {
    const view = await setup();
    expect(bell()).toHaveAccessibleName("Notifications");
    receive(RAID, "hey @Zoe, pull at eight");
    await waitFor(() => {
      expect(bell()).toHaveAccessibleName("Notifications, 1 unread");
    });
    const centre = openPanel();
    const entry = within(centre).getByRole("button", { name: /^Anna mentioned you/ });
    expect(entry).toHaveTextContent("hey @Zoe, pull at eight");
    expect(entry).toHaveTextContent("#Raid");
    expect(entry).toHaveTextContent("Unread");
    expect(within(centre).getByTestId("centre-filter-mentions")).toHaveTextContent("Mentions, 1");
    expect(within(centre).getByText("Do Not Disturb is off")).toBeInTheDocument();
    expect(await axeViolations(view.container)).toEqual([]);
    fireEvent.click(entry);
    expect(getSelectedConversation()).toEqual(RAID);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(bell()).toHaveAccessibleName("Notifications");
  });

  it("counts a reply to the user under Mentions", async () => {
    await setup();
    receive(RAID, "<blockquote><b>Zoe</b>: lunch at noon?</blockquote>sure");
    const centre = openPanel();
    await within(centre).findByRole("button", { name: /^Anna replied to your message/ });
    expect(within(centre).getByTestId("centre-filter-mentions")).toHaveTextContent("1");
  });

  it("keeps counting during Do Not Disturb while no toast shows (Q1.5)", async () => {
    await setup();
    await act(async () => {
      await api.dndSet(true);
    });
    receive(RAID, "@Zoe raid now");
    await waitFor(() => {
      expect(bell()).toHaveAccessibleName("Notifications, 1 unread");
    });
    expect(
      within(screen.getByRole("region", { name: "Notifications" })).queryByRole("button"),
    ).toBeNull();
    const centre = openPanel();
    await within(centre).findByText("Do Not Disturb is on");
  });

  it("collects nothing for plain messages and reads entries of the chat on screen", async () => {
    await setup();
    receive(RAID, "just chatting");
    receive(LOBBY, "@Zoe here");
    await waitFor(() => {
      expect(getCentreState().items).toHaveLength(1);
    });
    expect(getCentreState().items[0]?.read).toBe(true);
    expect(bell()).toHaveAccessibleName("Notifications");
  });

  it("answers a chat request in place and marks everything read", async () => {
    await setup();
    const accept = vi.spyOn(api, "pmRequestAccept");
    act(() => {
      api.mock.receivePmRequest(ANNA, { chatRequest: true, note: "Loved your playlist" });
    });
    receive(RAID, "@Zoe hi");
    await waitFor(() => {
      expect(bell()).toHaveAccessibleName("Notifications, 2 unread");
    });
    const centre = openPanel();
    const request = within(centre)
      .getByRole("button", { name: /^Anna wants to chat with you/ })
      .closest("li");
    if (!request) throw new Error("no request entry");
    expect(request).toHaveTextContent("Loved your playlist");
    expect(request).toHaveTextContent("Chat request");
    fireEvent.click(within(centre).getByRole("button", { name: "Mark all as read" }));
    expect(bell()).toHaveAccessibleName("Notifications");
    fireEvent.click(within(request).getByRole("button", { name: "Accept" }));
    await waitFor(() => {
      expect(accept).toHaveBeenCalledTimes(1);
    });
  });

  it("shows a card sent privately as an invite that can be dismissed", async () => {
    await setup();
    const anna: Conversation = { kind: "privateSession", session: ANNA };
    receive(anna, '[card title="Raid tonight"]Bring flasks[/card]');
    const centre = openPanel();
    const entry = (
      await within(centre).findByRole("button", { name: /Anna sent you a card/ })
    ).closest("li");
    if (!entry) throw new Error("no invite entry");
    expect(entry).toHaveTextContent("Raid tonight");
    expect(within(centre).getByTestId("centre-filter-invites")).toHaveTextContent("1");
    fireEvent.click(within(entry).getByRole("button", { name: "Dismiss" }));
    expect(within(centre).queryByRole("button", { name: /sent you a card/ })).toBeNull();
  });

  it("opens with Ctrl+Shift+N, moves with the arrow keys and returns focus on Escape", async () => {
    await setup();
    receive(RAID, "@Zoe one");
    receive(RAID, "@Zoe two");
    await waitFor(() => {
      expect(getCentreState().items).toHaveLength(2);
    });
    fireEvent.keyDown(window, { key: "N", ctrlKey: true, shiftKey: true });
    const entries = within(panel()).getAllByTestId("centre-entry");
    expect(entries[0]).toHaveFocus();
    fireEvent.keyDown(entries[0] as HTMLElement, { key: "ArrowDown" });
    expect(entries[1]).toHaveFocus();
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      callback(0);
      return 0;
    });
    fireEvent.keyDown(entries[1] as HTMLElement, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(bell()).toHaveFocus();
    vi.restoreAllMocks();
  });
});

describe("notification centre sources", () => {
  it("turns a new watch party into an invite until it is joined", async () => {
    resetCentre();
    await connectedMock({ bindStore: true });
    const handlers = new Map<string, (payload: unknown) => void>();
    const fake = {
      on: (event: string, handler: (payload: unknown) => void) => {
        handlers.set(event, handler);
        return () => undefined;
      },
    } as unknown as MumcordApi;
    const stop = bindCentreSources(fake);
    const party = {
      channelId: 1,
      videoId: "x",
      playing: true,
      positionMs: 0,
      participants: [ANNA],
      lastAction: null,
      joined: false,
      currentEntry: 0,
      playlist: [],
      skippedBy: null,
    } as unknown as WatchParty;
    act(() => {
      handlers.get("watchPartyChanged")?.({ serverId: 1, party });
      handlers.get("watchPartyChanged")?.({ serverId: 1, party: { ...party, playing: false } });
    });
    expect(getCentreState().items.map((item) => item.id)).toEqual(["watchParty/1"]);
    act(() => {
      handlers.get("watchPartyChanged")?.({ serverId: 1, party: { ...party, joined: true } });
    });
    expect(getCentreState().items).toEqual([]);
    stop();
    resetAll();
  });
});
