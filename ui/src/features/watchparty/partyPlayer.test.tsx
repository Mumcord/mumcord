/**
 * The party's player in the cinema view and in the mini bar (issue #163): both follow the party, so
 * Play and Pause from anywhere reach the player, and the mini bar keeps the video running.
 */
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, WatchParty } from "../../api";
import { renderWithEmbeds as render } from "../../app/testPreferences";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { leaveCinema, openMedia, resetMediaView } from "../media/mediaView";
import { resetKnownDuration } from "./knownDuration";
import { WatchMiniBar } from "./WatchMiniBar";
import { WatchTogetherView } from "./WatchTogetherView";
import { resetWatchPartyStore } from "./watchPartyStore";

const VIDEO = "dQw4w9WgXcQ";
const PAGE_ORIGIN = "http://localhost:43215";

describe("party player", () => {
  let api: MockApi;
  let channelId: number;

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    const { users, selfSession } = getServerState();
    channelId = users.get(selfSession ?? -1)?.channelId ?? -1;
    vi.spyOn(api, "linkPreview").mockRejectedValue(new Error("offline"));
    vi.spyOn(window, "addEventListener").mockClear();
    resetMediaView();
  });
  afterEach(() => {
    vi.restoreAllMocks();
    resetWatchPartyStore();
    resetMediaView();
    resetKnownDuration();
    resetAll();
  });

  const show = (patch: Partial<WatchParty>) => {
    act(() => {
      api.mock.emit("watchPartyChanged", {
        serverId: 1,
        party: {
          channelId,
          videoId: VIDEO,
          playing: false,
          positionMs: 0,
          participants: [1],
          lastAction: null,
          joined: true,
          currentEntry: 0,
          playlist: [],
          skippedBy: null,
          positions: [],
          host: 1,
          history: [],
          ...patch,
        },
      });
    });
  };

  /** The answering player frame and a spy on what the app sends to it. */
  async function readyPlayer() {
    const frame = await screen.findByTitle<HTMLIFrameElement>("Watch party player");
    await waitFor(() => {
      expect(vi.spyOn(window, "addEventListener")).toHaveBeenCalledWith(
        "message",
        expect.any(Function),
      );
    });
    const target = frame.contentWindow;
    if (target === null) throw new Error("no frame window");
    const post = vi.spyOn(target, "postMessage").mockImplementation(() => undefined);
    fireEvent.load(frame);
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", {
          origin: PAGE_ORIGIN,
          source: target,
          data: { type: "ready" },
        }),
      );
    });
    return { frame, post };
  }

  it("plays and pauses the cinema player when the party does", async () => {
    openMedia("watch");
    render(<WatchTogetherView serverId={1} />);
    show({ playing: false });
    const { post } = await readyPlayer();
    expect(post).toHaveBeenCalledWith({ type: "pause" }, PAGE_ORIGIN);
    show({ playing: true, positionMs: 0 });
    expect(post).toHaveBeenLastCalledWith({ type: "play" }, PAGE_ORIGIN);
    show({ playing: false, positionMs: 0 });
    expect(post).toHaveBeenLastCalledWith({ type: "pause" }, PAGE_ORIGIN);
  });

  it("pauses with Space while the party plays, before the player reported anything", async () => {
    const command = vi.spyOn(api, "watchPartyCommand").mockResolvedValue(null);
    openMedia("watch");
    render(<WatchTogetherView serverId={1} />);
    show({ playing: true });
    await readyPlayer();
    fireEvent.keyDown(document.body, { key: " " });
    expect(command).toHaveBeenCalledWith(1, expect.objectContaining({ kind: "pause" }));
  });

  it("keeps the video playing in the mini bar and drives it from its Play button", async () => {
    const command = vi.spyOn(api, "watchPartyCommand").mockResolvedValue(null);
    openMedia("watch");
    leaveCinema();
    render(<WatchMiniBar serverId={1} />);
    show({ playing: false, positionMs: 12_000 });
    const { frame, post } = await readyPlayer();
    expect(frame).toHaveAttribute("data-ready", "true");
    expect(post).toHaveBeenCalledWith({ type: "seek", seconds: 12 }, PAGE_ORIGIN);
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(command).toHaveBeenCalledWith(1, expect.objectContaining({ kind: "play" }));
    show({ playing: true, positionMs: 12_000 });
    expect(post).toHaveBeenLastCalledWith({ type: "play" }, PAGE_ORIGIN);
    expect(screen.getByRole("button", { name: "Pause" })).toBeVisible();
  });

  it("moves the seek bar with the player's reports and seeks the player when dragged", async () => {
    const command = vi.spyOn(api, "watchPartyCommand").mockResolvedValue(null);
    openMedia("watch");
    render(<WatchTogetherView serverId={1} />);
    show({ playing: true });
    const { frame, post } = await readyPlayer();
    const page = (data: unknown) => {
      act(() => {
        window.dispatchEvent(
          new MessageEvent("message", { origin: PAGE_ORIGIN, source: frame.contentWindow, data }),
        );
      });
    };
    page({ type: "meta", seconds: 200, bufferedSeconds: 50 });
    page({ type: "state", state: 1 });
    page({ type: "time", seconds: 42 });
    const seek = screen.getByRole("slider", { name: "Seek" });
    expect(seek).toHaveAttribute("max", "200000");
    expect(seek).toHaveValue("42000");
    fireEvent.change(seek, { target: { value: "120000" } });
    expect(post).toHaveBeenLastCalledWith({ type: "seek", seconds: 120 }, PAGE_ORIGIN);
    expect(command).toHaveBeenCalledWith(1, { kind: "seek", positionMs: 120_000 });
  });
});
