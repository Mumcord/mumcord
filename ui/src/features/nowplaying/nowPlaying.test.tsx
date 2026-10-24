import { act, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, NowPlaying } from "../../api";
import { ChannelsPanel } from "../channels/ChannelsPanel";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { ProfileCard } from "../members/ProfileCard";
import { resetNowPlayingStore } from "./nowPlayingStore";
import { formatTime, positionAt, trackLabel } from "./progress";

const ANNA = 2;

const song: NowPlaying = {
  title: "Song",
  artist: "Band",
  album: "Record",
  hasCover: true,
  positionMs: 60_000,
  durationMs: 200_000,
  playing: true,
};

let api: MockApi;

beforeEach(stubDialog);
afterEach(() => {
  vi.useRealTimers();
  resetNowPlayingStore();
  resetAll();
});

function share(track: NowPlaying | null, session = ANNA) {
  act(() => {
    api.mock.emit("nowPlayingChanged", { session, track });
  });
}

describe("progress", () => {
  it("advances from the arrival time while playing and stops at the end", () => {
    expect(positionAt({ track: song, atMs: 1000 }, 3500)).toBe(62_500);
    expect(positionAt({ track: { ...song, playing: false }, atMs: 1000 }, 3500)).toBe(60_000);
    expect(positionAt({ track: song, atMs: 0 }, 1_000_000)).toBe(200_000);
    expect(positionAt({ track: { ...song, durationMs: null }, atMs: 0 }, 1000)).toBe(61_000);
    expect(positionAt({ track: song, atMs: 5000 }, 1000)).toBe(60_000);
  });

  it("formats times and labels", () => {
    expect(formatTime(65_000)).toBe("1:05");
    expect(formatTime(3_725_000)).toBe("1:02:05");
    expect(trackLabel(song)).toBe("Band – Song");
    expect(trackLabel({ ...song, artist: "" })).toBe("Song");
  });
});

describe("music icon", () => {
  const treeRow = () => screen.getByRole("treeitem", { name: /^Anna/ });

  beforeEach(async () => {
    api = await connectedMock();
    render(<ChannelsPanel />);
    await screen.findByRole("tree", { name: "Channels" });
  });

  it("shows while the person plays a track, with the track as tooltip", () => {
    expect(treeRow().querySelector('[data-icon="music"]')).toBeNull();
    share(song);
    const icon = within(treeRow()).getByRole("img", { name: "Listening to Band – Song" });
    expect(icon).toHaveAttribute("title", "Listening to Band – Song");
    share({ ...song, playing: false });
    expect(treeRow().querySelector('[data-icon="music"]')).toBeNull();
  });

  it("shows on the own row too, from the own track", async () => {
    const own = await self();
    const ownRow = screen.getByRole("treeitem", { name: new RegExp(`^${own.name}`) });
    expect(ownRow.querySelector('[data-icon="music"]')).toBeNull();
    share(song, own.session);
    const icon = within(ownRow).getByRole("img", { name: "Listening to Band – Song" });
    expect(icon).toHaveAttribute("title", "Listening to Band – Song");
    share(null, own.session);
    expect(ownRow.querySelector('[data-icon="music"]')).toBeNull();
  });

  it("goes away when sharing stops, the person leaves or the server resyncs", async () => {
    share(song);
    share(null);
    expect(treeRow().querySelector('[data-icon="music"]')).toBeNull();

    share(song);
    const snapshot = await api.sessionSnapshot();
    if (!snapshot) throw new Error("not connected");
    act(() => {
      api.mock.emit("serverSynced", { snapshot });
    });
    expect(treeRow().querySelector('[data-icon="music"]')).toBeNull();
  });
});

/** The own session and name; the engine reports the own track with the own session. */
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
  it("shows cover, title, artist and a progress bar that moves on", async () => {
    api = await connectedMock();
    const cover = vi.spyOn(api, "nowPlayingCover").mockResolvedValue("data:image/png;base64,AAAA");
    vi.useFakeTimers({ shouldAdvanceTime: true, now: 1_000_000 });
    await renderCard();
    share(song);
    const block = await screen.findByRole("region", { name: "Now playing" });
    expect(within(block).getByText("Song")).toBeInTheDocument();
    expect(within(block).getByText("Band")).toBeInTheDocument();
    const bar = within(block).getByRole("progressbar", { name: "Playback progress" });
    expect(bar).toHaveAttribute("max", "200000");
    expect(bar).toHaveAttribute("value", "60000");
    expect(within(block).getByText("1:00 / 3:20")).toBeInTheDocument();
    expect(cover).toHaveBeenCalledWith(ANNA);
    await vi.waitFor(() => {
      expect(block.querySelector("img")).toHaveAttribute("src", "data:image/png;base64,AAAA");
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(Number(bar.getAttribute("value"))).toBeGreaterThanOrEqual(65_000);

    share(null);
    expect(screen.queryByRole("region", { name: "Now playing" })).toBeNull();
  });

  it("shows the own track on the own card", async () => {
    api = await connectedMock();
    const own = await self();
    const cover = vi.spyOn(api, "nowPlayingCover").mockResolvedValue(null);
    await renderCard(own.session, own.name);
    share(song, own.session);
    const block = await screen.findByRole("region", { name: "Now playing" });
    expect(within(block).getByText("Song")).toBeInTheDocument();
    expect(cover).toHaveBeenCalledWith(own.session);
  });

  it("loads no cover when the track has none and freezes while paused", async () => {
    api = await connectedMock();
    const cover = vi.spyOn(api, "nowPlayingCover");
    await renderCard();
    share({ ...song, hasCover: false, playing: false });
    const block = await screen.findByRole("region", { name: "Now playing" });
    expect(within(block).getByText("Paused")).toBeInTheDocument();
    expect(block.querySelector("img")).toBeNull();
    expect(cover).not.toHaveBeenCalled();
  });
});
