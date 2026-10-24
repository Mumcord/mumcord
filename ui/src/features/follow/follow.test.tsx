import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { ChannelsPanel, selectConversation, getSelectedConversation } from "../channels";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { resetServerStore } from "../channels/serverStore";
import { ChatView } from "../chat";
import { resetChatStore } from "../chat/chatStore";
import { privateConversation } from "../members";
import { ChannelUnreadBadge, DirectMessages, FollowHeaderActions, resetFollowStore } from ".";

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
        <DirectMessages />
      </nav>
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
    await old.connect(1, null);
    render(<ChatView renderHeaderActions={(c) => <FollowHeaderActions conversation={c} />} />);
    expect(await screen.findByText(/needs a server running version 1.4/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Follow/ })).toBeNull();
  });

  it("shows unread badges for followed channels and marks read when viewed", async () => {
    await setup();
    await act(() => api.channelFollow(2));
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

  it("lists private chats and clears a conversation after confirmation", async () => {
    await setup();
    const users = (await api.sessionSnapshot())?.users ?? [];
    const anna = users.find((u) => u.name === "Anna");
    if (!anna) throw new Error("seed user missing");
    act(() => {
      api.mock.receiveMessage(privateConversation(anna), "psst", anna.session);
    });
    const entry = await screen.findByRole("button", { name: /Anna/ });
    expect(entry).toHaveTextContent(/1/);
    fireEvent.click(entry);
    expect(getSelectedConversation()).toEqual(privateConversation(anna));
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
