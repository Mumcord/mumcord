import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { ChannelsPanel, getSelectedConversation, getServerState, selfUser } from "../channels";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetServerStore } from "../channels/serverStore";
import {
  ChannelUnreadBadge,
  FollowAllMenu,
  FollowNoticeLine,
  FollowStar,
  resetFollowStore,
} from ".";
import { setFollowNotice } from "./notice";
import { describeSummary } from "./summary";
import { i18n } from "../../i18n";

let api: MockApi;

function renderPanel() {
  return render(
    <nav>
      <ChannelsPanel
        header={<FollowAllMenu />}
        channelBadge={(id) => <ChannelUnreadBadge channelId={id} />}
        channelAction={(channel) => <FollowStar {...channel} />}
      />
      <FollowNoticeLine />
    </nav>,
  );
}

async function setup() {
  api = await connectedMock();
  const view = renderPanel();
  await screen.findByRole("tree", { name: "Channels" });
  return view;
}

function raidRow(): HTMLElement {
  return screen.getByRole("treeitem", { name: /^Raid/ });
}

beforeEach(() => {
  resetFollowStore();
  setFollowNotice(null);
});
afterEach(() => {
  resetFollowStore();
  setFollowNotice(null);
  resetAll();
});

describe("follow eye", () => {
  it("follows and unfollows without selecting or joining the channel", async () => {
    const view = await setup();
    const selectedBefore = getSelectedConversation();
    const eye = within(raidRow()).getByRole("button", { name: "Follow Raid" });
    expect(eye).toHaveAttribute("data-followed", "false");
    // The eye's slot ends the row, so the hover-only Join button cannot push it aside.
    expect(eye.parentElement?.nextElementSibling).toBeNull();
    const ownChannel = selfUser(getServerState())?.channelId;
    fireEvent.click(eye);
    fireEvent.doubleClick(eye);
    const filled = await within(raidRow()).findByRole("button", { name: "Unfollow Raid" });
    expect(filled).toHaveAttribute("data-followed", "true");
    expect(getSelectedConversation()).toEqual(selectedBefore);
    expect(selfUser(getServerState())?.channelId).toBe(ownChannel);
    expect(ownChannel).not.toBe(2);
    fireEvent.click(filled);
    expect(
      await within(raidRow()).findByRole("button", { name: "Follow Raid" }),
    ).toBeInTheDocument();
    expect(await axeViolations(view.container)).toEqual([]);
  });

  it("is reachable with Tab from the focused row and toggles with Enter without selecting", async () => {
    await setup();
    const row = raidRow();
    act(() => {
      row.focus();
      fireEvent.focus(row);
    });
    const eye = within(row).getByRole("button", { name: "Follow Raid" });
    await waitFor(() => {
      expect(eye).toHaveAttribute("tabindex", "0");
    });
    const selectedBefore = getSelectedConversation();
    eye.focus();
    fireEvent.keyDown(eye, { key: "Enter" });
    expect(getSelectedConversation()).toEqual(selectedBefore);
    fireEvent.click(eye);
    expect(await within(row).findByRole("button", { name: "Unfollow Raid" })).toBeInTheDocument();
    // Arrow keys still move through the tree from the eye.
    fireEvent.keyDown(eye, { key: "ArrowUp" });
    await waitFor(() => {
      expect(document.activeElement).not.toBe(eye);
    });
  });

  it("shows the limit error below the tree and keeps the unread badge", async () => {
    await setup();
    await act(() => api.channelFollow(2));
    act(() => {
      api.mock.receiveMessage({ kind: "channel", channelId: 2 }, "hello", 2);
    });
    expect(await within(raidRow()).findByText(/1 unread message/)).toBeInTheDocument();
    expect(within(raidRow()).getByRole("button", { name: "Unfollow Raid" })).toBeInTheDocument();
    api.mock.failNext("channelFollow", "listenerLimitReached");
    fireEvent.click(screen.getByRole("button", { name: "Follow AFK" }));
    expect(await screen.findByRole("status")).toHaveTextContent(/more channels/);
  });

  it("is hidden on servers before 1.4", async () => {
    await connectedMock();
    resetFollowStore();
    resetServerStore();
    const old = createMockApi({ serverVersion: "1.3.4" });
    setApi(old);
    await old.connect(1, null);
    renderPanel();
    await screen.findByRole("tree", { name: "Channels" });
    expect(screen.queryByRole("button", { name: /Follow/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Server options/ })).toBeNull();
  });
});

describe("follow all", () => {
  it("follows up to the listener limit, reports the rest and unfollows all", async () => {
    await setup();
    const menuButton = screen.getByRole("button", { name: /Server options/ });
    fireEvent.click(menuButton);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Follow all channels" }));
    // Mock: limit 2; Root, Raid and AFK are followable, Lobby is the own voice channel.
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "Followed 2 channels. 1 skipped: limit reached.",
      ),
    );
    expect(screen.getAllByRole("button", { name: /^Unfollow/ })).toHaveLength(2);
    expect(screen.queryByRole("button", { name: "Unfollow Lobby" })).toBeNull();

    fireEvent.contextMenu(menuButton);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Unfollow all channels" }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Unfollowed all channels."),
    );
    expect(screen.queryAllByRole("button", { name: /^Unfollow/ })).toHaveLength(0);
  });

  it("closes the menu with Escape and returns focus", async () => {
    await setup();
    const menuButton = screen.getByRole("button", { name: /Server options/ });
    fireEvent.click(menuButton);
    const menu = await screen.findByRole("menu");
    fireEvent.keyDown(menu, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("menu")).toBeNull();
    });
    expect(document.activeElement).toBe(menuButton);
  });
});

describe("follow all summary", () => {
  it("lists every skip reason", () => {
    const t = i18n.getFixedT("en", "follow");
    expect(describeSummary(t, { followed: 42, limitReached: 5, notAllowed: 3, noAnswer: 0 })).toBe(
      "Followed 42 channels. 8 skipped: limit reached, not allowed.",
    );
    expect(describeSummary(t, { followed: 1, limitReached: 0, notAllowed: 0, noAnswer: 0 })).toBe(
      "Followed 1 channel.",
    );
  });
});
