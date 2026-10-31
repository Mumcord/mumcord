import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MockApi } from "../../api";
import { getSelectedConversation } from "../channels";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { resetFollowStore } from "../follow";
import { resetFriendsStore } from "../friends";
import { resetRequestsStore } from "../requests";
import { resetConversationsStore } from "./conversationsStore";
import { MessagesButton } from "./MessagesButton";
import { MessagesOverview } from "./MessagesOverview";

const ANNA = { kind: "private", certHash: "annahash" } as const;
const CARLA = { kind: "private", certHash: "carlahash" } as const;

let api: MockApi;

beforeEach(async () => {
  stubDialog();
  api = await connectedMock({ bindStore: true });
  await waitFor(() => {
    expect(getServerState().users.has(2)).toBe(true);
  });
});

afterEach(() => {
  resetConversationsStore();
  resetRequestsStore();
  resetFollowStore();
  resetFriendsStore();
  resetAll();
});

function rows() {
  return screen.queryAllByTestId("pm-row");
}

function row(index: number): HTMLElement {
  const found = rows()[index];
  if (!found) throw new Error(`no row ${String(index)}`);
  return found;
}

function openMenu(row: HTMLElement) {
  fireEvent.keyDown(row, { key: "F10", shiftKey: true });
  return screen.getByRole("menu");
}

describe("private messages overview", () => {
  it("shows the calm empty state with the start button", () => {
    render(<MessagesOverview />);
    expect(screen.getByText("No conversations yet")).toBeInTheDocument();
    const [, start] = screen.getAllByRole("button", { name: "Start a new conversation" });
    if (!start) throw new Error("no start button");
    fireEvent.click(start);
    expect(screen.getByRole("dialog", { name: "Start a new conversation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Anna" })).toBeInTheDocument();
  });

  it("lists conversations newest first with server, preview and unread count, and opens them", async () => {
    render(<MessagesOverview />);
    act(() => {
      api.mock.receiveMessage(CARLA, "<b>first</b>", 4);
      api.mock.receiveMessage(ANNA, "Booked the <i>hall</i>", 2);
    });
    await waitFor(() => {
      expect(rows()).toHaveLength(2);
    });
    expect(row(0)).toHaveAccessibleName(/^Anna, .*, online, 1 unread, Booked the hall, /);
    expect(row(1)).toHaveAccessibleName(/^Carla, /);
    // Server HTML only ever shows as plain text.
    expect(row(0).querySelector("i, b")).toBeNull();
    fireEvent.keyDown(row(0), { key: "ArrowDown" });
    expect(row(1)).toHaveFocus();
    fireEvent.click(row(0));
    expect(getSelectedConversation()).toEqual(ANNA);
  });

  it("pins, mutes and marks as read from the conversation menu", async () => {
    render(
      <>
        <MessagesButton />
        <MessagesOverview />
      </>,
    );
    act(() => {
      api.mock.receiveMessage(ANNA, "one", 2);
      api.mock.receiveMessage(CARLA, "two", 4);
    });
    await waitFor(() => {
      expect(rows()).toHaveLength(2);
    });
    expect(screen.getByTestId("pm-button")).toHaveAccessibleName("Messages, 2 unread");

    fireEvent.click(within(openMenu(row(1))).getByRole("menuitem", { name: "Pin to top" }));
    await waitFor(() => {
      expect(screen.getByRole("region", { name: "Pinned" })).toHaveTextContent("Anna");
    });
    expect(rows()[0]).toHaveAccessibleName(/pinned/);

    fireEvent.click(within(openMenu(row(0))).getByRole("menuitem", { name: "Mute conversation" }));
    fireEvent.click(
      within(screen.getByRole("menu")).getByRole("menuitem", { name: "For 8 hours" }),
    );
    await waitFor(() => {
      expect(rows()[0]).toHaveAccessibleName(/muted/);
    });
    // Muted unread messages no longer count on the button.
    expect(screen.getByTestId("pm-button")).toHaveAccessibleName("Messages, 1 unread");

    fireEvent.click(within(openMenu(row(1))).getByRole("menuitem", { name: "Mark as read" }));
    await waitFor(() => {
      expect(screen.getByTestId("pm-button")).toHaveAccessibleName("Messages");
    });
    expect(within(openMenu(row(0))).getByRole("menuitem", { name: /Block Anna/ })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("filters unread conversations and searches", async () => {
    render(<MessagesOverview />);
    act(() => {
      api.mock.receiveMessage(ANNA, "party on Saturday", 2);
      api.mock.receiveMessage(CARLA, "hello", 4);
    });
    await waitFor(() => {
      expect(rows()).toHaveLength(2);
    });
    await act(() => api.conversationMarkRead(1, CARLA));
    fireEvent.click(screen.getByTestId("pm-filter-unread"));
    expect(rows()).toHaveLength(1);
    fireEvent.click(screen.getByTestId("pm-filter-all"));
    fireEvent.change(screen.getByRole("searchbox", { name: "Search conversations" }), {
      target: { value: "SATURDAY" },
    });
    expect(rows()).toHaveLength(1);
    expect(rows()[0]).toHaveAccessibleName(/^Anna/);
  });

  it("collapses more than one message request", async () => {
    render(<MessagesOverview />);
    act(() => {
      api.mock.receivePmRequest(4, { chatRequest: true, note: null });
      api.mock.receivePmRequest(2, { chatRequest: true, note: "<b>Hi!</b> cup?" });
    });
    expect(await screen.findByRole("button", { name: "Show 1 more request" })).toBeInTheDocument();
    expect(screen.getAllByTestId("pm-request")).toHaveLength(1);
    // The note is untrusted plain text.
    expect(screen.getByTestId("request-note")).toHaveTextContent("“<b>Hi!</b> cup?”");
    fireEvent.click(screen.getByRole("button", { name: "Show 1 more request" }));
    expect(screen.getAllByTestId("pm-request")).toHaveLength(2);
    fireEvent.click(screen.getByTestId("pm-filter-requests"));
    expect(screen.getAllByTestId("pm-request")).toHaveLength(2);
  });

  it("deletes the history on this device after a confirmation", async () => {
    render(<MessagesOverview />);
    act(() => {
      api.mock.receiveMessage(ANNA, "secret", 2);
    });
    await waitFor(() => {
      expect(rows()).toHaveLength(1);
    });
    fireEvent.click(within(openMenu(row(0))).getByRole("menuitem", { name: "Delete history" }));
    const dialog = screen.getByRole("dialog", { name: "Delete history with Anna?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete history" }));
    await waitFor(() => {
      expect(rows()).toHaveLength(0);
    });
    expect(await api.historyLoad(1, ANNA, null, 10)).toEqual([]);
  });
});
