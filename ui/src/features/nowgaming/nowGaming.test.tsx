import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, NowGaming } from "../../api";
import { ChannelsPanel } from "../channels/ChannelsPanel";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { ProfileCard } from "../members/ProfileCard";
import { resetNowGamingStore } from "./nowGamingStore";
import { playedAt, playedParts } from "./playTime";

const ANNA = 2;

const factorio: NowGaming = {
  name: "Factorio",
  hasIcon: true,
  playedMs: 80 * 60_000,
  canJoin: false,
};

let api: MockApi;

beforeEach(stubDialog);
afterEach(() => {
  vi.useRealTimers();
  resetNowGamingStore();
  resetAll();
});

function share(game: NowGaming | null, session = ANNA) {
  act(() => {
    api.mock.emit("nowGamingChanged", { session, game });
  });
}

describe("play time", () => {
  it("moves on from the arrival time and splits into hours and minutes", () => {
    expect(playedAt({ game: factorio, atMs: 1000 }, 61_000)).toBe(81 * 60_000);
    expect(playedAt({ game: factorio, atMs: 5000 }, 1000)).toBe(80 * 60_000);
    expect(playedParts(80 * 60_000)).toEqual({ hours: 1, minutes: 20 });
    expect(playedParts(59_999)).toEqual({ hours: 0, minutes: 0 });
  });
});

describe("controller icon", () => {
  const treeRow = () => screen.getByRole("treeitem", { name: /^Anna/ });

  beforeEach(async () => {
    api = await connectedMock();
    render(<ChannelsPanel />);
    await screen.findByRole("tree", { name: "Channels" });
  });

  it("shows while the person plays, with the game as tooltip", () => {
    expect(treeRow().querySelector('[data-icon="gamepad"]')).toBeNull();
    share(factorio);
    const icon = within(treeRow()).getByRole("img", { name: "Playing Factorio" });
    expect(icon).toHaveAttribute("title", "Playing Factorio");
    share(null);
    expect(treeRow().querySelector('[data-icon="gamepad"]')).toBeNull();
  });

  it("goes away when the server resyncs", async () => {
    share(factorio);
    const snapshot = await api.sessionSnapshot();
    if (!snapshot) throw new Error("not connected");
    act(() => {
      api.mock.emit("serverSynced", { snapshot });
    });
    expect(treeRow().querySelector('[data-icon="gamepad"]')).toBeNull();
  });
});

async function self(): Promise<{ session: number; name: string }> {
  const snapshot = await api.sessionSnapshot();
  const own = snapshot?.users.find((u) => u.session === snapshot.selfSession);
  if (!own) throw new Error("not connected");
  return { session: own.session, name: own.name };
}

async function renderCard(session = ANNA, name = "Anna") {
  await waitFor(() => {
    expect(getServerState().serverId).toBe(1);
  });
  render(<ProfileCard session={session} onClose={() => undefined} />);
  await screen.findByRole("dialog", { name });
}

describe("profile block", () => {
  it("shows the game icon, name and play time", async () => {
    api = await connectedMock();
    const icon = vi.spyOn(api, "nowGamingIcon").mockResolvedValue("data:image/jpeg;base64,AAAA");
    await renderCard();
    share(factorio);
    const block = await screen.findByRole("region", { name: "Now gaming" });
    expect(within(block).getByText("Factorio")).toBeInTheDocument();
    expect(within(block).getByText("for 1 h 20 min")).toBeInTheDocument();
    expect(icon).toHaveBeenCalledWith(ANNA);
    await vi.waitFor(() => {
      expect(block.querySelector("img")).toHaveAttribute("src", "data:image/jpeg;base64,AAAA");
    });
    expect(within(block).queryByRole("button", { name: "Join game" })).toBeNull();
    share(null);
    expect(screen.queryByRole("region", { name: "Now gaming" })).toBeNull();
  });

  it("falls back to a generic controller without an icon", async () => {
    api = await connectedMock();
    const icon = vi.spyOn(api, "nowGamingIcon");
    await renderCard();
    share({ ...factorio, hasIcon: false, playedMs: 5 * 60_000 });
    const block = await screen.findByRole("region", { name: "Now gaming" });
    expect(block.querySelector("img")).toBeNull();
    expect(block.querySelector('[data-icon="gamepad"]')).not.toBeNull();
    expect(within(block).getByText("for 5 min")).toBeInTheDocument();
    expect(icon).not.toHaveBeenCalled();
  });

  it("offers Join game only with a join target and opens it after confirming", async () => {
    api = await connectedMock();
    const join = vi.spyOn(api, "nowGamingJoin");
    await renderCard();
    share({ ...factorio, canJoin: true });
    const block = await screen.findByRole("region", { name: "Now gaming" });
    fireEvent.click(within(block).getByRole("button", { name: "Join game" }));
    const confirm = await screen.findByRole("dialog", { name: "Join game?" });
    expect(within(confirm).getByText(/plays Factorio on/)).toBeInTheDocument();
    expect(join).not.toHaveBeenCalled();
    fireEvent.click(within(confirm).getByRole("button", { name: "Open Steam" }));
    expect(join).toHaveBeenCalledWith(ANNA);
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Join game?" })).toBeNull();
    });
  });

  it("never offers to join the own game", async () => {
    api = await connectedMock();
    const own = await self();
    await renderCard(own.session, own.name);
    share({ ...factorio, canJoin: true }, own.session);
    const block = await screen.findByRole("region", { name: "Now gaming" });
    expect(within(block).getByText("Factorio")).toBeInTheDocument();
    expect(within(block).queryByRole("button", { name: "Join game" })).toBeNull();
  });
});
