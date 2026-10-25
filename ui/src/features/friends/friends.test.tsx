import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { MockApi } from "../../api";
import { getSelectedConversation } from "../channels/selection";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { FriendsList } from "./FriendsList";
import { resetFriendsStore } from "./friendsStore";
import { PrivatePresence } from "./PrivatePresence";

let api: MockApi;

async function connected() {
  api = await connectedMock();
  await waitFor(() => {
    expect(getServerState().users.has(2)).toBe(true);
  });
}

afterEach(() => {
  resetFriendsStore();
  resetAll();
});

describe("FriendsList", () => {
  it("is hidden without friends", async () => {
    await connected();
    const { container } = render(<FriendsList />);
    expect(container).toBeEmptyDOMElement();
  });

  it("lists online friends first with their channel; a click opens the private chat", async () => {
    await connected();
    render(<FriendsList />);
    await act(async () => {
      await api.friendAdd(4);
      await api.friendAdd(2);
    });
    const list = screen.getByRole("region", { name: "Friends" });
    const items = within(list).getAllByRole("button", { name: /,/ });
    expect(items.map((item) => item.getAttribute("aria-label"))).toEqual([
      "Anna, Online in Lobby",
      "Carla, Online in Raid",
    ]);
    const carla = items[1];
    if (!carla) throw new Error("Carla is missing");
    fireEvent.click(carla);
    expect(getSelectedConversation()).toEqual({ kind: "private", certHash: "carlahash" });
  });

  it("shows a friend who leaves as offline with last seen, and keeps them after a rename", async () => {
    await connected();
    render(<FriendsList />);
    await act(async () => {
      await api.friendAdd(2);
    });
    const anna = getServerState().users.get(2);
    if (!anna) throw new Error("Anna is missing");
    act(() => {
      api.mock.removeUser(2);
    });
    const list = screen.getByRole("region", { name: "Friends" });
    expect(within(list).getByRole("button", { name: /,/ })).toHaveAccessibleName(
      /^Anna, Offline, last seen /,
    );

    act(() => {
      api.mock.upsertUser({ ...anna, session: 12, name: "Annabel" });
    });
    expect(within(list).getAllByRole("button", { name: /,/ })).toHaveLength(1);
    expect(within(list).getByRole("button", { name: /,/ })).toHaveAccessibleName(
      /^Annabel, Online/,
    );
  });

  it("removing a friend empties the list", async () => {
    await connected();
    render(<FriendsList />);
    await act(async () => {
      await api.friendAdd(2);
    });
    expect(screen.getByRole("region", { name: "Friends" })).toBeInTheDocument();
    await act(async () => {
      await api.friendRemove("annahash");
    });
    expect(screen.queryByRole("region", { name: "Friends" })).toBeNull();
  });

  it("refuses people without a certificate", async () => {
    await connected();
    await expect(api.friendAdd(3)).rejects.toMatchObject({ code: "invalidInput" });
  });
});

describe("PrivatePresence", () => {
  it("says online or offline, and when a friend was last seen", async () => {
    await connected();
    await api.friendAdd(2);
    const { rerender } = render(<PrivatePresence online certHash="annahash" />);
    expect(screen.getByText("Online")).toBeInTheDocument();
    rerender(<PrivatePresence online={false} certHash="someone" />);
    expect(screen.getByText("Offline")).toBeInTheDocument();
    act(() => {
      api.mock.removeUser(2);
    });
    rerender(<PrivatePresence online={false} certHash="annahash" />);
    expect(screen.getByText(/^Offline, last seen /)).toBeInTheDocument();
  });
});
