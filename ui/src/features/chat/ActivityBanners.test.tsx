import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare, WatchParty } from "../../api";
import { getServerState } from "../channels";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetScreenShareStore, resetWatchingStore, useWatchedShare } from "../screenshare";
import { resetWatchPartyStore } from "../watchparty";
import { ActivityBanners } from "./ActivityBanners";

const SERVER = 1;

function share(session: number): ScreenShare {
  return {
    session,
    paused: false,
    sound: false,
    soundAvailable: false,
    viewers: [],
    blocked: null,
  };
}

function party(patch: Partial<WatchParty> = {}): WatchParty {
  return {
    channelId: 0,
    videoId: "dQw4w9WgXcQ",
    playing: true,
    positionMs: 0,
    participants: [2],
    lastAction: null,
    joined: false,
    currentEntry: 0,
    playlist: [],
    skippedBy: null,
    ...patch,
  };
}

function Watched() {
  const watched = useWatchedShare();
  return <p data-testid="watched">{watched?.session ?? "none"}</p>;
}

async function setup(): Promise<{ api: MockApi; channelId: number }> {
  const api = await connectedMock({ bindStore: true });
  const state = getServerState();
  const self = state.users.get(state.selfSession ?? -1);
  const channelId = self?.channelId ?? 0;
  render(
    <>
      <ActivityBanners channelId={channelId} />
      <Watched />
    </>,
  );
  return { api, channelId };
}

function emitShares(api: MockApi, sessions: number[]): void {
  act(() => {
    api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: sessions.map(share) });
  });
}

function emitParty(api: MockApi, value: WatchParty | null, channelId: number): void {
  act(() => {
    api.mock.emit("watchPartyChanged", {
      serverId: SERVER,
      party: value && { ...value, channelId },
    });
  });
}

afterEach(() => {
  resetWatchingStore();
  resetScreenShareStore();
  resetWatchPartyStore();
  resetAll();
});

describe("activity banners", () => {
  it("shows a banner per share and hides it when the share ends", async () => {
    const { api } = await setup();
    const anna = getServerState().users.get(2)?.name ?? "";
    expect(screen.queryByRole("status")).toBeNull();
    emitShares(api, [2]);
    expect(screen.getByRole("status")).toHaveTextContent(`${anna} is sharing their screen`);
    emitShares(api, []);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("Watch opens the share and hides the banner while watching", async () => {
    const { api } = await setup();
    emitShares(api, [2]);
    fireEvent.click(screen.getByRole("button", { name: "Watch" }));
    expect(screen.getByTestId("watched")).toHaveTextContent("2");
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows a party banner, Join joins, and it returns after leaving", async () => {
    const { api, channelId } = await setup();
    const join = vi
      .spyOn(api, "watchPartyJoin")
      .mockResolvedValue(party({ channelId, joined: true }));
    emitParty(api, party(), channelId);
    expect(screen.getByRole("status")).toHaveTextContent("Watch party:");
    fireEvent.click(screen.getByRole("button", { name: "Join" }));
    expect(join).toHaveBeenCalledWith(SERVER);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.queryByRole("status")).toBeNull();
    emitParty(api, party({ joined: false }), channelId);
    expect(screen.getByRole("status")).toBeInTheDocument();
    emitParty(api, null, channelId);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows at most three banners and posts no chat message", async () => {
    const { api, channelId } = await setup();
    const channelSend = vi.spyOn(api, "sendChannelMessage");
    const privateSend = vi.spyOn(api, "sendPrivateMessage");
    emitParty(api, party(), channelId);
    emitShares(api, [2, 3, 4, 5]);
    expect(screen.getAllByRole("status")).toHaveLength(3);
    expect(channelSend).not.toHaveBeenCalled();
    expect(privateSend).not.toHaveBeenCalled();
  });
});
