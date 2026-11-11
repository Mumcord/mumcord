import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { ChannelMentionMenu } from "./ChannelMentionMenu";

let api: MockApi;

afterEach(() => {
  cleanup();
  resetAll();
});

async function renderMenu(ids: number[]) {
  api = await connectedMock({ bindStore: true });
  await act(() => Promise.resolve());
  const onClose = vi.fn();
  render(
    <ChannelMentionMenu channelIds={ids} menu={{ x: 0, y: 0, opener: null }} onClose={onClose} />,
  );
  return onClose;
}

const self = () => {
  const state = getServerState();
  return state.selfSession === null ? undefined : state.users.get(state.selfSession);
};

describe("channel mention menu (#643)", () => {
  it("joins the channel like the tree does", async () => {
    await renderMenu([2]);
    expect(screen.getByRole("menu", { name: "Channel Raid" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Join channel" }));
    await waitFor(() => {
      expect(self()?.channelId).toBe(2);
    });
  });

  it("offers no Join for the channel you are in, but Show and Copy", async () => {
    await renderMenu([1]);
    expect(screen.queryByRole("menuitem", { name: "Join channel" })).not.toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Show in channel list" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Copy name" })).toBeInTheDocument();
  });

  it("lists channels sharing a name with their path", async () => {
    api = await connectedMock({ bindStore: true });
    // Sibling names are unique in Mumble, so the second "Raid" sits under Lobby.
    act(() => {
      api.mock.updateChannel(3, { name: "Raid", parentId: 1 });
    });
    await waitFor(() => {
      expect(getServerState().channels.get(3)?.parentId).toBe(1);
    });
    render(
      <ChannelMentionMenu
        channelIds={[2, 3]}
        menu={{ x: 0, y: 0, opener: null }}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByRole("menuitem", { name: "Join Raid" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Join Lobby › Raid" })).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "Show Lobby › Raid in channel list" }),
    ).toBeInTheDocument();
  });
});
