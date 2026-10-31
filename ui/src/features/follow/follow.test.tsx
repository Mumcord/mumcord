import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { ChannelsPanel, selectConversation } from "../channels";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { resetServerStore } from "../channels/serverStore";
import { ChatView } from "../chat";
import { resetChatStore } from "../chat/chatStore";
import { privateConversation, ProfilePreviewHost } from "../members";
import { ChannelUnreadBadge, FollowHeaderActions, MessageNotifications, resetFollowStore } from ".";

const RAID = { kind: "channel", channelId: 2 } as const;
let api: MockApi;

async function setup() {
  resetFollowStore();
  api = await connectedMock();
  resetChatStore();
  stubDialog();
  const view = render(
    <>
      <nav>
        <ChannelsPanel channelBadge={(id) => <ChannelUnreadBadge channelId={id} />} />
        <MessageNotifications />
      </nav>
      <ProfilePreviewHost />
      <main>
        <ChatView renderHeaderActions={(c) => <FollowHeaderActions conversation={c} />} />
      </main>
    </>,
  );
  await screen.findByRole("tree", { name: "Channels" });
  await screen.findByRole("heading", { level: 2, name: /Lobby/ });
  return view;
}

beforeEach(() => {
  resetFollowStore();
});
afterEach(() => {
  resetFollowStore();
  resetAll();
});

describe("follow", () => {
  it("toggles following and has no axe violations", async () => {
    const view = await setup();
    act(() => {
      selectConversation(RAID);
    });
    const button = await screen.findByRole("button", { name: /Follow/ });
    expect(button).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(button);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Following/ })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
    expect(await axeViolations(view.container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: /Following/ }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Follow$/ })).toHaveAttribute(
        "aria-pressed",
        "false",
      ),
    );
  });

  it("explains permission and limit errors", async () => {
    await setup();
    act(() => {
      selectConversation(RAID);
    });
    const button = await screen.findByRole("button", { name: /Follow/ });
    api.mock.failNext("channelFollow", "permissionDenied");
    fireEvent.click(button);
    expect(await screen.findByRole("alert")).toHaveTextContent(/doesn't allow you/);
    api.mock.failNext("channelFollow", "listenerLimitReached");
    fireEvent.click(screen.getByRole("button", { name: /Follow/ }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(/more channels/));
  });

  it("hides the toggle with a hint on servers before 1.4", async () => {
    await connectedMock();
    resetFollowStore();
    resetServerStore();
    resetChatStore();
    const old = createMockApi({ serverVersion: "1.3.4" });
    setApi(old);
    await old.connect(1, null, null);
    render(<ChatView renderHeaderActions={(c) => <FollowHeaderActions conversation={c} />} />);
    expect(await screen.findByText(/needs a server running version 1.4/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Follow/ })).toBeNull();
  });

  it("shows unread badges for followed channels and marks read when viewed", async () => {
    await setup();
    await act(() => api.channelFollow(1, 2));
    act(() => {
      api.mock.receiveMessage(RAID, "hello", 2);
    });
    expect(await screen.findByText(/1 unread message/)).toBeInTheDocument();
    act(() => {
      selectConversation(RAID);
    });
    await waitFor(() => {
      expect(screen.queryByText(/unread message/)).toBeNull();
    });
  });

  it("counts unread in the own channel while another chat is open and not for own messages", async () => {
    await setup();
    const LOBBY = { kind: "channel", channelId: 1 } as const;
    act(() => {
      selectConversation(RAID);
    });
    act(() => {
      api.mock.receiveMessage(LOBBY, "psst", 2);
    });
    expect(await screen.findByText(/1 unread message/)).toBeInTheDocument();
    act(() => {
      selectConversation(LOBBY);
    });
    await waitFor(() => {
      expect(screen.queryByText(/unread message/)).toBeNull();
    });
    act(() => {
      selectConversation(RAID);
    });
    await act(() => api.sendChannelMessage(1, 1, "mine"));
    expect(screen.queryByText(/unread message/)).toBeNull();
  });

  it("never renders a badge for a message in the chat that is on screen", async () => {
    await setup();
    const LOBBY = { kind: "channel", channelId: 1 } as const;
    let seen = false;
    const observer = new MutationObserver((records) => {
      for (const record of records) {
        for (const node of record.addedNodes) {
          if (node instanceof Element && node.matches(".unread-badge, :has(.unread-badge)")) {
            seen = true;
          }
        }
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
    act(() => {
      api.mock.receiveMessage(LOBBY, "psst", 2);
    });
    await screen.findByText("psst");
    await new Promise((resolve) => setTimeout(resolve, 20));
    observer.disconnect();
    expect(seen).toBe(false);
    expect(document.querySelector(".unread-badge")).toBeNull();
  });

  // The count is CSS-generated, so the badge adds no visible DOM text beyond its accessible name.
  it("keeps the unread count out of DOM text but inside the treeitem's name", async () => {
    await setup();
    await act(() => api.channelFollow(1, 2));
    act(() => {
      api.mock.receiveMessage(RAID, "hello", 2);
    });
    await screen.findByText(/1 unread message/);
    const badge = document.querySelector<HTMLElement>(".unread-badge");
    expect(badge?.getAttribute("data-count")).toBe("1");
    expect(badge?.textContent).toBe(", 1 unread message");
    const raid = screen.getByRole("treeitem", { name: /^Raid/ });
    expect(raid).toHaveAccessibleName(/1 unread message/);
    const clone = badge?.cloneNode(true) as HTMLElement;
    clone.querySelectorAll(".visually-hidden").forEach((el) => {
      el.remove();
    });
    expect(clone.textContent).toBe("");
    expect(await axeViolations(raid.parentElement ?? document.body)).toEqual([]);
  });

  it("clears a private conversation after confirmation", async () => {
    await setup();
    const users = (await api.sessionSnapshot(1))?.users ?? [];
    const anna = users.find((u) => u.name === "Anna");
    if (!anna) throw new Error("seed user missing");
    act(() => {
      api.mock.receiveMessage(privateConversation(anna), "psst", anna.session);
    });
    act(() => {
      selectConversation(privateConversation(anna));
    });
    await screen.findByText("psst");
    fireEvent.click(screen.getByRole("button", { name: "Clear history" }));
    const dialog = screen.getByRole("alertdialog");
    expect(await axeViolations(dialog)).toEqual([]);
    const confirmButton = screen.getAllByRole("button", { name: "Clear history" }).at(-1);
    if (!confirmButton) throw new Error("no confirm button");
    fireEvent.click(confirmButton);
    await waitFor(() => {
      expect(screen.queryByText("psst")).toBeNull();
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});
